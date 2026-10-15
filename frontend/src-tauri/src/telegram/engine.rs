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
use crate::questionnaires::{repo as qrepo, service as qservice};
use crate::mm::match_repo::{MatchRepo, MatchRow};
use crate::mm::repository::{MmRepository, StoredProfile};
use matchmaking_core::{
    describe_preference_for, first_name_for, first_name_opt, localize_digits, validate_preference_for, Condition, ConditionOp, FieldDef, FieldKind,
    FieldRegistry, Interest, Locale, MatchStatus, Preference, Profile, Provenance, Strength, Value,
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

fn num(loc: Locale, value: impl std::fmt::Display) -> String {
    localize_digits(loc, &value.to_string())
}

fn greeting_name(profile: &Profile, loc: Locale) -> String {
    first_name_opt(profile, true).unwrap_or_else(|| loc.pick("there", "دوست عزیز").to_string())
}

fn normalize_digits(text: &str) -> String {
    text.chars()
        .filter_map(|ch| match ch {
            '۰'..='۹' => char::from_digit(ch as u32 - '۰' as u32, 10),
            '٠'..='٩' => char::from_digit(ch as u32 - '٠' as u32, 10),
            '٫' | ',' => Some('.'),
            '٬' => None,
            other => Some(other),
        })
        .collect()
}

fn parse_number(text: &str) -> Option<f64> {
    normalize_digits(text.trim()).parse::<f64>().ok().filter(|n| n.is_finite())
}

fn is_skip_word(text: &str) -> bool {
    let word = text.trim();
    word.eq_ignore_ascii_case("skip") || word == "رد"
}

fn parse_bool_word(text: &str) -> Option<bool> {
    match text.trim().to_lowercase().as_str() {
        "yes" | "y" | "true" | "1" | "۱" | "بله" | "بلی" | "آره" | "اره" => Some(true),
        "no" | "n" | "false" | "0" | "۰" | "خیر" | "نه" => Some(false),
        _ => None,
    }
}

fn language_keyboard() -> Keyboard {
    vec![vec![button("English", "lang:en"), button("فارسی", "lang:fa")]]
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
    let existing = repo::link_by_chat(pool, chat_id).await.map_err(err)?;
    let loc = existing.as_ref().map_or_else(|| Locale::parse_or_default(from.language_code.as_deref().unwrap_or("")), |link| link.locale);
    let Some(text) = msg.text.as_deref().map(str::trim).filter(|t| !t.is_empty()) else {
        return Ok(vec![Reply::text(chat_id, loc.pick("I can only read text messages for now.", "فعلاً فقط پیام‌های متنی را می‌توانم بخوانم."))]);
    };
    if text.chars().count() > MAX_TEXT_CHARS {
        return Ok(vec![Reply::text(
            chat_id,
            loc.pick("That message is too long. Please keep it under 2000 characters.", "این پیام خیلی طولانی است. لطفاً آن را زیر ۲۰۰۰ نویسه نگه دارید."),
        )]);
    }
    match existing {
        None => unlinked(pool, chat_id, from.id, from.username.as_deref(), loc, text).await,
        Some(link) if !link.consented => awaiting_consent(pool, &link, text).await,
        Some(link) => linked(pool, &link, text).await,
    }
}

fn consent_prompt(chat_id: i64, loc: Locale) -> Reply {
    Reply::with_keyboard(
        chat_id,
        render::consent_text(loc),
        vec![vec![button(loc.pick("I agree", "موافقم"), "consent:y"), button(loc.pick("No thanks", "نه، ممنون"), "consent:n")]],
    )
}

async fn unlinked(pool: &SqlitePool, chat_id: i64, user_id: i64, username: Option<&str>, loc: Locale, text: &str) -> R {
    match parse_command(text) {
        Some((cmd, arg)) if cmd == "start" && !arg.is_empty() => {
            if repo::failed_attempts_last_hour(pool, chat_id).await.map_err(err)? >= MAX_FAILED_CODES {
                return Ok(vec![Reply::text(chat_id, render::too_many_attempts_text(loc))]);
            }
            match repo::redeem_invite(pool, &arg).await.map_err(err)? {
                Err(_) => {
                    // The same message for every failure: do not reveal whether a code exists or was used.
                    repo::record_failed_attempt(pool, chat_id).await.map_err(err)?;
                    Ok(vec![Reply::text(chat_id, render::invalid_code_text(loc))])
                }
                Ok(profile_id) => match repo::create_link(pool, &profile_id, chat_id, user_id, username, loc).await {
                    Ok(()) => Ok(vec![consent_prompt(chat_id, loc)]),
                    Err(LinkError::ProfileHasChat) => Ok(vec![Reply::text(
                        chat_id,
                        loc.pick(
                            "This profile is already linked to a Telegram account. Please ask your matchmaker for help.",
                            "این پروفایل از قبل به یک حساب تلگرام متصل شده است. لطفاً از مشاور خود کمک بخواهید.",
                        ),
                    )]),
                    Err(LinkError::ChatHasProfile) => Ok(vec![Reply::text(chat_id, loc.pick("This chat is already linked to a profile.", "این گفتگو از قبل به یک پروفایل متصل شده است."))]),
                    Err(LinkError::Db(e)) => Err(e),
                },
            }
        }
        Some((cmd, _)) if cmd == "help" => Ok(vec![Reply::text(chat_id, render::help_text(loc))]),
        _ => Ok(vec![Reply::text(chat_id, render::unlinked_text(loc))]),
    }
}

async fn awaiting_consent(pool: &SqlitePool, link: &Link, text: &str) -> R {
    let chat_id = link.chat_id;
    if let Some((cmd, _)) = parse_command(text) {
        if cmd == "stop" || cmd == "unlink" {
            repo::delete_link(pool, &link.profile_id).await.map_err(err)?;
            return Ok(vec![Reply::text(chat_id, render::consent_declined_text(link.locale))]);
        }
        if cmd == "language" {
            return Ok(vec![Reply::with_keyboard(chat_id, render::language_prompt_text(), language_keyboard())]);
        }
    }
    Ok(vec![consent_prompt(chat_id, link.locale)])
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
    Ok(vec![Reply::text(chat_id, link.locale.pick("Thank you. I have passed your message to your matchmaker.", "ممنون. پیام شما را به مشاورتان رساندم."))])
}

async fn command(pool: &SqlitePool, link: &Link, cmd: &str, arg: &str) -> R {
    let chat_id = link.chat_id;
    let loc = link.locale;
    match cmd {
        "start" => {
            let p = load_profile(pool, &link.profile_id).await?;
            Ok(vec![Reply::text(chat_id, render::welcome_text(loc, &greeting_name(&p.profile, loc)))])
        }
        "help" => Ok(vec![Reply::text(chat_id, render::help_text(loc))]),
        "language" => Ok(vec![Reply::with_keyboard(chat_id, render::language_prompt_text(), language_keyboard())]),
        "cancel" => {
            repo::clear_state(pool, chat_id).await.map_err(err)?;
            Ok(vec![Reply::text(chat_id, loc.pick("Okay, cancelled.", "باشه، لغو شد."))])
        }
        "profile" => {
            let reg = MmRepository::registry(pool).await.map_err(err)?;
            let p = load_profile(pool, &link.profile_id).await?;
            Ok(vec![Reply::text(chat_id, render::profile_summary(loc, &p.profile, &reg))])
        }
        "edit" => start_edit(pool, link).await,
        "preferences" => show_preferences(pool, link).await,
        "matches" => show_matches(pool, link).await,
        "questionnaires" => show_questionnaires(pool, link).await,
        "match" => open_match_by_number(pool, link, arg).await,
        "status" => show_status(pool, link).await,
        "settings" => show_settings(link),
        "stop" => {
            repo::set_notifications(pool, &link.profile_id, false).await.map_err(err)?;
            Ok(vec![Reply::text(
                chat_id,
                loc.pick(
                    "I will not send you notifications any more. Send /resume to turn them back on. You can still use the commands.",
                    "دیگر برایتان اعلان نمی‌فرستم. برای روشن کردن دوباره، /resume را بفرستید. هنوز می‌توانید از دستورها استفاده کنید.",
                ),
            )])
        }
        "resume" => {
            repo::set_notifications(pool, &link.profile_id, true).await.map_err(err)?;
            Ok(vec![Reply::text(chat_id, loc.pick("Notifications are on again.", "اعلان‌ها دوباره روشن شد."))])
        }
        "unlink" => Ok(vec![Reply::with_keyboard(
            chat_id,
            loc.pick(
                "Unlink this chat from your profile? You will stop receiving messages here. Your profile stays with your matchmaker.",
                "این گفتگو از پروفایل شما جدا شود؟ دیگر اینجا پیامی دریافت نمی‌کنید. پروفایل شما نزد مشاورتان می‌ماند.",
            ),
            vec![vec![button(loc.pick("Yes, unlink", "بله، جدا شود"), "unlink:y"), button(loc.pick("Keep", "نگه دار"), "unlink:n")]],
        )]),
        "forget" => Ok(vec![Reply::with_keyboard(
            chat_id,
            loc.pick(
                "Ask your matchmaker to delete all your data? They will be notified of your request and can complete it in the app.",
                "از مشاورتان بخواهید همه داده‌های شما را حذف کند؟ درخواست شما به او اطلاع داده می‌شود و می‌تواند آن را در برنامه انجام دهد.",
            ),
            vec![vec![button(loc.pick("Yes, delete my data", "بله، داده‌هایم حذف شود"), "forget:y"), button(loc.pick("Keep", "نگه دار"), "forget:n")]],
        )]),
        _ => Ok(vec![Reply::text(chat_id, loc.pick("I did not understand that. Send /help to see what I can do.", "متوجه نشدم. برای دیدن کارهایی که می‌توانم انجام دهم، /help را بفرستید."))]),
    }
}

fn show_settings(link: &Link) -> R {
    let loc = link.locale;
    let on = link.notifications_enabled;
    let kb = vec![
        vec![if on {
            button(loc.pick("Turn notifications off", "خاموش کردن اعلان‌ها"), "set:n:off")
        } else {
            button(loc.pick("Turn notifications on", "روشن کردن اعلان‌ها"), "set:n:on")
        }],
        vec![button(loc.pick("Language", "زبان"), "set:lang")],
        vec![button(loc.pick("Unlink this chat", "جدا کردن این گفتگو"), "set:unlink"), button(loc.pick("Delete my data", "حذف داده‌های من"), "set:forget")],
    ];
    let state = if on { loc.pick("on", "روشن") } else { loc.pick("off", "خاموش") };
    let text = match loc {
        Locale::En => format!("<b>Settings</b>\nNotifications: {state}\n\nMessages here pass through Telegram. Your matchmaker can see everything you send."),
        Locale::Fa => format!("<b>تنظیمات</b>\nاعلان‌ها: {state}\n\nپیام‌های اینجا از طریق تلگرام می‌گذرند. مشاور شما همه آنچه را می‌فرستید می‌بیند."),
    };
    Ok(vec![Reply::with_keyboard(link.chat_id, text, kb)])
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

async fn other_name(pool: &SqlitePool, other_id: &str, loc: Locale) -> Result<String, String> {
    let p = load_profile(pool, other_id).await?;
    Ok(first_name_for(&p.profile, repo::show_first_name(pool).await.map_err(err)?, loc))
}

async fn show_matches(pool: &SqlitePool, link: &Link) -> R {
    let loc = link.locale;
    let ms = visible_matches(pool, &link.profile_id).await?;
    if ms.is_empty() {
        return Ok(vec![Reply::text(link.chat_id, loc.pick("You have no introductions at the moment.", "فعلاً معرفی‌ای ندارید."))]);
    }
    let mut text = String::from(loc.pick("<b>Your introductions</b>\n", "<b>معرفی‌های شما</b>\n"));
    let mut buttons = vec![];
    for (i, m) in ms.iter().enumerate() {
        let (_, mine, other) = my_side(m, &link.profile_id);
        let name = other_name(pool, &other, link.locale).await?;
        text.push_str(&format!("{}. {} - {}\n", num(loc, i + 1), render::user_text(loc, &name), render::friendly_status(loc, m.status, mine != Interest::Unknown, &name)));
        buttons.push(button(format!("{} {}", loc.pick("Open", "باز کردن"), num(loc, i + 1)), format!("match:open:{}", m.id)));
    }
    Ok(vec![Reply::with_keyboard(link.chat_id, text, rows(buttons, 3))])
}

async fn open_match_by_number(pool: &SqlitePool, link: &Link, arg: &str) -> R {
    let ms = visible_matches(pool, &link.profile_id).await?;
    match arg.trim().parse::<usize>().ok().and_then(|n| n.checked_sub(1)).and_then(|i| ms.get(i)) {
        Some(m) => open_match(pool, link, m).await,
        None => Ok(vec![Reply::text(
            link.chat_id,
            link.locale.pick(
                "Send /matches to see your introductions, then /match followed by a number.",
                "برای دیدن معرفی‌هایتان /matches را بفرستید، سپس /match و بعد از آن یک عدد.",
            ),
        )]),
    }
}

async fn open_match(pool: &SqlitePool, link: &Link, m: &MatchRow) -> R {
    let loc = link.locale;
    let (_, mine, other) = my_side(m, &link.profile_id);
    let other_p = load_profile(pool, &other).await?;
    let card = notify::card_for(pool, &other_p, loc).await?;
    let awaiting = m.status == MatchStatus::IntroductionProposed && mine == Interest::Unknown;
    let mut text = render::introduction_text(loc, &card, awaiting);
    if !awaiting {
        text.push_str(&format!("\n{}: {}", loc.pick("Status", "وضعیت"), render::friendly_status(loc, m.status, mine != Interest::Unknown, &card.title)));
    }
    Ok(vec![if awaiting { Reply::with_keyboard(link.chat_id, text, notify::intro_keyboard(loc, &m.id)) } else { Reply::text(link.chat_id, text) }])
}

async fn show_status(pool: &SqlitePool, link: &Link) -> R {
    let reg = MmRepository::registry(pool).await.map_err(err)?;
    let p = load_profile(pool, &link.profile_id).await?;
    let loc = link.locale;
    let mut text = String::from(loc.pick("<b>Status</b>\n", "<b>وضعیت</b>\n"));
    match p.profile.completeness(&reg) {
        Some(c) if c < 1.0 => {
            let percent = num(loc, (c * 100.0).round());
            text.push_str(&match loc {
                Locale::En => format!("Profile: {percent}% complete (send /edit to finish)\n"),
                Locale::Fa => format!("پروفایل: {percent}٪ کامل است (برای تکمیل /edit را بفرستید)\n"),
            });
        }
        _ => text.push_str(loc.pick("Profile: complete\n", "پروفایل: کامل است\n")),
    }
    let ms = visible_matches(pool, &link.profile_id).await?;
    if ms.is_empty() {
        text.push_str(loc.pick("Introductions: none at the moment\n", "معرفی‌ها: فعلاً ندارید\n"));
    }
    for (i, m) in ms.iter().enumerate() {
        let (_, mine, other) = my_side(m, &link.profile_id);
        let name = other_name(pool, &other, link.locale).await?;
        text.push_str(&format!("{}. {} - {}\n", num(loc, i + 1), render::user_text(loc, &name), render::friendly_status(loc, m.status, mine != Interest::Unknown, &name)));
    }
    let state = if link.notifications_enabled { loc.pick("on", "روشن") } else { loc.pick("off", "خاموش") };
    text.push_str(&format!("{}: {}", loc.pick("Notifications", "اعلان‌ها"), state));
    Ok(vec![Reply::text(link.chat_id, text)])
}

// ------------------------------------------------------------------ questionnaires

async fn show_questionnaires(pool: &SqlitePool, link: &Link) -> R {
    let loc = link.locale;
    let open = qrepo::open_responses(pool, &link.profile_id).await.map_err(err)?;
    if open.is_empty() {
        return Ok(vec![Reply::text(
            link.chat_id,
            loc.pick(
                "You have no questionnaires waiting. Your matchmaker will tell you if there is one.",
                "پرسشنامه‌ای در انتظار شما نیست. اگر پرسشنامه‌ای باشد مشاورتان خبر می‌دهد.",
            ),
        )]);
    }
    let mut text = String::from(loc.pick(
        "<b>Questionnaires</b>\nThere are no right or wrong answers. You can stop at any time and continue later with /questionnaires.\n",
        "<b>پرسشنامه‌ها</b>\nپاسخ درست یا غلط وجود ندارد. هر زمان بخواهید می‌توانید متوقف شوید و بعداً با /questionnaires ادامه دهید.\n",
    ));
    let mut buttons = vec![];
    for r in &open {
        let inst = qservice::instrument_for(pool, r).await?;
        let progress = match loc {
            Locale::En => format!("{} of {} answered", r.answers.len(), inst.items.len()),
            Locale::Fa => format!("{} از {} پاسخ داده شده", num(loc, r.answers.len()), num(loc, inst.items.len())),
        };
        text.push_str(&format!("• {} ({})\n", render::user_text(loc, &inst.name), progress));
        let verb = if r.answers.is_empty() { loc.pick("Start", "شروع") } else { loc.pick("Continue", "ادامه") };
        buttons.push(button(format!("{}: {}", verb, inst.name), format!("q:start:{}", r.id)));
    }
    Ok(vec![Reply::with_keyboard(link.chat_id, text, buttons.into_iter().map(|b| vec![b]).collect())])
}

/// The next unanswered question as a message with one button per scale point.
async fn ask_question(pool: &SqlitePool, chat_id: i64, loc: Locale, r: &qrepo::ResponseRow) -> R {
    let inst = qservice::instrument_for(pool, r).await?;
    let Some((idx, item)) = matchmaking_core::next_item(&inst, &r.answers) else {
        return Ok(vec![Reply::text(chat_id, loc.pick("You have answered every question. Thank you!", "به همه پرسش‌ها پاسخ دادید. ممنون!"))]);
    };
    let position = match loc {
        Locale::En => format!("Question {} of {}", idx + 1, inst.items.len()),
        Locale::Fa => format!("پرسش {} از {}", num(loc, idx + 1), num(loc, inst.items.len())),
    };
    let mut text = format!("<b>{}</b>\n{}\n\n{}\n\n", render::user_text(loc, &inst.name), position, render::user_text(loc, &item.text));
    for (n, label) in inst.scale.labels.iter().enumerate() {
        text.push_str(&format!("{} = {}\n", num(loc, inst.scale.min + n as i32), render::user_text(loc, label)));
    }
    let buttons: Vec<InlineButton> = (inst.scale.min..=inst.scale.max).map(|v| button(num(loc, v), format!("q:a:{}:{}:{}", r.id, idx, v))).collect();
    Ok(vec![Reply::with_keyboard(chat_id, text, vec![buttons])])
}

async fn questionnaire_callback(pool: &SqlitePool, link: &Link, cb_id: &str, rest: &str, message_id: Option<i64>) -> R {
    let chat_id = link.chat_id;
    let loc = link.locale;
    let invalid = loc.pick("This button is no longer valid.", "این دکمه دیگر معتبر نیست.");
    let mut parts = rest.split(':');
    let (kind, id) = (parts.next().unwrap_or(""), parts.next().and_then(|v| v.parse::<i64>().ok()));
    let Some(id) = id else { return Ok(vec![toast(cb_id, invalid)]) };
    // Never trust the button: the response must exist, belong to this person and still be open.
    let Some(r) = qrepo::get_response(pool, id).await.map_err(err)?.filter(|r| r.profile_id == link.profile_id && r.status != "complete") else {
        return Ok(vec![toast(cb_id, loc.pick("This questionnaire is no longer open.", "این پرسشنامه دیگر باز نیست."))]);
    };
    let clear = message_id.map(|mid| Reply::EditMarkup { chat_id, message_id: mid, keyboard: None });
    match kind {
        "start" => {
            let mut out = vec![done(cb_id)];
            out.extend(ask_question(pool, chat_id, loc, &r).await?);
            Ok(out)
        }
        "a" => {
            let (Some(idx), Some(val)) = (parts.next().and_then(|v| v.parse::<usize>().ok()), parts.next().and_then(|v| v.parse::<i32>().ok())) else {
                return Ok(vec![toast(cb_id, invalid)]);
            };
            let inst = qservice::instrument_for(pool, &r).await?;
            // The tapped question must be the one currently waiting: a second tap or an old message changes nothing.
            let current = matchmaking_core::next_item(&inst, &r.answers);
            let Some((cur_idx, item)) = current.filter(|(i, _)| *i == idx) else {
                let mut out = vec![toast(cb_id, loc.pick("That question has already been answered.", "به این پرسش قبلاً پاسخ داده شده است."))];
                out.extend(clear);
                return Ok(out);
            };
            let item_id = item.id.clone();
            let _ = cur_idx;
            let outcome = match qservice::record_answer_for(pool, id, &item_id, val, loc).await {
                Ok(o) => o,
                Err(e) => return Ok(vec![toast(cb_id, &e)]),
            };
            let mut out = vec![done(cb_id)];
            out.extend(clear);
            if outcome.completed {
                // Scores are not shown here: they are for the matchmaker, who can put them in context.
                out.push(Reply::text(
                    chat_id,
                    loc.pick(
                        "Thank you, you have finished this questionnaire. Your matchmaker will go through the results with you.",
                        "ممنون، این پرسشنامه را تمام کردید. مشاورتان نتایج را همراه شما مرور می‌کند.",
                    ),
                ));
            } else {
                let fresh = qrepo::get_response(pool, id).await.map_err(err)?.ok_or("Response not found")?;
                out.extend(ask_question(pool, chat_id, loc, &fresh).await?);
            }
            Ok(out)
        }
        _ => Ok(vec![toast(cb_id, invalid)]),
    }
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
        let mut out = vec![Reply::text(
            link.chat_id,
            link.locale.pick(
                "Let's complete your profile. You can skip any question, or send /cancel to stop.",
                "بیایید پروفایل شما را کامل کنیم. می‌توانید هر پرسشی را رد کنید یا برای توقف /cancel را بفرستید.",
            ),
        )];
        out.push(ask_field(link.chat_id, link.locale, reg.get(&first).ok_or("field missing")?, &[]));
        return Ok(out);
    }
    // everything required is filled: offer to change anything
    let buttons: Vec<InlineButton> = reg.defs().filter(|d| editable(d)).take(30).map(|d| button(d.label_for(link.locale).to_string(), format!("edit:f:{}", d.key))).collect();
    Ok(vec![Reply::with_keyboard(
        link.chat_id,
        link.locale.pick("Your profile is complete. What would you like to change?", "پروفایل شما کامل است. چه چیزی را می‌خواهید تغییر دهید؟"),
        rows(buttons, 2),
    )])
}

fn ask_field(chat_id: i64, loc: Locale, def: &FieldDef, selected: &[String]) -> Reply {
    let mut text = format!("<b>{}</b>\n", render::esc(def.label_for(loc)));
    if def.sensitive {
        text.push_str(loc.pick(
            "This is sensitive information and Telegram will carry it. Skip it if you prefer to tell your matchmaker in person.\n",
            "این اطلاعات حساس است و از طریق تلگرام منتقل می‌شود. اگر ترجیح می‌دهید حضوری به مشاورتان بگویید، آن را رد کنید.\n",
        ));
    }
    let skip = button(loc.pick("Skip", "رد کردن"), "edit:skip");
    match &def.kind {
        FieldKind::Choice(opts) => {
            text.push_str(loc.pick("Choose one:", "یکی را انتخاب کنید:"));
            let mut kb = rows(opts.iter().enumerate().map(|(i, o)| button(pretty(o), format!("edit:c:{i}"))).collect(), 2);
            kb.push(vec![skip]);
            Reply::with_keyboard(chat_id, text, kb)
        }
        FieldKind::MultiChoice(opts) => {
            text.push_str(loc.pick("Choose all that apply, then tap Done:", "همه موارد مناسب را انتخاب کنید، سپس «انجام شد» را بزنید:"));
            let mut kb = rows(
                opts.iter()
                    .enumerate()
                    .map(|(i, o)| button(format!("{}{}", if selected.contains(o) { "✓ " } else { "" }, pretty(o)), format!("edit:m:{i}")))
                    .collect(),
                2,
            );
            kb.push(vec![button(loc.pick("Done", "انجام شد"), "edit:done"), skip]);
            Reply::with_keyboard(chat_id, text, kb)
        }
        FieldKind::Bool => {
            text.push_str(loc.pick("Yes or no?", "بله یا خیر؟"));
            Reply::with_keyboard(chat_id, text, vec![vec![button(loc.pick("Yes", "بله"), "edit:b:1"), button(loc.pick("No", "خیر"), "edit:b:0")], vec![skip]])
        }
        FieldKind::Number => {
            text.push_str(loc.pick("Send a number.", "یک عدد بفرستید."));
            Reply::with_keyboard(chat_id, text, vec![vec![skip]])
        }
        _ => {
            text.push_str(loc.pick("Type your answer.", "پاسخ خود را بنویسید."));
            Reply::with_keyboard(chat_id, text, vec![vec![skip]])
        }
    }
}

fn parse_typed(def: &FieldDef, text: &str) -> Option<Value> {
    let t = text.trim();
    match &def.kind {
        FieldKind::Number => parse_number(t).map(Value::Num),
        FieldKind::Text => (t.chars().count() <= 500).then(|| Value::Text(t.to_string())),
        FieldKind::Bool => parse_bool_word(t).map(Value::Bool),
        FieldKind::Choice(opts) => opts.iter().find(|o| pretty(o).eq_ignore_ascii_case(t) || o.eq_ignore_ascii_case(t)).map(|o| Value::Text(o.clone())),
        FieldKind::MultiChoice(opts) => {
            let picked: Option<Vec<String>> = t
                .split([',', '،'])
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
    if let Err(e) = reg.validate_value_for(key, &value, link.locale) {
        return Ok(Err(e));
    }
    let mut p = load_profile(pool, &link.profile_id).await?;
    if !p.profile.set(key, value, Provenance::User) {
        return Ok(Ok(link
            .locale
            .pick(
                "Your matchmaker has already filled this in, so only they can change it. I kept it as it is.",
                "مشاورتان قبلاً این را پر کرده است، پس فقط او می‌تواند آن را تغییر دهد. آن را همان‌طور نگه داشتم.",
            )
            .to_string()));
    }
    MmRepository::update_profile_data(pool, &p.profile).await.map_err(err)?;
    MmRepository::audit(pool, "update", "profile", &link.profile_id, Some(key)).await;
    Ok(Ok(link.locale.pick("Saved.", "ذخیره شد.").to_string()))
}

/// Move to the next question, or finish.
async fn advance_edit(pool: &SqlitePool, link: &Link, reg: &FieldRegistry, st: &Json, mut out: Vec<Reply>) -> R {
    let mut queue = state_list(st, "queue");
    if !queue.is_empty() {
        queue.remove(0);
    }
    if let Some(next) = queue.first().cloned() {
        repo::set_state(pool, link.chat_id, &json!({"flow": "edit", "queue": queue, "current": next, "multi": []})).await.map_err(err)?;
        out.push(ask_field(link.chat_id, link.locale, reg.get(&next).ok_or("field missing")?, &[]));
        return Ok(out);
    }
    repo::clear_state(pool, link.chat_id).await.map_err(err)?;
    let p = load_profile(pool, &link.profile_id).await?;
    let loc = link.locale;
    out.push(Reply::text(
        link.chat_id,
        match p.profile.completeness(reg) {
            Some(c) if c < 1.0 => {
                let percent = num(loc, (c * 100.0).round());
                match loc {
                    Locale::En => format!("Thank you. Your profile is {percent}% complete; send /edit when you are ready to add the rest."),
                    Locale::Fa => format!("ممنون. پروفایل شما {percent}٪ کامل است؛ هر وقت آماده بودید برای افزودن بقیه /edit را بفرستید."),
                }
            }
            _ => loc.pick("Thank you. Your profile is complete.", "ممنون. پروفایل شما کامل است.").to_string(),
        },
    ));
    Ok(out)
}

async fn flow_text(pool: &SqlitePool, link: &Link, st: Json, text: &str) -> R {
    let chat_id = link.chat_id;
    let loc = link.locale;
    match state_str(&st, "flow").as_deref() {
        Some("edit") => {
            let reg = MmRepository::registry(pool).await.map_err(err)?;
            let Some(key) = state_str(&st, "current") else { return Ok(vec![]) };
            let def = reg.get(&key).ok_or("field missing")?.clone();
            let Some(value) = parse_typed(&def, text) else {
                return Ok(vec![Reply::text(chat_id, loc.pick("I could not read that.", "نتوانستم این را بخوانم.")), ask_field(chat_id, loc, &def, &state_list(&st, "multi"))]);
            };
            match save_answer(pool, link, &reg, &key, value).await? {
                Err(msg) => Ok(vec![Reply::text(chat_id, format!("{} {}", loc.pick("That does not look right:", "این درست به نظر نمی‌رسد:"), render::esc(&msg))), ask_field(chat_id, loc, &def, &[])]),
                Ok(msg) => advance_edit(pool, link, &reg, &st, vec![Reply::text(chat_id, msg)]).await,
            }
        }
        Some("pref") => pref_text(pool, link, st, text).await,
        Some("reply") => {
            let match_id = state_str(&st, "match_id");
            repo::add_message(pool, &link.profile_id, "in", text, match_id.as_deref()).await.map_err(err)?;
            repo::clear_state(pool, chat_id).await.map_err(err)?;
            Ok(vec![Reply::text(chat_id, loc.pick("Thank you, I have passed that to your matchmaker.", "ممنون، این را به مشاورتان رساندم."))])
        }
        _ => {
            repo::clear_state(pool, chat_id).await.map_err(err)?;
            repo::add_message(pool, &link.profile_id, "in", text, None).await.map_err(err)?;
            Ok(vec![Reply::text(chat_id, loc.pick("Thank you. I have passed your message to your matchmaker.", "ممنون. پیام شما را به مشاورتان رساندم."))])
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
    let loc = link.locale;
    let mut text = format!("<b>{}</b>\n", loc.pick("What you are looking for", "آنچه به دنبالش هستید"));
    if prefs.is_empty() {
        text.push_str(loc.pick("Nothing set yet.\n", "هنوز چیزی تنظیم نشده است.\n"));
    }
    let mut kb: Keyboard = vec![];
    for (i, p) in prefs.iter().enumerate() {
        text.push_str(&format!("{}. {}\n", num(loc, i + 1), render::user_text(loc, &describe_preference_for(p, &reg, loc))));
        kb.push(vec![button(format!("{} {}", loc.pick("Remove", "حذف"), num(loc, i + 1)), format!("pref:del:{}", p.id))]);
    }
    kb.push(vec![button(loc.pick("Add something", "افزودن مورد"), "pref:add")]);
    Ok(vec![Reply::with_keyboard(link.chat_id, text, kb)])
}

fn strength_keyboard(loc: Locale) -> Keyboard {
    vec![
        vec![button(loc.pick("Must have", "حتماً باشد"), "pref:s:required"), button(loc.pick("Deal-breaker", "غیرقابل‌قبول"), "pref:s:deal_breaker")],
        vec![button(loc.pick("Preferred", "ترجیحاً"), "pref:s:preferred"), button(loc.pick("Nice to have", "خوب است باشد"), "pref:s:flexible")],
    ]
}

async fn finish_preference(pool: &SqlitePool, link: &Link, st: &Json, strength: Strength) -> R {
    let chat_id = link.chat_id;
    let loc = link.locale;
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
                    return Ok(vec![Reply::text(chat_id, loc.pick("Nothing was set, so I did not add anything.", "چیزی تنظیم نشد، پس چیزی اضافه نکردم."))]);
                }
            };
            Condition { field: field.clone(), op, value, value2, value_rel: None, value2_rel: None }
        }
    };
    let pref = Preference { id: uuid::Uuid::new_v4().to_string(), condition: cond, strength, importance: 3, note: None };
    if let Err(e) = validate_preference_for(&pref, &reg, loc) {
        repo::clear_state(pool, chat_id).await.map_err(err)?;
        return Ok(vec![Reply::text(chat_id, format!("{} {}", loc.pick("I could not add that:", "نتوانستم این را اضافه کنم:"), render::esc(&e)))]);
    }
    let mut prefs = MmRepository::get_preferences(pool, &link.profile_id).await.map_err(err)?;
    if prefs.len() >= MAX_PREFERENCES {
        repo::clear_state(pool, chat_id).await.map_err(err)?;
        return Ok(vec![Reply::text(chat_id, loc.pick("You already have the maximum number of preferences.", "شما به حداکثر تعداد ترجیح‌ها رسیده‌اید."))]);
    }
    let described = describe_preference_for(&pref, &reg, loc);
    prefs.push(pref);
    MmRepository::save_preferences(pool, &link.profile_id, &prefs).await.map_err(err)?;
    MmRepository::audit(pool, "update_preferences", "profile", &link.profile_id, Some(&format!("{} items", prefs.len()))).await;
    repo::clear_state(pool, chat_id).await.map_err(err)?;
    Ok(vec![Reply::text(chat_id, format!("{} {}\n{}", loc.pick("Added:", "اضافه شد:"), render::user_text(loc, &described), loc.pick("Send /preferences to review.", "برای مرور /preferences را بفرستید.")))])
}

async fn pref_text(pool: &SqlitePool, link: &Link, mut st: Json, text: &str) -> R {
    let chat_id = link.chat_id;
    let loc = link.locale;
    let step = state_str(&st, "step").unwrap_or_default();
    if step != "lo" && step != "hi" {
        return Ok(vec![Reply::text(chat_id, loc.pick("Please use the buttons, or send /cancel.", "لطفاً از دکمه‌ها استفاده کنید یا /cancel را بفرستید."))]);
    }
    let t = text.trim();
    let value = if is_skip_word(t) { Some(Json::Null) } else { parse_number(t).filter(|n| n.is_finite()).map(|n| json!(n)) };
    let Some(value) = value else {
        return Ok(vec![Reply::text(chat_id, loc.pick("Please send a number, or the word skip.", "لطفاً یک عدد بفرستید یا بنویسید «رد»."))]);
    };
    st[&step] = value;
    if step == "lo" {
        st["step"] = json!("hi");
        repo::set_state(pool, chat_id, &st).await.map_err(err)?;
        return Ok(vec![Reply::text(chat_id, loc.pick("And the highest? Send a number, or the word skip.", "و بیشترین مقدار؟ یک عدد بفرستید یا بنویسید «رد»."))]);
    }
    st["step"] = json!("strength");
    repo::set_state(pool, chat_id, &st).await.map_err(err)?;
    Ok(vec![Reply::with_keyboard(chat_id, loc.pick("How important is this?", "این چقدر مهم است؟"), strength_keyboard(loc))])
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
        let guessed = Locale::parse_or_default(cb.from.language_code.as_deref().unwrap_or(""));
        return Ok(vec![toast(&cb.id, guessed.pick("This chat is not linked to a profile.", "این گفتگو به هیچ پروفایلی وصل نیست."))]);
    };
    let loc = link.locale;
    let invalid = loc.pick("This button is no longer valid.", "این دکمه دیگر معتبر نیست.");
    let unavailable = loc.pick("This introduction is no longer available.", "این معرفی دیگر در دسترس نیست.");
    let data = cb.data.clone().unwrap_or_default();
    let (verb, rest) = data.split_once(':').unwrap_or((data.as_str(), ""));
    let clear = |kb: Option<Keyboard>| message_id.map(|mid| Reply::EditMarkup { chat_id, message_id: mid, keyboard: kb });
    let mut out: Vec<Reply> = vec![];

    if verb == "lang" {
        let Some(chosen) = Locale::parse(rest) else { return Ok(vec![toast(&cb.id, invalid)]) };
        repo::set_language(pool, &link.profile_id, chosen).await.map_err(err)?;
        out.push(done(&cb.id));
        out.extend(clear(None));
        out.push(Reply::text(chat_id, render::language_changed_text(chosen)));
        if !link.consented {
            out.push(consent_prompt(chat_id, chosen));
        }
        return Ok(out);
    }

    if verb == "consent" {
        if link.consented {
            return Ok(vec![toast(&cb.id, loc.pick("Already answered.", "قبلاً پاسخ داده شده است."))]);
        }
        out.push(done(&cb.id));
        out.extend(clear(None));
        if rest == "y" {
            repo::set_consent(pool, &link.profile_id).await.map_err(err)?;
            MmRepository::audit(pool, "telegram_consent", "profile", &link.profile_id, Some(&format!("v{}", repo::CONSENT_VERSION))).await;
            let p = load_profile(pool, &link.profile_id).await?;
            out.push(Reply::text(chat_id, render::welcome_text(loc, &greeting_name(&p.profile, loc))));
        } else {
            repo::delete_link(pool, &link.profile_id).await.map_err(err)?;
            out.push(Reply::text(chat_id, render::consent_declined_text(loc)));
        }
        return Ok(out);
    }
    if !link.consented {
        return Ok(vec![toast(&cb.id, loc.pick("Please answer the privacy notice first.", "لطفاً ابتدا به اعلان حریم خصوصی پاسخ دهید."))]);
    }

    match verb {
        "intro" => {
            let Some((answer, match_id)) = rest.split_once(':') else { return Ok(vec![toast(&cb.id, invalid)]) };
            let Some(m) = MatchRepo::get(pool, match_id).await.map_err(err)? else { return Ok(vec![toast(&cb.id, unavailable)]) };
            if m.profile_a != link.profile_id && m.profile_b != link.profile_id {
                // Someone pressed a button for a match that is not theirs: refuse without saying anything about it.
                return Ok(vec![toast(&cb.id, invalid)]);
            }
            let (side, mine, other) = my_side(&m, &link.profile_id);
            if !matchmaking_core::can_record_responses(m.status) {
                out.push(toast(&cb.id, loc.pick("This introduction is no longer open.", "این معرفی دیگر باز نیست.")));
                out.extend(clear(None));
                return Ok(out);
            }
            let response = match answer {
                "y" => Interest::Interested,
                "n" => Interest::NotInterested,
                _ => return Ok(vec![toast(&cb.id, invalid)]),
            };
            if mine == response {
                return Ok(vec![toast(&cb.id, loc.pick("Already recorded.", "قبلاً ثبت شده است."))]);
            }
            let (_, new_status) = set_response_inner(pool, &m.id, side, response, loc).await?;
            out.push(done(&cb.id));
            out.extend(clear(None));
            match (response, new_status) {
                // both-interested and declined notices come from the notification layer, to avoid saying things twice
                (Interest::Interested, MatchStatus::BothInterested) => {}
                (Interest::NotInterested, _) => out.push(Reply::text(chat_id, loc.pick("Understood, thank you. I have noted that you are not interested. Nothing more will be shared.", "متوجه شدم، ممنون. ثبت کردم که علاقه‌ای ندارید. چیز دیگری به اشتراک گذاشته نمی‌شود."))),
                (Interest::Interested, _) => {
                    let name = other_name(pool, &other, link.locale).await?;
                    out.push(Reply::text(chat_id, match loc {
                        Locale::En => format!("Thank you! I will let you know when {} has answered.", render::esc(&name)),
                        Locale::Fa => format!("ممنون! وقتی {} پاسخ داد به شما خبر می‌دهم.", render::user_text(loc, &name)),
                    }));
                }
                _ => {}
            }
            Ok(out)
        }
        "match" => {
            let Some(match_id) = rest.strip_prefix("open:") else { return Ok(vec![toast(&cb.id, invalid)]) };
            let ms = visible_matches(pool, &link.profile_id).await?;
            match ms.iter().find(|m| m.id == match_id) {
                Some(m) => {
                    out.push(done(&cb.id));
                    out.extend(open_match(pool, &link, m).await?);
                    Ok(out)
                }
                None => Ok(vec![toast(&cb.id, unavailable)]),
            }
        }
        "set" => match rest {
            "n:on" | "n:off" => {
                repo::set_notifications(pool, &link.profile_id, rest == "n:on").await.map_err(err)?;
                out.push(toast(&cb.id, if rest == "n:on" { loc.pick("Notifications on", "اعلان‌ها روشن شد") } else { loc.pick("Notifications off", "اعلان‌ها خاموش شد") }));
                let refreshed = Link { notifications_enabled: rest == "n:on", ..link.clone() };
                out.extend(clear(None));
                out.extend(show_settings(&refreshed)?);
                Ok(out)
            }
            "lang" => Ok(vec![done(&cb.id), Reply::with_keyboard(chat_id, render::language_prompt_text(), language_keyboard())]),
            "unlink" => Ok(vec![done(&cb.id), Reply::with_keyboard(chat_id, loc.pick("Unlink this chat from your profile?", "این گفتگو از پروفایل شما جدا شود؟"), vec![vec![button(loc.pick("Yes, unlink", "بله، جدا شود"), "unlink:y"), button(loc.pick("Keep", "نگه داشتن"), "unlink:n")]])]),
            "forget" => Ok(vec![done(&cb.id), Reply::with_keyboard(chat_id, loc.pick("Ask your matchmaker to delete all your data?", "از مچ‌میکر بخواهید همه داده‌های شما را حذف کند؟"), vec![vec![button(loc.pick("Yes, delete my data", "بله، داده‌هایم حذف شود"), "forget:y"), button(loc.pick("Keep", "نگه داشتن"), "forget:n")]])]),
            _ => Ok(vec![toast(&cb.id, invalid)]),
        },
        "unlink" => {
            out.push(done(&cb.id));
            out.extend(clear(None));
            if rest == "y" {
                repo::delete_link(pool, &link.profile_id).await.map_err(err)?;
                MmRepository::audit(pool, "telegram_unlink", "profile", &link.profile_id, None).await;
                out.push(Reply::text(chat_id, loc.pick("Done. This chat is no longer linked. Your matchmaker can send a new invitation if you want to reconnect.", "انجام شد. این گفتگو دیگر وصل نیست. اگر خواستید دوباره وصل شوید، مچ‌میکر می‌تواند دعوت‌نامه جدیدی بفرستد.")));
            } else {
                out.push(Reply::text(chat_id, loc.pick("Okay, nothing changed.", "باشه، چیزی تغییر نکرد.")));
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
                out.push(Reply::text(chat_id, loc.pick("I have asked your matchmaker to delete your data. They will confirm once it is done.", "از مچ‌میکر خواستم داده‌های شما را حذف کند. پس از انجام، تأیید خواهد کرد.")));
            } else {
                out.push(Reply::text(chat_id, loc.pick("Okay, nothing changed.", "باشه، چیزی تغییر نکرد.")));
            }
            Ok(out)
        }
        "q" => questionnaire_callback(pool, &link, &cb.id, rest, message_id).await,
        "edit" => Ok(with_message_id(edit_callback(pool, &link, &cb.id, rest, out).await?, chat_id, message_id)),
        "pref" => Ok(with_message_id(pref_callback(pool, &link, &cb.id, rest, out).await?, chat_id, message_id)),
        _ => Ok(vec![toast(&cb.id, invalid)]),
    }
}

async fn edit_callback(pool: &SqlitePool, link: &Link, cb_id: &str, rest: &str, mut out: Vec<Reply>) -> R {
    let chat_id = link.chat_id;
    let loc = link.locale;
    let invalid = loc.pick("This button is no longer valid.", "این دکمه دیگر معتبر نیست.");
    let closed = loc.pick("This question is no longer open.", "این پرسش دیگر باز نیست.");
    let reg = MmRepository::registry(pool).await.map_err(err)?;
    out.push(done(cb_id));

    // "edit:f:<key>" starts a single-field edit from the menu
    if let Some(key) = rest.strip_prefix("f:") {
        let Some(def) = reg.get(key).filter(|d| editable(d)) else { return Ok(vec![toast(cb_id, invalid)]) };
        repo::set_state(pool, chat_id, &json!({"flow": "edit", "queue": [key], "current": key, "multi": []})).await.map_err(err)?;
        out.push(ask_field(chat_id, loc, def, &[]));
        return Ok(out);
    }
    let Some(st) = repo::get_state(pool, chat_id).await.map_err(err)?.filter(|s| state_str(s, "flow").as_deref() == Some("edit")) else {
        return Ok(vec![toast(cb_id, loc.pick("This question is no longer open. Send /edit to start again.", "این پرسش دیگر باز نیست. برای شروع دوباره /edit را بفرستید."))]);
    };
    let Some(key) = state_str(&st, "current") else { return Ok(vec![toast(cb_id, closed)]) };
    let def = reg.get(&key).ok_or("field missing")?.clone();

    if rest == "skip" {
        return advance_edit(pool, link, &reg, &st, out).await;
    }
    let value: Option<Value> = match (rest.split_once(':'), &def.kind) {
        (Some(("c", i)), FieldKind::Choice(opts)) => i.parse::<usize>().ok().and_then(|i| opts.get(i)).map(|o| Value::Text(o.clone())),
        (Some(("b", v)), FieldKind::Bool) => Some(Value::Bool(v == "1")),
        (Some(("m", i)), FieldKind::MultiChoice(opts)) => {
            // toggle one option and redraw the buttons
            let Some(opt) = i.parse::<usize>().ok().and_then(|i| opts.get(i)).cloned() else { return Ok(vec![toast(cb_id, invalid)]) };
            let mut sel = state_list(&st, "multi");
            if let Some(pos) = sel.iter().position(|s| *s == opt) { sel.remove(pos); } else { sel.push(opt); }
            let mut st2 = st.clone();
            st2["multi"] = json!(sel);
            repo::set_state(pool, chat_id, &st2).await.map_err(err)?;
            out.pop();
            out.push(done(cb_id));
            if let Reply::Send { keyboard, .. } = ask_field(chat_id, loc, &def, &sel) {
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
    let Some(value) = value else { return Ok(vec![toast(cb_id, loc.pick("Please choose at least one option, or Skip.", "لطفاً دست‌کم یک گزینه انتخاب کنید یا رد کنید."))]) };
    match save_answer(pool, link, &reg, &key, value).await? {
        Err(msg) => {
            out.push(Reply::text(chat_id, format!("{} {}", loc.pick("That does not look right:", "این درست به نظر نمی‌رسد:"), render::esc(&msg))));
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
    let loc = link.locale;
    let invalid = loc.pick("This button is no longer valid.", "این دکمه دیگر معتبر نیست.");
    let closed = loc.pick("This question is no longer open.", "این پرسش دیگر باز نیست.");
    let done_label = loc.pick("Done", "تمام");
    let reg = MmRepository::registry(pool).await.map_err(err)?;
    out.push(done(cb_id));

    if rest == "add" {
        let buttons: Vec<InlineButton> = pref_fields(&reg).into_iter().map(|d| button(d.label_for(loc).to_string(), format!("pref:f:{}", d.key))).collect();
        repo::set_state(pool, chat_id, &json!({"flow": "pref", "step": "field"})).await.map_err(err)?;
        out.push(Reply::with_keyboard(chat_id, loc.pick("What is it about?", "درباره چیست؟"), rows(buttons, 2)));
        return Ok(out);
    }
    if let Some(id) = rest.strip_prefix("del:") {
        let mut prefs = MmRepository::get_preferences(pool, &link.profile_id).await.map_err(err)?;
        let before = prefs.len();
        prefs.retain(|p| p.id != id);
        if prefs.len() == before {
            return Ok(vec![toast(cb_id, loc.pick("That was already removed.", "این قبلاً حذف شده است."))]);
        }
        MmRepository::save_preferences(pool, &link.profile_id, &prefs).await.map_err(err)?;
        MmRepository::audit(pool, "update_preferences", "profile", &link.profile_id, Some(&format!("{} items", prefs.len()))).await;
        out.extend(show_preferences(pool, link).await?);
        return Ok(out);
    }
    let Some(mut st) = repo::get_state(pool, chat_id).await.map_err(err)?.filter(|s| state_str(s, "flow").as_deref() == Some("pref")) else {
        return Ok(vec![toast(cb_id, loc.pick("This question is no longer open. Send /preferences to start again.", "این پرسش دیگر باز نیست. برای شروع دوباره /preferences را بفرستید."))]);
    };

    if let Some(key) = rest.strip_prefix("f:") {
        let Some(def) = pref_fields(&reg).into_iter().find(|d| d.key == key).cloned() else { return Ok(vec![toast(cb_id, invalid)]) };
        st = json!({"flow": "pref", "step": "value", "field": key, "values": []});
        match &def.kind {
            FieldKind::Choice(opts) | FieldKind::MultiChoice(opts) => {
                repo::set_state(pool, chat_id, &st).await.map_err(err)?;
                let mut kb = rows(opts.iter().enumerate().map(|(i, o)| button(pretty(o), format!("pref:m:{i}"))).collect(), 2);
                kb.push(vec![button(done_label, "pref:done")]);
                out.push(Reply::with_keyboard(chat_id, format!("<b>{}</b>\n{}", render::esc(def.label_for(loc)), loc.pick("Tap every option you mean, then Done:", "هر گزینه‌ای که منظورتان است را بزنید، سپس «تمام»:")), kb));
            }
            FieldKind::Bool => {
                repo::set_state(pool, chat_id, &st).await.map_err(err)?;
                out.push(Reply::with_keyboard(chat_id, format!("<b>{}</b>", render::esc(def.label_for(loc))), vec![vec![button(loc.pick("Yes", "بله"), "pref:b:1"), button(loc.pick("No", "خیر"), "pref:b:0")]]));
            }
            _ => {
                st["step"] = json!("lo");
                repo::set_state(pool, chat_id, &st).await.map_err(err)?;
                out.push(Reply::text(chat_id, format!("<b>{}</b>\n{}", render::esc(def.label_for(loc)), loc.pick("What is the lowest you would accept? Send a number, or the word skip.", "کمترین مقداری که می‌پذیرید چیست؟ یک عدد بفرستید یا بنویسید «رد».")) ));
            }
        }
        return Ok(out);
    }
    let field = state_str(&st, "field").unwrap_or_default();
    let Some(def) = reg.get(&field).cloned() else { return Ok(vec![toast(cb_id, closed)]) };

    if let Some(i) = rest.strip_prefix("m:") {
        let (FieldKind::Choice(opts) | FieldKind::MultiChoice(opts)) = &def.kind else { return Ok(vec![toast(cb_id, invalid)] ) };
        let Some(opt) = i.parse::<usize>().ok().and_then(|i| opts.get(i)).cloned() else { return Ok(vec![toast(cb_id, invalid)]) };
        let mut vals = state_list(&st, "values");
        if let Some(pos) = vals.iter().position(|s| *s == opt) { vals.remove(pos); } else { vals.push(opt); }
        st["values"] = json!(vals);
        repo::set_state(pool, chat_id, &st).await.map_err(err)?;
        let mut kb = rows(opts.iter().enumerate().map(|(i, o)| button(format!("{}{}", if vals.contains(o) { "✓ " } else { "" }, pretty(o)), format!("pref:m:{i}"))).collect(), 2);
        kb.push(vec![button(done_label, "pref:done")]);
        out.push(Reply::EditMarkup { chat_id, message_id: 0, keyboard: Some(kb) }); // caller fills in the message id
        return Ok(out);
    }
    if rest == "done" {
        if state_list(&st, "values").is_empty() {
            return Ok(vec![toast(cb_id, loc.pick("Choose at least one option first.", "ابتدا دست‌کم یک گزینه انتخاب کنید."))]);
        }
        st["step"] = json!("strength");
        repo::set_state(pool, chat_id, &st).await.map_err(err)?;
        out.push(Reply::with_keyboard(chat_id, loc.pick("How important is this?", "این چقدر مهم است؟"), strength_keyboard(loc)));
        return Ok(out);
    }
    if let Some(v) = rest.strip_prefix("b:") {
        st["bool"] = json!(v == "1");
        st["step"] = json!("strength");
        repo::set_state(pool, chat_id, &st).await.map_err(err)?;
        out.push(Reply::with_keyboard(chat_id, loc.pick("How important is this?", "این چقدر مهم است؟"), strength_keyboard(loc)));
        return Ok(out);
    }
    if let Some(s) = rest.strip_prefix("s:") {
        if state_str(&st, "step").as_deref() != Some("strength") {
            return Ok(vec![toast(cb_id, loc.pick("Please answer the earlier question first.", "لطفاً ابتدا به پرسش قبلی پاسخ دهید."))]);
        }
        let strength = match s {
            "required" => Strength::Required,
            "deal_breaker" => Strength::DealBreaker,
            "preferred" => Strength::Preferred,
            "flexible" => Strength::Flexible,
            _ => return Ok(vec![toast(cb_id, invalid)]),
        };
        out.extend(finish_preference(pool, link, &st, strength).await?);
        return Ok(out);
    }
    Ok(vec![toast(cb_id, invalid)])
}
