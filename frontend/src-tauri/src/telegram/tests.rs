//! Conversation-level tests: real updates go through the engine against an in-memory database, with a test
//! double in place of Telegram. Nothing here needs a network.

use super::api::{BotInfo, HttpTelegramApi, TelegramApi, TgError};
use super::engine::handle_update;
use super::notify;
use super::repo;
use super::types::{Keyboard, Reply, Update};
use super::worker::{flush_outbox, MAX_ATTEMPTS};
use crate::mm::match_repo::MatchRepo;
use crate::mm::repository::MmRepository;
use async_trait::async_trait;
use matchmaking_core::{ConditionOp, Interest, MatchStatus, Profile, Provenance, Value};
use serde_json::json;
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::SqlitePool;
use std::sync::Mutex;

async fn test_pool() -> SqlitePool {
    let pool = SqlitePoolOptions::new().max_connections(1).connect("sqlite::memory:").await.unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    pool
}

fn txt(s: &str) -> Value {
    Value::Text(s.into())
}

async fn make_profile(pool: &SqlitePool, id: &str, fields: &[(&str, Value)]) {
    let mut p = Profile::new(id);
    for (k, v) in fields {
        p.set(k, v.clone(), Provenance::Matchmaker);
    }
    MmRepository::insert_profile(pool, &p).await.unwrap();
}

fn msg(chat: i64, text: &str) -> Update {
    serde_json::from_value(json!({"update_id": 1, "message": {"message_id": 10, "from": {"id": chat, "is_bot": false, "username": "u"}, "chat": {"id": chat, "type": "private"}, "text": text}})).unwrap()
}

fn tap(chat: i64, data: &str) -> Update {
    serde_json::from_value(json!({"update_id": 2, "callback_query": {"id": "cb1", "from": {"id": chat, "is_bot": false}, "data": data,
        "message": {"message_id": 77, "chat": {"id": chat, "type": "private"}, "text": "x"}}})).unwrap()
}

async fn say(pool: &SqlitePool, chat: i64, text: &str) -> Vec<Reply> {
    handle_update(pool, msg(chat, text)).await.unwrap()
}

async fn press(pool: &SqlitePool, chat: i64, data: &str) -> Vec<Reply> {
    handle_update(pool, tap(chat, data)).await.unwrap()
}

/// All visible text of a set of replies (messages and toasts).
fn texts(replies: &[Reply]) -> String {
    replies
        .iter()
        .filter_map(|r| match r {
            Reply::Send { text, .. } => Some(text.clone()),
            Reply::AnswerCallback { text: Some(t), .. } => Some(t.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn first_keyboard(replies: &[Reply]) -> Option<Keyboard> {
    replies.iter().find_map(|r| match r {
        Reply::Send { keyboard: Some(k), .. } => Some(k.clone()),
        _ => None,
    })
}

/// Link a profile to a chat through the real invitation + consent steps.
async fn link(pool: &SqlitePool, profile: &str, chat: i64) {
    let invite = repo::create_invite(pool, profile).await.unwrap();
    let r = say(pool, chat, &format!("/start {}", invite.code)).await;
    assert!(texts(&r).contains("Do you agree"), "{}", texts(&r));
    press(pool, chat, "consent:y").await;
    assert!(repo::link_by_chat(pool, chat).await.unwrap().unwrap().consented);
}

// ------------------------------------------------------------------ linking, consent, privacy

#[tokio::test]
async fn unlinked_chats_get_nothing_and_groups_bots_are_ignored() {
    let pool = test_pool().await;
    make_profile(&pool, "p1", &[("full_name", txt("Sara Karimi"))]).await;
    for t in ["/start", "/profile", "/matches", "hello", "/edit"] {
        let r = texts(&say(&pool, 500, t).await);
        assert!(r.contains("invitation from your matchmaker"), "{t}: {r}");
        assert!(!r.contains("Sara"));
    }
    assert!(texts(&say(&pool, 500, "/help").await).contains("How this works"));
    // groups and bots are ignored entirely
    let group: Update = serde_json::from_value(json!({"update_id": 3, "message": {"message_id": 1, "from": {"id": 5, "is_bot": false}, "chat": {"id": -100, "type": "group"}, "text": "/start"}})).unwrap();
    assert!(handle_update(&pool, group).await.unwrap().is_empty());
    let bot: Update = serde_json::from_value(json!({"update_id": 3, "message": {"message_id": 1, "from": {"id": 6, "is_bot": true}, "chat": {"id": 6, "type": "private"}, "text": "/start"}})).unwrap();
    assert!(handle_update(&pool, bot).await.unwrap().is_empty());
    // a button pressed in an unlinked chat does nothing
    assert!(texts(&press(&pool, 500, "intro:y:whatever").await).contains("not linked"));
}

#[tokio::test]
async fn invitation_consent_and_single_use() {
    let pool = test_pool().await;
    make_profile(&pool, "p1", &[("full_name", txt("Sara Karimi")), ("religion", txt("Secretive"))]).await;
    let invite = repo::create_invite(&pool, "p1").await.unwrap();
    assert_eq!(invite.code.len(), 10);
    // only a hash is stored
    let stored: String = sqlx::query_scalar("SELECT code_hash FROM mm_telegram_invites").fetch_one(&pool).await.unwrap();
    assert_ne!(stored, invite.code);
    assert_eq!(stored.len(), 64);

    // lowercase and dashes are tolerated
    let sloppy = format!("{}-{}", &invite.code[..5].to_lowercase(), &invite.code[5..]);
    let r = say(&pool, 100, &format!("/start {sloppy}")).await;
    assert!(texts(&r).contains("Before we start"));
    let kb = first_keyboard(&r).unwrap();
    assert_eq!(kb[0][0].callback_data, "consent:y");

    // until the person agrees nothing else works and nothing is shown
    for t in ["/profile", "/matches", "hello"] {
        let r = say(&pool, 100, t).await;
        assert!(texts(&r).contains("Do you agree"), "{t}");
        assert!(!texts(&r).contains("Sara"));
    }
    assert!(repo::get_state(&pool, 100).await.unwrap().is_none());

    // the same code cannot be used by someone else
    let r = say(&pool, 200, &format!("/start {}", invite.code)).await;
    assert!(texts(&r).contains("not valid or has expired"));

    let r = press(&pool, 100, "consent:y").await;
    assert!(texts(&r).contains("Welcome, Sara"));
    assert!(r.iter().any(|x| matches!(x, Reply::EditMarkup { keyboard: None, message_id: 77, .. })), "buttons are removed");
    let l = repo::link_by_chat(&pool, 100).await.unwrap().unwrap();
    assert!(l.consented && l.notifications_enabled);
    assert_eq!(l.profile_id, "p1");
    assert!(texts(&press(&pool, 100, "consent:y").await).contains("Already answered"));
    // a second chat cannot claim the same profile with a new invitation either
    let again = repo::create_invite(&pool, "p1").await.unwrap();
    assert!(texts(&say(&pool, 300, &format!("/start {}", again.code)).await).contains("already linked"));
}

#[tokio::test]
async fn declining_the_notice_unlinks() {
    let pool = test_pool().await;
    make_profile(&pool, "p1", &[]).await;
    let invite = repo::create_invite(&pool, "p1").await.unwrap();
    say(&pool, 100, &format!("/start {}", invite.code)).await;
    let r = press(&pool, 100, "consent:n").await;
    assert!(texts(&r).contains("unlinked this chat"));
    assert!(repo::link_by_chat(&pool, 100).await.unwrap().is_none());
    // /stop during the notice also unlinks
    let invite = repo::create_invite(&pool, "p1").await.unwrap();
    say(&pool, 100, &format!("/start {}", invite.code)).await;
    say(&pool, 100, "/stop").await;
    assert!(repo::link_by_chat(&pool, 100).await.unwrap().is_none());
}

#[tokio::test]
async fn guessing_is_limited_expired_and_revoked_codes_fail() {
    let pool = test_pool().await;
    make_profile(&pool, "p1", &[]).await;
    let real = repo::create_invite(&pool, "p1").await.unwrap();
    for i in 0..5 {
        let r = texts(&say(&pool, 100, &format!("/start WRONGCODE{i}")).await);
        assert!(r.contains("not valid"), "{r}");
    }
    // after five failures even the right code is refused from that chat
    assert!(texts(&say(&pool, 100, &format!("/start {}", real.code)).await).contains("Too many attempts"));
    // another chat is unaffected
    assert!(texts(&say(&pool, 101, &format!("/start {}", real.code)).await).contains("Before we start"));

    // expired
    make_profile(&pool, "p2", &[]).await;
    let old = repo::create_invite(&pool, "p2").await.unwrap();
    sqlx::query("UPDATE mm_telegram_invites SET expires_at = ? WHERE profile_id = 'p2'").bind("2000-01-01T00:00:00+00:00").execute(&pool).await.unwrap();
    assert!(texts(&say(&pool, 102, &format!("/start {}", old.code)).await).contains("not valid or has expired"));
    // a newer invitation revokes the previous one
    make_profile(&pool, "p3", &[]).await;
    let first = repo::create_invite(&pool, "p3").await.unwrap();
    let second = repo::create_invite(&pool, "p3").await.unwrap();
    assert!(texts(&say(&pool, 103, &format!("/start {}", first.code)).await).contains("not valid"));
    assert!(texts(&say(&pool, 104, &format!("/start {}", second.code)).await).contains("Before we start"));
}

#[tokio::test]
async fn profile_view_never_shows_sensitive_values() {
    let pool = test_pool().await;
    make_profile(&pool, "p1", &[("full_name", txt("Sara Karimi")), ("age", Value::Num(29.0)), ("city", txt("Shiraz")), ("religion", txt("TopSecretFaith")), ("health_notes", txt("TopSecretDiagnosis")), ("phone", txt("+989000000"))]).await;
    link(&pool, "p1", 100).await;
    let r = texts(&say(&pool, 100, "/profile").await);
    assert!(r.contains("Age: 29") && r.contains("Current city: Shiraz"), "{r}");
    assert!(!r.contains("TopSecret") && !r.contains("+989") && !r.contains("Karimi"), "{r}");
    assert!(r.contains("sensitive detail(s) are saved but not shown"), "{r}");
    assert!(r.contains("Still needed"), "missing required fields are listed");
}

// ------------------------------------------------------------------ editing and preferences

#[tokio::test]
async fn editing_walks_through_missing_fields() {
    let pool = test_pool().await;
    // required fields: full_name, gender, age, city, education, occupation, marital_status, has_children
    make_profile(&pool, "p1", &[("full_name", txt("Sara")), ("gender", txt("female")), ("education", txt("master")), ("occupation", txt("Engineer")), ("marital_status", txt("never_married")), ("has_children", Value::Bool(false))]).await;
    link(&pool, "p1", 100).await;
    let r = say(&pool, 100, "/edit").await;
    assert!(texts(&r).contains("Age"), "{}", texts(&r));
    // typed nonsense is rejected and the question is asked again
    let r = say(&pool, 100, "twenty-nine").await;
    assert!(texts(&r).contains("could not read that") && texts(&r).contains("Age"));
    let r = say(&pool, 100, "29").await;
    assert!(texts(&r).contains("Saved") && texts(&r).contains("Current city"), "{}", texts(&r));
    let p = MmRepository::get_profile(&pool, "p1").await.unwrap().unwrap();
    assert_eq!(p.profile.get("age"), Some(&Value::Num(29.0)));
    assert_eq!(p.profile.fields["age"].source, Provenance::User);
    // free text answer finishes the profile
    let r = say(&pool, 100, "Shiraz").await;
    assert!(texts(&r).contains("profile is complete"), "{}", texts(&r));
    assert!(repo::get_state(&pool, 100).await.unwrap().is_none());
    // with everything filled in, /edit offers a menu; changing a choice field uses buttons
    let r = say(&pool, 100, "/edit").await;
    assert!(texts(&r).contains("What would you like to change"));
    let r = press(&pool, 100, "edit:f:smoking").await;
    let kb = first_keyboard(&r).unwrap();
    assert!(kb.iter().flatten().any(|b| b.callback_data == "edit:c:0"));
    let r = press(&pool, 100, "edit:c:2").await;
    assert!(texts(&r).contains("Saved"));
    let p = MmRepository::get_profile(&pool, "p1").await.unwrap().unwrap();
    assert_eq!(p.profile.get("smoking"), Some(&txt("regularly")));
    // pressing an answer button after the question is gone does nothing
    assert!(texts(&press(&pool, 100, "edit:c:1").await).contains("no longer open"));
}

#[tokio::test]
async fn answers_cannot_overwrite_what_the_matchmaker_set_and_skip_works() {
    let pool = test_pool().await;
    make_profile(&pool, "p1", &[("full_name", txt("Sara")), ("gender", txt("female")), ("education", txt("master")), ("occupation", txt("Engineer")), ("marital_status", txt("never_married")), ("has_children", Value::Bool(false)), ("age", Value::Num(29.0))]).await;
    link(&pool, "p1", 100).await;
    say(&pool, 100, "/edit").await; // asks for the city (the only missing field)
    press(&pool, 100, "edit:skip").await;
    let p = MmRepository::get_profile(&pool, "p1").await.unwrap().unwrap();
    assert!(p.profile.get("city").is_none(), "skipping stores nothing");
    // change the matchmaker-set age through the menu: refused, value kept
    say(&pool, 100, "/edit").await;
    press(&pool, 100, "edit:f:age").await;
    let r = say(&pool, 100, "35").await;
    assert!(texts(&r).contains("only they can change it"), "{}", texts(&r));
    let p = MmRepository::get_profile(&pool, "p1").await.unwrap().unwrap();
    assert_eq!(p.profile.get("age"), Some(&Value::Num(29.0)));
    assert_eq!(p.profile.fields["age"].source, Provenance::Matchmaker);
}

#[tokio::test]
async fn preferences_can_be_added_listed_and_removed() {
    let pool = test_pool().await;
    make_profile(&pool, "p1", &[("full_name", txt("Sara"))]).await;
    link(&pool, "p1", 100).await;
    assert!(texts(&say(&pool, 100, "/preferences").await).contains("Nothing set yet"));

    // choice field: smoking in [never, occasionally], deal-breaker wording via "Must have"
    let r = press(&pool, 100, "pref:add").await;
    assert!(first_keyboard(&r).unwrap().iter().flatten().any(|b| b.callback_data == "pref:f:smoking"));
    press(&pool, 100, "pref:f:smoking").await;
    press(&pool, 100, "pref:m:0").await; // never
    let r = press(&pool, 100, "pref:m:1").await; // occasionally
    assert!(r.iter().any(|x| matches!(x, Reply::EditMarkup { message_id: 77, keyboard: Some(_), .. })), "buttons are redrawn on the tapped message");
    assert!(texts(&press(&pool, 100, "pref:s:required").await).contains("Please answer the earlier question first"));
    press(&pool, 100, "pref:done").await;
    let r = press(&pool, 100, "pref:s:required").await;
    assert!(texts(&r).contains("Smoking is never or occasionally (must have)"), "{}", texts(&r));

    // number field: age at least 25, no upper limit
    press(&pool, 100, "pref:add").await;
    press(&pool, 100, "pref:f:age").await;
    assert!(texts(&say(&pool, 100, "banana").await).contains("send a number"));
    say(&pool, 100, "25").await;
    let r = say(&pool, 100, "skip").await;
    assert!(first_keyboard(&r).is_some());
    let r = press(&pool, 100, "pref:s:preferred").await;
    assert!(texts(&r).contains("Age is at least 25 (preferred)"), "{}", texts(&r));

    let prefs = MmRepository::get_preferences(&pool, "p1").await.unwrap();
    assert_eq!(prefs.len(), 2);
    assert!(prefs[0].condition.op == ConditionOp::In && prefs[1].condition.op == ConditionOp::Ge);

    let r = say(&pool, 100, "/preferences").await;
    assert!(texts(&r).contains("1. Smoking") && texts(&r).contains("2. Age"));
    let kb = first_keyboard(&r).unwrap();
    let del = kb.iter().flatten().find(|b| b.text == "Remove 1").unwrap().callback_data.clone();
    let r = press(&pool, 100, &del).await;
    assert!(!texts(&r).contains("Smoking"));
    assert_eq!(MmRepository::get_preferences(&pool, "p1").await.unwrap().len(), 1);
    assert!(texts(&press(&pool, 100, &del).await).contains("already removed"));
    // the matchmaker's engine reads exactly what the person entered
    assert!(matchmaking_core::validate_preference(&MmRepository::get_preferences(&pool, "p1").await.unwrap()[0], &MmRepository::registry(&pool).await.unwrap()).is_ok());
}

// ------------------------------------------------------------------ introductions

async fn new_match(pool: &SqlitePool, a: &str, b: &str) -> String {
    let card = json!({"eligible": true, "ranking_score": 70.0});
    let set_id = MmRepository::create_rule_set(pool, &format!("rs-{a}-{b}"), "", &matchmaking_core::default_ruleset(), None).await.unwrap();
    MatchRepo::create(pool, a, b, None, &set_id, 1, MatchStatus::Approved, &card).await.unwrap()
}

/// What the matchmaker's "Propose introduction" does, minus the Tauri wrapper.
async fn propose(pool: &SqlitePool, id: &str) {
    MatchRepo::record_transition(pool, id, MatchStatus::Approved, MatchStatus::IntroductionProposed, None, None, None).await.unwrap();
    notify::after_status_change(pool, id, MatchStatus::Approved, MatchStatus::IntroductionProposed).await;
}

async fn outbox(pool: &SqlitePool, profile: &str) -> Vec<repo::OutboxRow> {
    repo::outbox_for_profile(pool, profile, 50).await.unwrap().into_iter().rev().collect()
}

#[tokio::test]
async fn introduction_flow_both_say_yes() {
    let pool = test_pool().await;
    make_profile(&pool, "pa", &[("full_name", txt("Sara Karimi")), ("age", Value::Num(29.0)), ("city", txt("Shiraz")), ("religion", txt("PrivateFaith")), ("health_notes", txt("PrivateHealth")), ("phone", txt("+989111")), ("telegram_username", txt("sara_k"))]).await;
    make_profile(&pool, "pb", &[("full_name", txt("Ali Rezaei")), ("age", Value::Num(33.0)), ("city", txt("Shiraz")), ("occupation", txt("Doctor"))]).await;
    link(&pool, "pa", 100).await;
    link(&pool, "pb", 200).await;
    let m = new_match(&pool, "pa", "pb").await;
    propose(&pool, &m).await;

    let a_msgs = outbox(&pool, "pa").await;
    let b_msgs = outbox(&pool, "pb").await;
    assert_eq!(a_msgs.len(), 1);
    assert_eq!(b_msgs.len(), 1);
    // pa is introduced to Ali, pb to Sara: first names and shared facts only
    assert!(a_msgs[0].text.contains("Ali") && a_msgs[0].text.contains("Doctor") && a_msgs[0].text.contains("33"));
    assert!(b_msgs[0].text.contains("Sara") && b_msgs[0].text.contains("Shiraz") && b_msgs[0].text.contains("29"));
    for t in [&a_msgs[0].text, &b_msgs[0].text] {
        for secret in ["Karimi", "Rezaei", "PrivateFaith", "PrivateHealth", "+989111", "sara_k"] {
            assert!(!t.contains(secret), "introduction leaked {secret}: {t}");
        }
    }
    assert_eq!(a_msgs[0].keyboard.as_ref().unwrap()[0][0].callback_data, format!("intro:y:{m}"));

    // /matches shows only what a person may know
    let r = texts(&say(&pool, 100, "/matches").await);
    assert!(r.contains("1. Ali - Waiting for your answer"), "{r}");
    assert!(!r.contains("Doctor"), "the list does not repeat the card");
    let r = texts(&say(&pool, 100, "/match 1").await);
    assert!(r.contains("Doctor") && r.contains("Would you like to be introduced"));

    // pa says yes: still waiting for Ali; the match has not moved
    let r = press(&pool, 100, &format!("intro:y:{m}")).await;
    assert!(texts(&r).contains("I will let you know when Ali has answered"), "{}", texts(&r));
    assert_eq!(MatchRepo::get(&pool, &m).await.unwrap().unwrap().status, MatchStatus::IntroductionProposed);
    assert!(texts(&press(&pool, 100, &format!("intro:y:{m}")).await).contains("Already recorded"));
    assert!(texts(&say(&pool, 100, "/matches").await).contains("Waiting for Ali to answer"));

    // pb says yes: both interested, both told through the outbox, without contact details
    let r = press(&pool, 200, &format!("intro:y:{m}")).await;
    assert!(!texts(&r).contains("will let you know"), "no duplicate chatter");
    let row = MatchRepo::get(&pool, &m).await.unwrap().unwrap();
    assert_eq!(row.status, MatchStatus::BothInterested);
    for (who, other) in [("pa", "Ali"), ("pb", "Sara")] {
        let msgs = outbox(&pool, who).await;
        let good = msgs.iter().find(|r| r.kind == "both_interested").expect("both-interested notice");
        assert!(good.text.contains(other) && !good.text.contains("+989") && !good.text.contains("sara_k"));
    }
    // a person may still change their mind before contact is exchanged: that ends the introduction, neutrally for the other
    let r = press(&pool, 100, &format!("intro:n:{m}")).await;
    assert!(texts(&r).contains("noted that you are not interested"));
    assert_eq!(MatchRepo::get(&pool, &m).await.unwrap().unwrap().status, MatchStatus::Declined);
    let note = outbox(&pool, "pb").await.into_iter().find(|r| r.kind == "not_going_ahead").expect("neutral notice for the other person");
    assert!(!note.text.contains("Sara"));
    // once the introduction is over, buttons do nothing more
    assert!(texts(&press(&pool, 200, &format!("intro:n:{m}")).await).contains("no longer open"));
}

#[tokio::test]
async fn a_decline_is_neutral_and_private() {
    let pool = test_pool().await;
    make_profile(&pool, "pa", &[("full_name", txt("Sara"))]).await;
    make_profile(&pool, "pb", &[("full_name", txt("Ali"))]).await;
    link(&pool, "pa", 100).await;
    link(&pool, "pb", 200).await;
    let m = new_match(&pool, "pa", "pb").await;
    propose(&pool, &m).await;
    let r = press(&pool, 200, &format!("intro:n:{m}")).await;
    assert!(texts(&r).contains("noted that you are not interested"));
    assert_eq!(MatchRepo::get(&pool, &m).await.unwrap().unwrap().status, MatchStatus::Declined);
    let a = outbox(&pool, "pa").await;
    let note = a.iter().find(|r| r.kind == "not_going_ahead").expect("neutral notice for the other person");
    assert!(!note.text.contains("Ali") && !note.text.to_lowercase().contains("declin") && !note.text.to_lowercase().contains("not interested"), "{}", note.text);
    // the person who declined gets no further message
    assert!(outbox(&pool, "pb").await.iter().all(|r| r.kind != "not_going_ahead"));
    assert!(texts(&say(&pool, 200, "/matches").await).contains("no introductions"));
}

#[tokio::test]
async fn buttons_are_checked_against_the_database() {
    let pool = test_pool().await;
    for (id, name) in [("pa", "Sara"), ("pb", "Ali"), ("pc", "Mina")] {
        make_profile(&pool, id, &[("full_name", txt(name))]).await;
    }
    link(&pool, "pa", 100).await;
    link(&pool, "pb", 200).await;
    link(&pool, "pc", 300).await;
    let m = new_match(&pool, "pa", "pb").await;
    // before the introduction is proposed nobody can answer, and the pair is invisible
    assert!(texts(&press(&pool, 100, &format!("intro:y:{m}")).await).contains("no longer open"));
    assert!(texts(&say(&pool, 100, "/matches").await).contains("no introductions"));
    propose(&pool, &m).await;
    // an outsider pressing someone else's button learns nothing and changes nothing
    let r = press(&pool, 300, &format!("intro:y:{m}")).await;
    assert!(texts(&r).contains("no longer valid"));
    assert_eq!(MatchRepo::get(&pool, &m).await.unwrap().unwrap().a_response, Interest::Unknown);
    // malformed and unknown data is harmless
    for data in ["intro:y", "intro:maybe:xyz", "intro:y:does-not-exist", "match:open:nope", "bogus:1", "", "pref:s:required", "edit:c:0"] {
        let r = press(&pool, 100, data).await;
        assert!(!texts(&r).is_empty(), "{data} gets a polite answer");
    }
    // a profile with no link cannot be reached by a stale button
    repo::delete_link(&pool, "pb").await.unwrap();
    assert!(texts(&press(&pool, 200, &format!("intro:y:{m}")).await).contains("not linked"));
}

#[tokio::test]
async fn unreachable_people_are_skipped() {
    let pool = test_pool().await;
    for (id, name) in [("pa", "Sara"), ("pb", "Ali")] {
        make_profile(&pool, id, &[("full_name", txt(name))]).await;
    }
    link(&pool, "pa", 100).await;
    // pb is not on Telegram; pa switches notifications off
    say(&pool, 100, "/stop").await;
    let m = new_match(&pool, "pa", "pb").await;
    propose(&pool, &m).await;
    assert!(outbox(&pool, "pa").await.is_empty() && outbox(&pool, "pb").await.is_empty());
    let row = MatchRepo::get(&pool, &m).await.unwrap().unwrap();
    assert_eq!(notify::reachable_sides(&pool, &row).await.unwrap(), (false, false));
    say(&pool, 100, "/resume").await;
    assert_eq!(notify::reachable_sides(&pool, &row).await.unwrap(), (true, false));
    // /match still works, so a person who turned notifications off can look
    assert!(texts(&say(&pool, 100, "/matches").await).contains("1. Ali"));
}

#[tokio::test]
async fn withdrawn_introductions_are_announced_neutrally() {
    let pool = test_pool().await;
    for (id, name) in [("pa", "Sara"), ("pb", "Ali")] {
        make_profile(&pool, id, &[("full_name", txt(name))]).await;
    }
    link(&pool, "pa", 100).await;
    link(&pool, "pb", 200).await;
    let m = new_match(&pool, "pa", "pb").await;
    propose(&pool, &m).await;
    MatchRepo::record_transition(&pool, &m, MatchStatus::IntroductionProposed, MatchStatus::Rejected, None, None, Some("internal reason")).await.unwrap();
    notify::after_status_change(&pool, &m, MatchStatus::IntroductionProposed, MatchStatus::Rejected).await;
    for who in ["pa", "pb"] {
        let w = outbox(&pool, who).await.into_iter().find(|r| r.kind == "withdrawn").expect("withdrawal notice");
        assert!(!w.text.contains("internal reason"));
    }
}

#[tokio::test]
async fn rejecting_a_pair_that_was_never_introduced_tells_nobody() {
    let pool = test_pool().await;
    for (id, name) in [("pa", "Sara"), ("pb", "Ali")] {
        make_profile(&pool, id, &[("full_name", txt(name))]).await;
    }
    link(&pool, "pa", 100).await;
    link(&pool, "pb", 200).await;
    let m = new_match(&pool, "pa", "pb").await;
    MatchRepo::record_transition(&pool, &m, MatchStatus::Approved, MatchStatus::Rejected, None, None, None).await.unwrap();
    notify::after_status_change(&pool, &m, MatchStatus::Approved, MatchStatus::Rejected).await;
    assert!(outbox(&pool, "pa").await.is_empty() && outbox(&pool, "pb").await.is_empty());
}

// ------------------------------------------------------------------ messages, settings, requests

#[tokio::test]
async fn free_text_reaches_the_matchmaker_and_settings_work() {
    let pool = test_pool().await;
    make_profile(&pool, "p1", &[("full_name", txt("Sara"))]).await;
    link(&pool, "p1", 100).await;
    let r = say(&pool, 100, "Can we talk on Friday?").await;
    assert!(texts(&r).contains("passed your message to your matchmaker"));
    assert_eq!(repo::unread_counts(&pool).await.unwrap().get("p1"), Some(&1));
    let msgs = repo::messages(&pool, "p1", 10).await.unwrap();
    assert_eq!(msgs[0].text, "Can we talk on Friday?");
    assert_eq!(msgs[0].direction, "in");
    repo::mark_read(&pool, "p1").await.unwrap();
    assert!(repo::unread_counts(&pool).await.unwrap().is_empty());
    // over-long and non-text messages are refused politely
    assert!(texts(&say(&pool, 100, &"x".repeat(2500)).await).contains("too long"));
    let photo: Update = serde_json::from_value(json!({"update_id": 5, "message": {"message_id": 1, "from": {"id": 100, "is_bot": false}, "chat": {"id": 100, "type": "private"}}})).unwrap();
    assert!(texts(&handle_update(&pool, photo).await.unwrap()).contains("only read text"));
    // settings toggle
    let r = press(&pool, 100, "set:n:off").await;
    assert!(texts(&r).contains("Notifications: off"));
    assert!(!repo::link_by_chat(&pool, 100).await.unwrap().unwrap().notifications_enabled);
    // /cancel and unknown commands
    assert!(texts(&say(&pool, 100, "/cancel").await).contains("cancelled"));
    assert!(texts(&say(&pool, 100, "/dance").await).contains("did not understand"));
    // a reply to an information request keeps its match
    repo::set_state(&pool, 100, &json!({"flow": "reply", "match_id": "m-1"})).await.unwrap();
    say(&pool, 100, "I studied at Shiraz University").await;
    let msgs = repo::messages(&pool, "p1", 10).await.unwrap();
    assert_eq!(msgs[0].match_id.as_deref(), Some("m-1"));
    assert!(repo::get_state(&pool, 100).await.unwrap().is_none());
}

#[tokio::test]
async fn unlink_and_forget_are_confirmed_and_complete() {
    let pool = test_pool().await;
    make_profile(&pool, "p1", &[("full_name", txt("Sara"))]).await;
    link(&pool, "p1", 100).await;
    repo::enqueue(&pool, "p1", "profile_reminder", "hi", None, None).await.unwrap();
    // asking is not doing
    assert!(first_keyboard(&say(&pool, 100, "/forget").await).is_some());
    assert!(repo::open_requests(&pool).await.unwrap().is_empty());
    press(&pool, 100, "forget:n").await;
    assert!(repo::open_requests(&pool).await.unwrap().is_empty());
    press(&pool, 100, "forget:y").await;
    press(&pool, 100, "forget:y").await; // idempotent
    assert_eq!(repo::open_requests(&pool).await.unwrap().len(), 1);
    repo::resolve_request(&pool, repo::open_requests(&pool).await.unwrap()[0].0).await.unwrap();
    assert!(repo::open_requests(&pool).await.unwrap().is_empty());

    assert!(first_keyboard(&say(&pool, 100, "/unlink").await).is_some());
    press(&pool, 100, "unlink:y").await;
    assert!(repo::link_by_chat(&pool, 100).await.unwrap().is_none());
    assert_eq!(outbox(&pool, "p1").await[0].status, "cancelled", "queued messages are not sent after unlinking");
    assert!(texts(&say(&pool, 100, "/profile").await).contains("invitation"));
    // deleting the profile removes everything attached to it
    link(&pool, "p1", 101).await;
    say(&pool, 101, "hello matchmaker").await;
    MmRepository::delete_profile(&pool, "p1").await.unwrap();
    for table in ["mm_telegram_links", "mm_telegram_messages", "mm_outbox", "mm_telegram_invites", "mm_data_requests"] {
        let n: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM {table}")).fetch_one(&pool).await.unwrap();
        assert_eq!(n, 0, "{table} still has rows after the profile was deleted");
    }
}

// ------------------------------------------------------------------ delivery

#[derive(Default)]
struct MockApi {
    sent: Mutex<Vec<(i64, String)>>,
    next: Mutex<Vec<Result<i64, TgError>>>,
}

impl MockApi {
    fn script(&self, results: Vec<Result<i64, TgError>>) {
        *self.next.lock().unwrap() = results;
    }
}

#[async_trait]
impl TelegramApi for MockApi {
    async fn get_me(&self) -> Result<BotInfo, TgError> {
        Ok(BotInfo { id: 1, username: "matchwise_test_bot".into() })
    }
    async fn get_updates(&self, _o: i64, _t: u32) -> Result<Vec<Update>, TgError> {
        Ok(vec![])
    }
    async fn send_message(&self, chat_id: i64, html: &str, _k: Option<&Keyboard>) -> Result<i64, TgError> {
        let r = { let mut n = self.next.lock().unwrap(); if n.is_empty() { Ok(1) } else { n.remove(0) } };
        if r.is_ok() {
            self.sent.lock().unwrap().push((chat_id, html.to_string()));
        }
        r
    }
    async fn answer_callback(&self, _i: &str, _t: Option<&str>) -> Result<(), TgError> {
        Ok(())
    }
    async fn edit_markup(&self, _c: i64, _m: i64, _k: Option<&Keyboard>) -> Result<(), TgError> {
        Ok(())
    }
    async fn set_commands(&self) -> Result<(), TgError> {
        Ok(())
    }
}

#[tokio::test]
async fn outbox_delivery_retries_and_respects_recipients() {
    let pool = test_pool().await;
    for id in ["p1", "p2", "p3"] {
        make_profile(&pool, id, &[("full_name", txt(id))]).await;
    }
    link(&pool, "p1", 100).await;
    link(&pool, "p2", 200).await;
    link(&pool, "p3", 300).await;
    let api = MockApi::default();

    // success
    let id1 = repo::enqueue(&pool, "p1", "x", "hello p1", None, None).await.unwrap();
    let st = flush_outbox(&pool, &api).await.unwrap();
    assert_eq!((st.sent, st.failed), (1, 0));
    assert_eq!(api.sent.lock().unwrap()[0], (100, "hello p1".to_string()));
    assert!(repo::due(&pool, 10).await.unwrap().iter().all(|r| r.id != id1));

    // a person who withdrew in the meantime is never messaged
    repo::enqueue(&pool, "p2", "x", "late news", None, None).await.unwrap();
    repo::set_notifications(&pool, "p2", false).await.unwrap();
    let st = flush_outbox(&pool, &api).await.unwrap();
    assert_eq!(st.cancelled, 1);
    assert_eq!(api.sent.lock().unwrap().len(), 1);

    // blocked bot: given up, notifications switched off
    repo::enqueue(&pool, "p3", "x", "blocked", None, None).await.unwrap();
    api.script(vec![Err(TgError::Forbidden)]);
    let st = flush_outbox(&pool, &api).await.unwrap();
    assert_eq!(st.failed, 1);
    assert!(!repo::link_by_profile(&pool, "p3").await.unwrap().unwrap().notifications_enabled);

    // rate limit: kept for later; transient errors: backoff, then give up after the maximum
    repo::set_notifications(&pool, "p3", true).await.unwrap();
    let rl = repo::enqueue(&pool, "p3", "x", "rate", None, None).await.unwrap();
    api.script(vec![Err(TgError::RateLimited { retry_after_secs: 7 })]);
    assert_eq!(flush_outbox(&pool, &api).await.unwrap().retried, 1);
    assert!(repo::due(&pool, 10).await.unwrap().iter().all(|r| r.id != rl), "not due again immediately");
    let net = repo::enqueue(&pool, "p1", "x", "flaky", None, None).await.unwrap();
    for attempt in 0..MAX_ATTEMPTS {
        sqlx::query("UPDATE mm_outbox SET next_attempt_at = '2000-01-01T00:00:00+00:00' WHERE id = ?").bind(net).execute(&pool).await.unwrap();
        api.script(vec![Err(TgError::Network("down".into()))]);
        flush_outbox(&pool, &api).await.unwrap();
        let row = repo::outbox_for_profile(&pool, "p1", 20).await.unwrap().into_iter().find(|r| r.id == net).unwrap();
        assert_eq!(row.attempts, attempt + 1);
        assert_eq!(row.status, if attempt + 1 >= MAX_ATTEMPTS { "failed" } else { "pending" });
    }
    // a bad token stops the run with an error
    repo::enqueue(&pool, "p1", "x", "auth", None, None).await.unwrap();
    api.script(vec![Err(TgError::Unauthorized)]);
    assert!(flush_outbox(&pool, &api).await.is_err());
}

#[tokio::test]
async fn reminders_are_patient_and_limited() {
    let pool = test_pool().await;
    for (id, name) in [("pa", "Sara"), ("pb", "Ali")] {
        make_profile(&pool, id, &[("full_name", txt(name))]).await;
    }
    link(&pool, "pa", 100).await;
    link(&pool, "pb", 200).await;
    // right after linking nobody is nagged
    assert_eq!(notify::queue_reminders(&pool).await.unwrap(), 0);
    // three days later incomplete profiles get a reminder, at most three times
    sqlx::query("UPDATE mm_telegram_links SET linked_at = '2000-01-01T00:00:00+00:00'").execute(&pool).await.unwrap();
    assert_eq!(notify::queue_reminders(&pool).await.unwrap(), 2);
    assert_eq!(notify::queue_reminders(&pool).await.unwrap(), 0, "not again straight away");
    for _ in 0..4 {
        sqlx::query("UPDATE mm_outbox SET created_at = '2000-01-01T00:00:00+00:00'").execute(&pool).await.unwrap();
        notify::queue_reminders(&pool).await.unwrap();
    }
    assert_eq!(repo::count_kind(&pool, "pa", "profile_reminder", None).await.unwrap(), 3);

    // introductions waiting for an answer: reminded after three days, twice at most, never once answered
    let m = new_match(&pool, "pa", "pb").await;
    propose(&pool, &m).await;
    assert_eq!(notify::queue_reminders(&pool).await.unwrap(), 0);
    press(&pool, 100, &format!("intro:y:{m}")).await; // pa answers
    sqlx::query("UPDATE mm_outbox SET created_at = '2000-01-01T00:00:00+00:00' WHERE kind IN ('introduction','intro_reminder')").execute(&pool).await.unwrap();
    notify::queue_reminders(&pool).await.unwrap();
    assert_eq!(repo::count_kind(&pool, "pa", "intro_reminder", Some(&m)).await.unwrap(), 0, "pa already answered");
    assert_eq!(repo::count_kind(&pool, "pb", "intro_reminder", Some(&m)).await.unwrap(), 1);
    for _ in 0..3 {
        sqlx::query("UPDATE mm_outbox SET created_at = '2000-01-01T00:00:00+00:00' WHERE kind IN ('introduction','intro_reminder')").execute(&pool).await.unwrap();
        notify::queue_reminders(&pool).await.unwrap();
    }
    assert_eq!(repo::count_kind(&pool, "pb", "intro_reminder", Some(&m)).await.unwrap(), 2);
    // a hold (waiting for information) pauses reminders
    MatchRepo::set_hold(&pool, &m, Some("need info")).await.unwrap();
    sqlx::query("UPDATE mm_outbox SET created_at = '2000-01-01T00:00:00+00:00'").execute(&pool).await.unwrap();
    let before = repo::count_kind(&pool, "pb", "intro_reminder", Some(&m)).await.unwrap();
    notify::queue_reminders(&pool).await.unwrap();
    assert_eq!(repo::count_kind(&pool, "pb", "intro_reminder", Some(&m)).await.unwrap(), before);
}

// ------------------------------------------------------------------ the HTTP layer against a local stand-in server

async fn stand_in_server(replies: Vec<(u16, String)>) -> (String, std::sync::Arc<Mutex<Vec<String>>>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let seen = std::sync::Arc::new(Mutex::new(vec![]));
    let seen2 = seen.clone();
    tokio::spawn(async move {
        for (status, body) in replies {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 16384];
            let mut total = 0;
            loop {
                let n = sock.read(&mut buf[total..]).await.unwrap();
                total += n;
                let text = String::from_utf8_lossy(&buf[..total]).to_string();
                if let Some(split) = text.find("\r\n\r\n") {
                    let len = text.lines().find_map(|l| l.to_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap_or(0))).unwrap_or(0);
                    if total >= split + 4 + len || n == 0 {
                        break;
                    }
                }
                if n == 0 {
                    break;
                }
            }
            seen2.lock().unwrap().push(String::from_utf8_lossy(&buf[..total]).to_string());
            let resp = format!("HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body);
            sock.write_all(resp.as_bytes()).await.unwrap();
        }
    });
    (format!("http://{addr}"), seen)
}

#[tokio::test]
async fn http_api_builds_requests_and_maps_errors() {
    let token = "123456:SECRET-token_value";
    let (base, seen) = stand_in_server(vec![
        (200, r#"{"ok":true,"result":{"id":42,"is_bot":true,"username":"matchwise_bot"}}"#.into()),
        (200, r#"{"ok":true,"result":{"message_id":9}}"#.into()),
        (200, r#"{"ok":true,"result":[{"update_id":5,"message":{"message_id":1,"from":{"id":7,"is_bot":false},"chat":{"id":7,"type":"private"},"text":"hi"}},{"update_id":6,"weird":true},{"update_id":"bad"}]}"#.into()),
        (401, r#"{"ok":false,"error_code":401,"description":"Unauthorized"}"#.into()),
        (403, r#"{"ok":false,"error_code":403,"description":"Forbidden: bot was blocked by the user"}"#.into()),
        (429, r#"{"ok":false,"error_code":429,"description":"Too Many Requests","parameters":{"retry_after":13}}"#.into()),
        (400, r#"{"ok":false,"error_code":400,"description":"Bad Request: can't parse entities"}"#.into()),
    ])
    .await;
    let api = HttpTelegramApi::with_base(&base, token);
    assert_eq!(api.get_me().await.unwrap(), BotInfo { id: 42, username: "matchwise_bot".into() });
    let kb: Keyboard = vec![vec![super::types::button("Yes", "intro:y:m1")]];
    assert_eq!(api.send_message(7, "<b>Hi</b>", Some(&kb)).await.unwrap(), 9);
    let ups = api.get_updates(5, 25).await.unwrap();
    // an update with an unreadable id is skipped; an update of a kind we do not use is kept (and ignored by the engine)
    assert_eq!(ups.iter().map(|u| u.update_id).collect::<Vec<_>>(), vec![5, 6]);
    assert_eq!(api.get_me().await.unwrap_err(), TgError::Unauthorized);
    assert_eq!(api.send_message(7, "x", None).await.unwrap_err(), TgError::Forbidden);
    assert_eq!(api.send_message(7, "x", None).await.unwrap_err(), TgError::RateLimited { retry_after_secs: 13 });
    assert!(matches!(api.send_message(7, "<", None).await.unwrap_err(), TgError::BadRequest(_)));

    let reqs = seen.lock().unwrap().clone();
    assert!(reqs[0].starts_with(&format!("POST /bot{token}/getMe ")), "{}", reqs[0]);
    assert!(reqs[1].contains("/sendMessage") && reqs[1].contains("\"parse_mode\":\"HTML\"") && reqs[1].contains("\"callback_data\":\"intro:y:m1\"") && reqs[1].contains("\"chat_id\":7"));
    assert!(reqs[2].contains("/getUpdates") && reqs[2].contains("\"offset\":5") && reqs[2].contains("\"timeout\":25"));
}

#[tokio::test]
async fn errors_never_contain_the_token() {
    let token = "999999:VERY-SECRET-TOKEN";
    // nothing is listening on this port: the connection error text includes the request URL, which includes the token
    let api = HttpTelegramApi::with_base("http://127.0.0.1:9", token);
    let e = api.send_message(1, "x", None).await.unwrap_err();
    let shown = format!("{e} / {e:?}");
    assert!(matches!(e, TgError::Network(_)), "{shown}");
    assert!(!shown.contains("VERY-SECRET-TOKEN") && !shown.contains(token), "token leaked: {shown}");
}

#[tokio::test]
async fn sharing_settings_round_trip() {
    let pool = test_pool().await;
    assert_eq!(repo::introduction_fields(&pool).await.unwrap(), matchmaking_core::DEFAULT_INTRODUCTION_FIELDS.iter().map(|s| s.to_string()).collect::<Vec<_>>());
    assert!(repo::show_first_name(&pool).await.unwrap());
    repo::set_sharing(&pool, &["city".to_string()], false).await.unwrap();
    assert_eq!(repo::introduction_fields(&pool).await.unwrap(), vec!["city".to_string()]);
    assert!(!repo::show_first_name(&pool).await.unwrap());
    // the first-name setting changes what an introduction says
    make_profile(&pool, "pa", &[("full_name", txt("Sara Karimi")), ("city", txt("Shiraz")), ("age", Value::Num(29.0))]).await;
    let card = notify::card_for(&pool, &MmRepository::get_profile(&pool, "pa").await.unwrap().unwrap()).await.unwrap();
    assert_eq!(card.title, "Someone");
    assert_eq!(card.lines, vec![("Current city".to_string(), "Shiraz".to_string())], "only the one shared field appears");
}
