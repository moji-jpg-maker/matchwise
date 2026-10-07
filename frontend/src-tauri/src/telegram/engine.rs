//! The bot's brain: turns a Telegram update into replies. It talks only to the database and returns replies for
//! the caller to deliver, so it can be tested without a network.
//!
//! Rules it enforces:
//! * Only private chats are served. An unlinked chat can do nothing except use an invitation code.
//! * Nothing happens before the person has agreed to the privacy notice.
//! * Callback data is never trusted: every button press is re-checked against the database.
//! * Stored sensitive values are never displayed; scores, rules and other people's data are never shown.

use super::notify;
use super::render;
use super::repo::{self, Link, LinkError, MAX_FAILED_CODES};
use super::types::{button, CallbackQuery, InlineButton, Keyboard, Message, Reply, Update};
use crate::mm::match_commands::set_response_inner;
use crate::mm::match_repo::{MatchRepo, MatchRow};
use crate::mm::repository::{MmRepository, StoredProfile};
use matchmaking_core::{
    describe_preference, first_name, validate_preference, Condition, ConditionOp, FieldDef, FieldKind, FieldRegistry, Interest, MatchStatus,
    Preference, Provenance, Strength, Value,
};
use serde_json::{json, Value as Json};
use sqlx::SqlitePool;

type R = Result<Vec<Reply>, String>;

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

const MAX_TEXT_CHARS: usize = 2000;
const MAX_PREFERENCES: usize = 100;

pub async fn handle_update(pool: &SqlitePool, update: Update) -> R {
    if let Some(cb) = update.callback_query {
        return handle_callback(pool, cb).await;
    }
    if let Some(msg) = update.message {
        return handle_message(pool, msg).await;
    }
    Ok(vec![])
}

/// "/start abc" -> ("start", "abc"); "/help@MyBot" -> ("help", "").
fn parse_command(text: &str) -> Option<(String, String)> {
    let rest = text.strip_prefix('/')?;
    let (cmd, arg) = match rest.split_once(char::is_whitespace) {
        Some((c, a)) => (c, a.trim()),
        None => (rest, ""),
    };
    let cmd = cmd.split('@').next().unwrap_or("").to_lowercase();
    Some((cmd, arg.to_string()))
}

fn rows(buttons: Vec<InlineButton>, per_row: usize) -> Keyboard {
    buttons.chunks(per_row).map(|c| c.to_vec()).collect()
}

fn pretty(s: &str) -> String {
    s.replace('_', " ")
}

// ------------------------------------------------------------------ messages

async fn handle_message(pool: &SqlitePool, msg: Message) -> R {
    if msg.chat.kind != "private" {
        return Ok(vec![]);
    }
    let chat_id = msg.chat.id;
    let from = match &msg.from {
        Some(u) if !u.is_bot && u.id == chat_id => u.clone(),
        _ => return Ok(vec![]),
    };
    let Some(text) = msg.text.as_deref().map(str::trim).filter(|t| !t.is_empty()) else {
        return Ok(vec![Reply::text(chat_id, "I can only read text messages for now.")]);
    };
    if text.chars().count() > MAX_TEXT_CHARS {
        return Ok(vec![Reply::text(chat_id, "That message is too long. Please keep it under 2000 characters.")]);
    }
    match repo::link_by_chat(pool, chat_id).await.map_err(err)? {
        None => unlinked(pool, chat_id, from.id, from.username.as_deref(), text).await,
        Some(link) if !link.consented => awaiting_consent(pool, &link, text).await,
        Some(link) => linked(pool, &link, text).await,
    }
}

fn consent_prompt(chat_id: i64) -> Reply {
    Reply::with_keyboard(chat_id, render::consent_text(), vec![vec![button("I agree", "consent:y"), button("No thanks", "consent:n")]])
}

async fn unlinked(pool: &SqlitePool, chat_id: i64, user_id: i64, username: Option<&str>, text: &str) -> R {
    match parse_command(text) {
        Some((cmd, arg)) if cmd == "start" && !arg.is_empty() => {
            if repo::failed_attempts_last_hour(pool, chat_id).await.map_err(err)? >= MAX_FAILED_CODES {
                return Ok(vec![Reply::text(chat_id, render::too_many_attempts_text())]);
            }
            match repo::redeem_invite(pool, &arg).await.map_err(err)? {
                Err(_) => {
                    // The same message for every failure: do not reveal whether a code exists or was used.
                    repo::record_failed_attempt(pool, chat_id).await.map_err(err)?;
                    Ok(vec![Reply::text(chat_id, render::invalid_code_text())])
                }
                Ok(profile_id) => match repo::create_link(pool, &profile_id, chat_id, user_id, username).await {
                    Ok(()) => Ok(vec![consent_prompt(chat_id)]),
                    Err(LinkError::ProfileHasChat) => Ok(vec![Reply::text(chat_id, "This profile is already linked to a Telegram account. Please ask your matchmaker for help.")]),
                    Err(LinkError::ChatHasProfile) => Ok(vec![Reply::text(chat_id, "This chat is already linked to a profile.")]),
                    Err(LinkError::Db(e)) => Err(e),
                },
            }
        }
        Some((cmd, _)) if cmd == "help" => Ok(vec![Reply::text(chat_id, render::help_text())]),
        _ => Ok(vec![Reply::text(chat_id, render::unlinked_text())]),
    }
}

async fn awaiting_consent(pool: &SqlitePool, link: &Link, text: &str) -> R {
    let chat_id = link.chat_id;
    if let Some((cmd, _)) = parse_command(text) {
        if cmd == "stop" || cmd == "unlink" {
            repo::delete_link(pool, &link.profile_id).await.map_err(err)?;
            return Ok(vec![Reply::text(chat_id, render::consent_declined_text())]);
        }
    }
    Ok(vec![consent_prompt(chat_id)])
}

async fn load_profile(pool: &SqlitePool, id: &str) -> Result<StoredProfile, String> {
    MmRepository::get_profile(pool, id).await.map_err(err)?.ok_or_else(|| "Profile not found".to_string())
}

async fn linked(pool: &SqlitePool, link: &Link, text: &str) -> R {
    let chat_id = link.chat_id;
    if let Some((cmd, arg)) = parse_command(text) {
        if cmd != "cancel" {
            repo::clear_state(pool, chat_id).await.map_err(err)?;
        }
        return command(pool, link, &cmd, &arg).await;
    }
    if let Some(state) = repo::get_state(pool, chat_id).await.map_err(err)? {
        return flow_text(pool, link, state, text).await;
    }
    // Free text: it goes to the matchmaker.
    repo::add_message(pool, &link.profile_id, "in", text, None).await.map_err(err)?;
    Ok(vec![Reply::text(chat_id, "Thank you. I have passed your message to your matchmaker.")])
}

async fn command(pool: &SqlitePool, link: &Link, cmd: &str, arg: &str) -> R {
    let chat_id = link.chat_id;
    match cmd {
        "start" => {
            let p = load_profile(pool, &link.profile_id).await?;
            let name = first_name(&p.profile, true);
            let name = if name == "Someone" { "there".to_string() } else { name };
            Ok(vec![Reply::text(chat_id, render::welcome_text(&name))])
        }
        "help" => Ok(vec![Reply::text(chat_id, render::help_text())]),
        "cancel" => {
            repo::clear_state(pool, chat_id).await.map_err(err)?;
            Ok(vec![Reply::text(chat_id, "Okay, cancelled.")])
        }
        "profile" => {
            let reg = MmRepository::registry(pool).await.map_err(err)?;
            let p = load_profile(pool, &link.profile_id).await?;
            Ok(vec![Reply::text(chat_id, render::profile_summary(&p.profile, &reg))])
        }
        "edit" => start_edit(pool, link).await,
        "preferences" => show_preferences(pool, link).await,
        "matches" => show_matches(pool, link).await,
        "match" => open_match_by_number(pool, link, arg).await,
        "status" => show_status(pool, link).await,
        "settings" => show_settings(link),
        "stop" => {
            repo::set_notifications(pool, &link.profile_id, false).await.map_err(err)?;
            Ok(vec![Reply::text(chat_id, "I will not send you notifications any more. Send /resume to turn them back on. You can still use the commands.")])
        }
        "resume" => {
            repo::set_notifications(pool, &link.profile_id, true).await.map_err(err)?;
            Ok(vec![Reply::text(chat_id, "Notifications are on again.")])
        }
        "unlink" => Ok(vec![Reply::with_keyboard(
            chat_id,
            "Unlink this chat from your profile? You will stop receiving messages here. Your profile stays with your matchmaker.",
            vec![vec![button("Yes, unlink", "unlink:y"), button("Keep", "unlink:n")]],
        )]),
        "forget" => Ok(vec![Reply::with_keyboard(
            chat_id,
            "Ask your matchmaker to delete all your data? They will be notified of your request and can complete it in the app.",
            vec![vec![button("Yes, delete my data", "forget:y"), button("Keep", "forget:n")]],
        )]),
        _ => Ok(vec![Reply::text(chat_id, "I did not understand that. Send /help to see what I can do.")]),
    }
}

fn show_settings(link: &Link) -> R {
    let on = link.notifications_enabled;
    let kb = vec![
        vec![if on { button("Turn notifications off", "set:n:off") } else { button("Turn notifications on", "set:n:on") }],
        vec![button("Unlink this chat", "set:unlink"), button("Delete my data", "set:forget")],
    ];
    Ok(vec![Reply::with_keyboard(
        link.chat_id,
        format!("<b>Settings</b>\nNotifications: {}\n\nMessages here pass through Telegram. Your matchmaker can see everything you send.", if on { "on" } else { "off" }),
        kb,
    )])
}

// ------------------------------------------------------------------ introductions

const VISIBLE: [MatchStatus; 6] = [
    MatchStatus::IntroductionProposed,
    MatchStatus::BothInterested,
    MatchStatus::ContactExchanged,
    MatchStatus::Conversation,
    MatchStatus::Meeting,
    MatchStatus::Feedback,
];

/// Introductions this person can see: only pairs the matchmaker has actually introduced to them, oldest first.
async fn visible_matches(pool: &SqlitePool, profile_id: &str) -> Result<Vec<MatchRow>, String> {
    let mut v: Vec<MatchRow> = MatchRepo::list(pool)
        .await
        .map_err(err)?
        .into_iter()
        .map(|(m, _)| m)
        .filter(|m| (m.profile_a == profile_id || m.profile_b == profile_id) && VISIBLE.contains(&m.status))
        .collect();
    v.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    Ok(v)
}

fn my_side(m: &MatchRow, me: &str) -> (&'static str, Interest, String) {
    if m.profile_a == me { ("a", m.a_response, m.profile_b.clone()) } else { ("b", m.b_response, m.profile_a.clone()) }
}

async fn other_name(pool: &SqlitePool, other_id: &str) -> Result<String, String> {
    let p = load_profile(pool, other_id).await?;
    Ok(first_name(&p.profile, repo::show_first_name(pool).await.map_err(err)?))
}

async fn show_matches(pool: &SqlitePool, link: &Link) -> R {
    let ms = visible_matches(pool, &link.profile_id).await?;
    if ms.is_empty() {
        return Ok(vec![Reply::text(link.chat_id, "You have no introductions at the moment.")]);
    }
    let mut text = String::from("<b>Your introductions</b>\n");
    let mut buttons = vec![];
    for (i, m) in ms.iter().enumerate() {
        let (_, mine, other) = my_side(m, &link.profile_id);
        let name = other_name(pool, &other).await?;
        text.push_str(&format!("{}. {} - {}\n", i + 1, render::esc(&name), render::friendly_status(m.status, mine != Interest::Unknown, &name)));
        buttons.push(button(format!("Open {}", i + 1), format!("match:open:{}", m.id)));
    }
    Ok(vec![Reply::with_keyboard(link.chat_id, text, rows(buttons, 3))])
}

async fn open_match_by_number(pool: &SqlitePool, link: &Link, arg: &str) -> R {
    let ms = visible_matches(pool, &link.profile_id).await?;
    match arg.trim().parse::<usize>().ok().and_then(|n| n.checked_sub(1)).and_then(|i| ms.get(i)) {
        Some(m) => open_match(pool, link, m).await,
        None => Ok(vec![Reply::text(link.chat_id, "Send /matches to see your introductions, then /match followed by a number.")]),
    }
}

async fn open_match(pool: &SqlitePool, link: &Link, m: &MatchRow) -> R {
    let (_, mine, other) = my_side(m, &link.profile_id);
    let other_p = load_profile(pool, &other).await?;
    let card = notify::card_for(pool, &other_p).await?;
    let awaiting = m.status == MatchStatus::IntroductionProposed && mine == Interest::Unknown;
    let mut text = render::introduction_text(&card, awaiting);
    if !awaiting {
        text.push_str(&format!("\nStatus: {}", render::friendly_status(m.status, mine != Interest::Unknown, &card.title)));
    }
    Ok(vec![if awaiting { Reply::with_keyboard(link.chat_id, text, notify::intro_keyboard(&m.id)) } else { Reply::text(link.chat_id, text) }])
}

async fn show_status(pool: &SqlitePool, link: &Link) -> R {
    let reg = MmRepository::registry(pool).await.map_err(err)?;
    let p = load_profile(pool, &link.profile_id).await?;
    let mut text = String::from("<b>Status</b>\n");
    match p.profile.completeness(&reg) {
        Some(c) if c < 1.0 => text.push_str(&format!("Profile: {}% complete (send /edit to finish)\n", (c * 100.0).round())),
        _ => text.push_str("Profile: complete\n"),
    }
    let ms = visible_matches(pool, &link.profile_id).await?;
    if ms.is_empty() {
        text.push_str("Introductions: none at the moment\n");
    }
    for (i, m) in ms.iter().enumerate() {
        let (_, mine, other) = my_side(m, &link.profile_id);
        let name = other_name(pool, &other).await?;
        text.push_str(&format!("{}. {} - {}\n", i + 1, render::esc(&name), render::friendly_status(m.status, mine != Interest::Unknown, &name)));
    }
    text.push_str(&format!("Notifications: {}", if link.notifications_enabled { "on" } else { "off" }));
    Ok(vec![Reply::text(link.chat_id, text)])
}

// ------------------------------------------------------------------ editing the profile

fn editable(def: &FieldDef) -> bool {
    !matches!(def.kind, FieldKind::Records(_))
}

async fn start_edit(pool: &SqlitePool, link: &Link) -> R {
    let reg = MmRepository::registry(pool).await.map_err(err)?;
    let p = load_profile(pool, &link.profile_id).await?;
    let queue: Vec<String> = p.profile.missing_required(&reg).into_iter().filter(|k| reg.get(k).map_or(false, editable)).collect();
    if let Some(first) = queue.first().cloned() {
        repo::set_state(pool, link.chat_id, &json!({"flow": "edit", "queue": queue, "current": first, "multi": []})).await.map_err(err)?;
        let mut out = vec![Reply::text(link.chat_id, "Let's complete your profile. You can skip any question, or send /cancel to stop.")];
        out.push(ask_field(link.chat_id, reg.get(&first).ok_or("field missing")?, &[]));
        return Ok(out);
    }
    // everything required is filled: offer to change anything
    let buttons: Vec<InlineButton> = reg.defs().filter(|d| editable(d)).take(30).map(|d| button(d.label.clone(), format!("edit:f:{}", d.key))).collect();
    Ok(vec![Reply::with_keyboard(link.chat_id, "Your profile is complete. What would you like to change?", rows(buttons, 2))])
}

fn ask_field(chat_id: i64, def: &FieldDef, selected: &[String]) -> Reply {
    let mut text = format!("<b>{}</b>\n", render::esc(&def.label));
    if def.sensitive {
        text.push_str("This is sensitive information and Telegram will carry it. Skip it if you prefer to tell your matchmaker in person.\n");
    }
    let skip = button("Skip", "edit:skip");
    match &def.kind {
        FieldKind::Choice(opts) => {
            text.push_str("Choose one:");
            let mut kb = rows(opts.iter().enumerate().map(|(i, o)| button(pretty(o), format!("edit:c:{i}"))).collect(), 2);
            kb.push(vec![skip]);
            Reply::with_keyboard(chat_id, text, kb)
        }
        FieldKind::MultiChoice(opts) => {
            text.push_str("Choose all that apply, then tap Done:");
            let mut kb = rows(
                opts.iter()
                    .enumerate()
                    .map(|(i, o)| button(format!("{}{}", if selected.contains(o) { "✓ " } else { "" }, pretty(o)), format!("edit:m:{i}")))
                    .collect(),
                2,
            );
            kb.push(vec![button("Done", "edit:done"), skip]);
            Reply::with_keyboard(chat_id, text, kb)
        }
        FieldKind::Bool => {
            text.push_str("Yes or no?");
            Reply::with_keyboard(chat_id, text, vec![vec![button("Yes", "edit:b:1"), button("No", "edit:b:0")], vec![skip]])
        }
        FieldKind::Number => {
            text.push_str("Send a number.");
            Reply::with_keyboard(chat_id, text, vec![vec![skip]])
        }
        _ => {
            text.push_str("Type your answer.");
            Reply::with_keyboard(chat_id, text, vec![vec![skip]])
        }
    }
}

fn parse_typed(def: &FieldDef, text: &str) -> Option<Value> {
    let t = text.trim();
    match &def.kind {
        FieldKind::Number => t.replace(',', ".").parse::<f64>().ok().filter(|n| n.is_finite()).map(Value::Num),
        FieldKind::Text => (t.chars().count() <= 500).then(|| Value::Text(t.to_string())),
        FieldKind::Bool => match t.to_lowercase().as_str() {
            "yes" | "y" | "true" | "1" => Some(Value::Bool(true)),
            "no" | "n" | "false" | "0" => Some(Value::Bool(false)),
            _ => None,
        },
        FieldKind::Choice(opts) => opts.iter().find(|o| pretty(o).eq_ignore_ascii_case(t) || o.eq_ignore_ascii_case(t)).map(|o| Value::Text(o.clone())),
        FieldKind::MultiChoice(opts) => {
            let picked: Option<Vec<String>> = t
                .split(',')
                .map(|p| p.trim())
                .filter(|p| !p.is_empty())
                .map(|p| opts.iter().find(|o| pretty(o).eq_ignore_ascii_case(p) || o.eq_ignore_ascii_case(p)).cloned())
                .collect();
            picked.filter(|v| !v.is_empty()).map(Value::List)
        }
        FieldKind::Records(_) => None,
    }
}

fn state_str(st: &Json, key: &str) -> Option<String> {
    st.get(key).and_then(|v| v.as_str()).map(|s| s.to_string())
}

fn state_list(st: &Json, key: &str) -> Vec<String> {
    st.get(key).and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default()
}

/// Save one answer. Returns a short message for the person (never the stored value).
async fn save_answer(pool: &SqlitePool, link: &Link, reg: &FieldRegistry, key: &str, value: Value) -> Result<Result<String, String>, String> {
    if let Err(e) = reg.validate_value(key, &value) {
        return Ok(Err(e));
    }
    let mut p = load_profile(pool, &link.profile_id).await?;
    if !p.profile.set(key, value, Provenance::User) {
        return Ok(Ok("Your matchmaker has already filled this in, so only they can change it. I kept it as it is.".to_string()));
    }
    MmRepository::update_profile_data(pool, &p.profile).await.map_err(err)?;
    MmRepository::audit(pool, "update", "profile", &link.profile_id, Some(key)).await;
    Ok(Ok("Saved.".to_string()))
}

/// Move to the next question, or finish.
async fn advance_edit(pool: &SqlitePool, link: &Link, reg: &FieldRegistry, st: &Json, mut out: Vec<Reply>) -> R {
    let mut queue = state_list(st, "queue");
    if !queue.is_empty() {
        queue.remove(0);
    }
    if let Some(next) = queue.first().cloned() {
        repo::set_state(pool, link.chat_id, &json!({"flow": "edit", "queue": queue, "current": next, "multi": []})).await.map_err(err)?;
        out.push(ask_field(link.chat_id, reg.get(&next).ok_or("field missing")?, &[]));
        return Ok(out);
    }
    repo::clear_state(pool, link.chat_id).await.map_err(err)?;
    let p = load_profile(pool, &link.profile_id).await?;
    out.push(Reply::text(
        link.chat_id,
        match p.profile.completeness(reg) {
            Some(c) if c < 1.0 => format!("Thank you. Your profile is {}% complete; send /edit when you are ready to add the rest.", (c * 100.0).round()),
            _ => "Thank you. Your profile is complete.".to_string(),
        },
    ));
    Ok(out)
}

async fn flow_text(pool: &SqlitePool, link: &Link, st: Json, text: &str) -> R {
    let chat_id = link.chat_id;
    match state_str(&st, "flow").as_deref() {
        Some("edit") => {
            let reg = MmRepository::registry(pool).await.map_err(err)?;
            let Some(key) = state_str(&st, "current") else { return Ok(vec![]) };
            let def = reg.get(&key).ok_or("field missing")?.clone();
            let Some(value) = parse_typed(&def, text) else {
                return Ok(vec![Reply::text(chat_id, "I could not read that."), ask_field(chat_id, &def, &state_list(&st, "multi"))]);
            };
            match save_answer(pool, link, &reg, &key, value).await? {
                Err(msg) => Ok(vec![Reply::text(chat_id, format!("That does not look right: {}", render::esc(&msg))), ask_field(chat_id, &def, &[])]),
                Ok(msg) => advance_edit(pool, link, &reg, &st, vec![Reply::text(chat_id, msg)]).await,
            }
        }
        Some("pref") => pref_text(pool, link, st, text).await,
        Some("reply") => {
            let match_id = state_str(&st, "match_id");
            repo::add_message(pool, &link.profile_id, "in", text, match_id.as_deref()).await.map_err(err)?;
            repo::clear_state(pool, chat_id).await.map_err(err)?;
            Ok(vec![Reply::text(chat_id, "Thank you, I have passed that to your matchmaker.")])
        }
        _ => {
            repo::clear_state(pool, chat_id).await.map_err(err)?;
            repo::add_message(pool, &link.profile_id, "in", text, None).await.map_err(err)?;
            Ok(vec![Reply::text(chat_id, "Thank you. I have passed your message to your matchmaker.")])
        }
    }
}

// ------------------------------------------------------------------ preferences

fn pref_fields(reg: &FieldRegistry) -> Vec<&FieldDef> {
    reg.defs().filter(|d| matches!(d.kind, FieldKind::Number | FieldKind::Bool | FieldKind::Choice(_) | FieldKind::MultiChoice(_))).take(30).collect()
}

async fn show_preferences(pool: &SqlitePool, link: &Link) -> R {
    let reg = MmRepository::registry(pool).await.map_err(err)?;
    let prefs = MmRepository::get_preferences(pool, &link.profile_id).await.map_err(err)?;
    let mut text = String::from("<b>What you are looking for</b>\n");
    if prefs.is_empty() {
        text.push_str("Nothing set yet.\n");
    }
    let mut kb: Keyboard = vec![];
    for (i, p) in prefs.iter().enumerate() {
        text.push_str(&format!("{}. {}\n", i + 1, render::esc(&describe_preference(p, &reg))));
        kb.push(vec![button(format!("Remove {}", i + 1), format!("pref:del:{}", p.id))]);
    }
    kb.push(vec![button("Add something", "pref:add")]);
    Ok(vec![Reply::with_keyboard(link.chat_id, text, kb)])
}

fn strength_keyboard() -> Keyboard {
    vec![
        vec![button("Must have", "pref:s:required"), button("Deal-breaker", "pref:s:deal_breaker")],
        vec![button("Preferred", "pref:s:preferred"), button("Nice to have", "pref:s:flexible")],
    ]
}

async fn finish_preference(pool: &SqlitePool, link: &Link, st: &Json, strength: Strength) -> R {
    let chat_id = link.chat_id;
    let reg = MmRepository::registry(pool).await.map_err(err)?;
    let field = state_str(st, "field").ok_or("state lost")?;
    let def = reg.get(&field).ok_or("field missing")?;
    let cond = match &def.kind {
        FieldKind::Choice(_) | FieldKind::MultiChoice(_) => Condition { field: field.clone(), op: ConditionOp::In, value: Some(Value::List(state_list(st, "values"))), value2: None, value_rel: None, value2_rel: None },
        FieldKind::Bool => Condition { field: field.clone(), op: ConditionOp::Eq, value: Some(Value::Bool(st.get("bool").and_then(|v| v.as_bool()).unwrap_or(true))), value2: None, value_rel: None, value2_rel: None },
        _ => {
            let lo = st.get("lo").and_then(|v| v.as_f64());
            let hi = st.get("hi").and_then(|v| v.as_f64());
            let (op, value, value2) = match (lo, hi) {
                (Some(l), Some(h)) => (ConditionOp::Between, Some(Value::Num(l)), Some(Value::Num(h))),
                (Some(l), None) => (ConditionOp::Ge, Some(Value::Num(l)), None),
                (None, Some(h)) => (ConditionOp::Le, Some(Value::Num(h)), None),
                (None, None) => {
                    repo::clear_state(pool, chat_id).await.map_err(err)?;
                    return Ok(vec![Reply::text(chat_id, "Nothing was set, so I did not add anything.")]);
                }
            };
            Condition { field: field.clone(), op, value, value2, value_rel: None, value2_rel: None }
        }
    };
    let pref = Preference { id: uuid::Uuid::new_v4().to_string(), condition: cond, strength, importance: 3, note: None };
    if let Err(e) = validate_preference(&pref, &reg) {
        repo::clear_state(pool, chat_id).await.map_err(err)?;
        return Ok(vec![Reply::text(chat_id, format!("I could not add that: {}", render::esc(&e)))]);
    }
    let mut prefs = MmRepository::get_preferences(pool, &link.profile_id).await.map_err(err)?;
    if prefs.len() >= MAX_PREFERENCES {
        repo::clear_state(pool, chat_id).await.map_err(err)?;
        return Ok(vec![Reply::text(chat_id, "You already have the maximum number of preferences.")]);
    }
    let described = describe_preference(&pref, &reg);
    prefs.push(pref);
    MmRepository::save_preferences(pool, &link.profile_id, &prefs).await.map_err(err)?;
    MmRepository::audit(pool, "update_preferences", "profile", &link.profile_id, Some(&format!("{} items", prefs.len()))).await;
    repo::clear_state(pool, chat_id).await.map_err(err)?;
    Ok(vec![Reply::text(chat_id, format!("Added: {}\nSend /preferences to review.", render::esc(&described)))])
}

async fn pref_text(pool: &SqlitePool, link: &Link, mut st: Json, text: &str) -> R {
    let chat_id = link.chat_id;
    let step = state_str(&st, "step").unwrap_or_default();
    if step != "lo" && step != "hi" {
        return Ok(vec![Reply::text(chat_id, "Please use the buttons, or send /cancel.")]);
    }
    let t = text.trim();
    let value = if t.eq_ignore_ascii_case("skip") { Some(Json::Null) } else { t.replace(',', ".").parse::<f64>().ok().filter(|n| n.is_finite()).map(|n| json!(n)) };
    let Some(value) = value else {
        return Ok(vec![Reply::text(chat_id, "Please send a number, or the word skip.")]);
    };
    st[&step] = value;
    if step == "lo" {
        st["step"] = json!("hi");
        repo::set_state(pool, chat_id, &st).await.map_err(err)?;
        return Ok(vec![Reply::text(chat_id, "And the highest? Send a number, or the word skip.")]);
    }
    st["step"] = json!("strength");
    repo::set_state(pool, chat_id, &st).await.map_err(err)?;
    Ok(vec![Reply::with_keyboard(chat_id, "How important is this?", strength_keyboard())])
}

// ------------------------------------------------------------------ buttons

/// Flows redraw the buttons of the message that was tapped; they cannot know its id, so it is filled in here.
fn with_message_id(replies: Vec<Reply>, chat_id: i64, message_id: Option<i64>) -> Vec<Reply> {
    replies
        .into_iter()
        .filter_map(|r| match r {
            Reply::EditMarkup { message_id: 0, keyboard, .. } => message_id.map(|mid| Reply::EditMarkup { chat_id, message_id: mid, keyboard }),
            other => Some(other),
        })
        .collect()
}

fn toast(id: &str, text: &str) -> Reply {
    Reply::AnswerCallback { id: id.to_string(), text: Some(text.to_string()) }
}

fn done(id: &str) -> Reply {
    Reply::AnswerCallback { id: id.to_string(), text: None }
}

async fn handle_callback(pool: &SqlitePool, cb: CallbackQuery) -> R {
    let (chat_id, message_id) = match &cb.message {
        Some(m) => (m.chat.id, Some(m.message_id)),
        None => (cb.from.id, None),
    };
    if cb.message.as_ref().map_or(false, |m| m.chat.kind != "private") || cb.from.is_bot || cb.from.id != chat_id {
        return Ok(vec![done(&cb.id)]);
    }
    let Some(link) = repo::link_by_chat(pool, chat_id).await.map_err(err)? else {
        return Ok(vec![toast(&cb.id, "This chat is not linked to a profile.")]);
    };
    let data = cb.data.clone().unwrap_or_default();
    let (verb, rest) = data.split_once(':').unwrap_or((data.as_str(), ""));
    let clear = |kb: Option<Keyboard>| message_id.map(|mid| Reply::EditMarkup { chat_id, message_id: mid, keyboard: kb });
    let mut out: Vec<Reply> = vec![];

    if verb == "consent" {
        if link.consented {
            return Ok(vec![toast(&cb.id, "Already answered.")]);
        }
        out.push(done(&cb.id));
        out.extend(clear(None));
        if rest == "y" {
            repo::set_consent(pool, &link.profile_id).await.map_err(err)?;
            MmRepository::audit(pool, "telegram_consent", "profile", &link.profile_id, Some(&format!("v{}", repo::CONSENT_VERSION))).await;
            let p = load_profile(pool, &link.profile_id).await?;
            let name = first_name(&p.profile, true);
            out.push(Reply::text(chat_id, render::welcome_text(&if name == "Someone" { "there".to_string() } else { name })));
        } else {
            repo::delete_link(pool, &link.profile_id).await.map_err(err)?;
            out.push(Reply::text(chat_id, render::consent_declined_text()));
        }
        return Ok(out);
    }
    if !link.consented {
        return Ok(vec![toast(&cb.id, "Please answer the privacy notice first.")]);
    }

    match verb {
        "intro" => {
            let Some((answer, match_id)) = rest.split_once(':') else { return Ok(vec![toast(&cb.id, "This button is no longer valid.")]) };
            let Some(m) = MatchRepo::get(pool, match_id).await.map_err(err)? else { return Ok(vec![toast(&cb.id, "This introduction is no longer available.")]) };
            if m.profile_a != link.profile_id && m.profile_b != link.profile_id {
                // Someone pressed a button for a match that is not theirs: refuse without saying anything about it.
                return Ok(vec![toast(&cb.id, "This button is no longer valid.")]);
            }
            let (side, mine, other) = my_side(&m, &link.profile_id);
            if !matchmaking_core::can_record_responses(m.status) {
                out.push(toast(&cb.id, "This introduction is no longer open."));
                out.extend(clear(None));
                return Ok(out);
            }
            let response = match answer {
                "y" => Interest::Interested,
                "n" => Interest::NotInterested,
                _ => return Ok(vec![toast(&cb.id, "This button is no longer valid.")]),
            };
            if mine == response {
                return Ok(vec![toast(&cb.id, "Already recorded.")]);
            }
            let (_, new_status) = set_response_inner(pool, &m.id, side, response).await?;
            out.push(done(&cb.id));
            out.extend(clear(None));
            match (response, new_status) {
                // both-interested and declined notices come from the notification layer, to avoid saying things twice
                (Interest::Interested, MatchStatus::BothInterested) => {}
                (Interest::NotInterested, _) => out.push(Reply::text(chat_id, "Understood, thank you. I have noted that you are not interested. Nothing more will be shared.")),
                (Interest::Interested, _) => {
                    let name = other_name(pool, &other).await?;
                    out.push(Reply::text(chat_id, format!("Thank you! I will let you know when {} has answered.", render::esc(&name))));
                }
                _ => {}
            }
            Ok(out)
        }
        "match" => {
            let Some(match_id) = rest.strip_prefix("open:") else { return Ok(vec![toast(&cb.id, "This button is no longer valid.")]) };
            let ms = visible_matches(pool, &link.profile_id).await?;
            match ms.iter().find(|m| m.id == match_id) {
                Some(m) => {
                    out.push(done(&cb.id));
                    out.extend(open_match(pool, &link, m).await?);
                    Ok(out)
                }
                None => Ok(vec![toast(&cb.id, "This introduction is no longer available.")]),
            }
        }
        "set" => match rest {
            "n:on" | "n:off" => {
                repo::set_notifications(pool, &link.profile_id, rest == "n:on").await.map_err(err)?;
                out.push(toast(&cb.id, if rest == "n:on" { "Notifications on" } else { "Notifications off" }));
                let refreshed = Link { notifications_enabled: rest == "n:on", ..link.clone() };
                out.extend(clear(None));
                out.extend(show_settings(&refreshed)?);
                Ok(out)
            }
            "unlink" => Ok(vec![done(&cb.id), Reply::with_keyboard(chat_id, "Unlink this chat from your profile?", vec![vec![button("Yes, unlink", "unlink:y"), button("Keep", "unlink:n")]])]),
            "forget" => Ok(vec![done(&cb.id), Reply::with_keyboard(chat_id, "Ask your matchmaker to delete all your data?", vec![vec![button("Yes, delete my data", "forget:y"), button("Keep", "forget:n")]])]),
            _ => Ok(vec![toast(&cb.id, "This button is no longer valid.")]),
        },
        "unlink" => {
            out.push(done(&cb.id));
            out.extend(clear(None));
            if rest == "y" {
                repo::delete_link(pool, &link.profile_id).await.map_err(err)?;
                MmRepository::audit(pool, "telegram_unlink", "profile", &link.profile_id, None).await;
                out.push(Reply::text(chat_id, "Done. This chat is no longer linked. Your matchmaker can send a new invitation if you want to reconnect."));
            } else {
                out.push(Reply::text(chat_id, "Okay, nothing changed."));
            }
            Ok(out)
        }
        "forget" => {
            out.push(done(&cb.id));
            out.extend(clear(None));
            if rest == "y" {
                let created = repo::add_request(pool, &link.profile_id, "deletion").await.map_err(err)?;
                if created {
                    MmRepository::audit(pool, "data_deletion_requested", "profile", &link.profile_id, None).await;
                }
                out.push(Reply::text(chat_id, "I have asked your matchmaker to delete your data. They will confirm once it is done."));
            } else {
                out.push(Reply::text(chat_id, "Okay, nothing changed."));
            }
            Ok(out)
        }
        "edit" => Ok(with_message_id(edit_callback(pool, &link, &cb.id, rest, out).await?, chat_id, message_id)),
        "pref" => Ok(with_message_id(pref_callback(pool, &link, &cb.id, rest, out).await?, chat_id, message_id)),
        _ => Ok(vec![toast(&cb.id, "This button is no longer valid.")]),
    }
}

async fn edit_callback(pool: &SqlitePool, link: &Link, cb_id: &str, rest: &str, mut out: Vec<Reply>) -> R {
    let chat_id = link.chat_id;
    let reg = MmRepository::registry(pool).await.map_err(err)?;
    out.push(done(cb_id));

    // "edit:f:<key>" starts a single-field edit from the menu
    if let Some(key) = rest.strip_prefix("f:") {
        let Some(def) = reg.get(key).filter(|d| editable(d)) else { return Ok(vec![toast(cb_id, "This button is no longer valid.")]) };
        repo::set_state(pool, chat_id, &json!({"flow": "edit", "queue": [key], "current": key, "multi": []})).await.map_err(err)?;
        out.push(ask_field(chat_id, def, &[]));
        return Ok(out);
    }
    let Some(st) = repo::get_state(pool, chat_id).await.map_err(err)?.filter(|s| state_str(s, "flow").as_deref() == Some("edit")) else {
        return Ok(vec![toast(cb_id, "This question is no longer open. Send /edit to start again.")]);
    };
    let Some(key) = state_str(&st, "current") else { return Ok(vec![toast(cb_id, "This question is no longer open.")]) };
    let def = reg.get(&key).ok_or("field missing")?.clone();

    if rest == "skip" {
        return advance_edit(pool, link, &reg, &st, out).await;
    }
    let value: Option<Value> = match (rest.split_once(':'), &def.kind) {
        (Some(("c", i)), FieldKind::Choice(opts)) => i.parse::<usize>().ok().and_then(|i| opts.get(i)).map(|o| Value::Text(o.clone())),
        (Some(("b", v)), FieldKind::Bool) => Some(Value::Bool(v == "1")),
        (Some(("m", i)), FieldKind::MultiChoice(opts)) => {
            // toggle one option and redraw the buttons
            let Some(opt) = i.parse::<usize>().ok().and_then(|i| opts.get(i)).cloned() else { return Ok(vec![toast(cb_id, "This button is no longer valid.")]) };
            let mut sel = state_list(&st, "multi");
            if let Some(pos) = sel.iter().position(|s| *s == opt) { sel.remove(pos); } else { sel.push(opt); }
            let mut st2 = st.clone();
            st2["multi"] = json!(sel);
            repo::set_state(pool, chat_id, &st2).await.map_err(err)?;
            out.pop();
            out.push(done(cb_id));
            if let Reply::Send { keyboard, .. } = ask_field(chat_id, &def, &sel) {
                out.push(Reply::EditMarkup { chat_id, message_id: 0, keyboard }); // caller fills in the message id
            }
            return Ok(out);
        }
        _ if rest == "done" && matches!(def.kind, FieldKind::MultiChoice(_)) => {
            let sel = state_list(&st, "multi");
            if sel.is_empty() { None } else { Some(Value::List(sel)) }
        }
        _ => None,
    };
    let Some(value) = value else { return Ok(vec![toast(cb_id, "Please choose at least one option, or Skip.")]) };
    match save_answer(pool, link, &reg, &key, value).await? {
        Err(msg) => {
            out.push(Reply::text(chat_id, format!("That does not look right: {}", render::esc(&msg))));
            Ok(out)
        }
        Ok(msg) => {
            out.push(Reply::text(chat_id, msg));
            advance_edit(pool, link, &reg, &st, out).await
        }
    }
}

async fn pref_callback(pool: &SqlitePool, link: &Link, cb_id: &str, rest: &str, mut out: Vec<Reply>) -> R {
    let chat_id = link.chat_id;
    let reg = MmRepository::registry(pool).await.map_err(err)?;
    out.push(done(cb_id));

    if rest == "add" {
        let buttons: Vec<InlineButton> = pref_fields(&reg).into_iter().map(|d| button(d.label.clone(), format!("pref:f:{}", d.key))).collect();
        repo::set_state(pool, chat_id, &json!({"flow": "pref", "step": "field"})).await.map_err(err)?;
        out.push(Reply::with_keyboard(chat_id, "What is it about?", rows(buttons, 2)));
        return Ok(out);
    }
    if let Some(id) = rest.strip_prefix("del:") {
        let mut prefs = MmRepository::get_preferences(pool, &link.profile_id).await.map_err(err)?;
        let before = prefs.len();
        prefs.retain(|p| p.id != id);
        if prefs.len() == before {
            return Ok(vec![toast(cb_id, "That was already removed.")]);
        }
        MmRepository::save_preferences(pool, &link.profile_id, &prefs).await.map_err(err)?;
        MmRepository::audit(pool, "update_preferences", "profile", &link.profile_id, Some(&format!("{} items", prefs.len()))).await;
        out.extend(show_preferences(pool, link).await?);
        return Ok(out);
    }
    let Some(mut st) = repo::get_state(pool, chat_id).await.map_err(err)?.filter(|s| state_str(s, "flow").as_deref() == Some("pref")) else {
        return Ok(vec![toast(cb_id, "This question is no longer open. Send /preferences to start again.")]);
    };

    if let Some(key) = rest.strip_prefix("f:") {
        let Some(def) = pref_fields(&reg).into_iter().find(|d| d.key == key).cloned() else { return Ok(vec![toast(cb_id, "This button is no longer valid.")]) };
        st = json!({"flow": "pref", "step": "value", "field": key, "values": []});
        match &def.kind {
            FieldKind::Choice(opts) | FieldKind::MultiChoice(opts) => {
                repo::set_state(pool, chat_id, &st).await.map_err(err)?;
                let mut kb = rows(opts.iter().enumerate().map(|(i, o)| button(pretty(o), format!("pref:m:{i}"))).collect(), 2);
                kb.push(vec![button("Done", "pref:done")]);
                out.push(Reply::with_keyboard(chat_id, format!("<b>{}</b>\nTap every option you mean, then Done:", render::esc(&def.label)), kb));
            }
            FieldKind::Bool => {
                repo::set_state(pool, chat_id, &st).await.map_err(err)?;
                out.push(Reply::with_keyboard(chat_id, format!("<b>{}</b>", render::esc(&def.label)), vec![vec![button("Yes", "pref:b:1"), button("No", "pref:b:0")]]));
            }
            _ => {
                st["step"] = json!("lo");
                repo::set_state(pool, chat_id, &st).await.map_err(err)?;
                out.push(Reply::text(chat_id, format!("<b>{}</b>\nWhat is the lowest you would accept? Send a number, or the word skip.", render::esc(&def.label))));
            }
        }
        return Ok(out);
    }
    let field = state_str(&st, "field").unwrap_or_default();
    let Some(def) = reg.get(&field).cloned() else { return Ok(vec![toast(cb_id, "This question is no longer open.")]) };

    if let Some(i) = rest.strip_prefix("m:") {
        let (FieldKind::Choice(opts) | FieldKind::MultiChoice(opts)) = &def.kind else { return Ok(vec![toast(cb_id, "This button is no longer valid.")] ) };
        let Some(opt) = i.parse::<usize>().ok().and_then(|i| opts.get(i)).cloned() else { return Ok(vec![toast(cb_id, "This button is no longer valid.")]) };
        let mut vals = state_list(&st, "values");
        if let Some(pos) = vals.iter().position(|s| *s == opt) { vals.remove(pos); } else { vals.push(opt); }
        st["values"] = json!(vals);
        repo::set_state(pool, chat_id, &st).await.map_err(err)?;
        let mut kb = rows(opts.iter().enumerate().map(|(i, o)| button(format!("{}{}", if vals.contains(o) { "✓ " } else { "" }, pretty(o)), format!("pref:m:{i}"))).collect(), 2);
        kb.push(vec![button("Done", "pref:done")]);
        out.push(Reply::EditMarkup { chat_id, message_id: 0, keyboard: Some(kb) }); // caller fills in the message id
        return Ok(out);
    }
    if rest == "done" {
        if state_list(&st, "values").is_empty() {
            return Ok(vec![toast(cb_id, "Choose at least one option first.")]);
        }
        st["step"] = json!("strength");
        repo::set_state(pool, chat_id, &st).await.map_err(err)?;
        out.push(Reply::with_keyboard(chat_id, "How important is this?", strength_keyboard()));
        return Ok(out);
    }
    if let Some(v) = rest.strip_prefix("b:") {
        st["bool"] = json!(v == "1");
        st["step"] = json!("strength");
        repo::set_state(pool, chat_id, &st).await.map_err(err)?;
        out.push(Reply::with_keyboard(chat_id, "How important is this?", strength_keyboard()));
        return Ok(out);
    }
    if let Some(s) = rest.strip_prefix("s:") {
        if state_str(&st, "step").as_deref() != Some("strength") {
            return Ok(vec![toast(cb_id, "Please answer the earlier question first.")]);
        }
        let strength = match s {
            "required" => Strength::Required,
            "deal_breaker" => Strength::DealBreaker,
            "preferred" => Strength::Preferred,
            "flexible" => Strength::Flexible,
            _ => return Ok(vec![toast(cb_id, "This button is no longer valid.")]),
        };
        out.extend(finish_preference(pool, link, &st, strength).await?);
        return Ok(out);
    }
    Ok(vec![toast(cb_id, "This button is no longer valid.")])
}
