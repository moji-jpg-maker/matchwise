//! Questionnaire workflow tests: the registry, assigning, answering (in the app and through the bot), and what ends up on the profile.

use super::repo;
use super::service;
use crate::mm::repository::MmRepository;
use crate::telegram::engine::handle_update;
use crate::telegram::repo as tg;
use crate::telegram::types::{Reply, Update};
use matchmaking_core::{builtin_mini_ipip, Evidence, EvidenceLevel, Instrument, Item, Method, Profile, Provenance, Scale, Subscale, Value};
use serde_json::json;
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::SqlitePool;

async fn test_pool() -> SqlitePool {
    let pool = SqlitePoolOptions::new().max_connections(1).connect("sqlite::memory:").await.unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    pool
}

async fn profile(pool: &SqlitePool, id: &str) {
    let mut p = Profile::new(id);
    p.set("full_name", Value::Text("Sara Karimi".into()), Provenance::Matchmaker);
    MmRepository::insert_profile(pool, &p).await.unwrap();
    // make sure the default fields (including the Big Five scores) exist
    MmRepository::registry(pool).await.unwrap();
}

async fn answer_all(pool: &SqlitePool, response: i64, value_for: impl Fn(&Item) -> i32) {
    let r = repo::get_response(pool, response).await.unwrap().unwrap();
    let inst = service::instrument_for(pool, &r).await.unwrap();
    for it in &inst.items {
        service::record_answer(pool, response, &it.id, value_for(it)).await.unwrap();
    }
}

#[tokio::test]
async fn the_registry_ships_one_validated_instrument_and_seeding_is_idempotent() {
    let pool = test_pool().await;
    repo::ensure_builtin(&pool).await.unwrap();
    repo::ensure_builtin(&pool).await.unwrap();
    let all = repo::list_instruments(&pool, false).await.unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].instrument.key, "mini_ipip");
    assert_eq!(all[0].instrument.evidence.level, EvidenceLevel::ValidatedInstrument);
    // archiving is respected: the shipped instrument is not silently re-created or un-archived
    repo::set_archived(&pool, "mini_ipip", true).await.unwrap();
    repo::ensure_builtin(&pool).await.unwrap();
    assert!(repo::list_instruments(&pool, false).await.unwrap().is_empty());
    assert_eq!(repo::list_instruments(&pool, true).await.unwrap().len(), 1);
}

#[tokio::test]
async fn answering_in_the_app_writes_questionnaire_scores_to_the_profile() {
    let pool = test_pool().await;
    repo::ensure_builtin(&pool).await.unwrap();
    profile(&pool, "p1").await;
    let id = repo::assign(&pool, "p1", "mini_ipip", 1).await.unwrap();
    assert_eq!(repo::open_responses(&pool, "p1").await.unwrap().len(), 1);

    // the first answers only move the response along
    let out = service::record_answer(&pool, id, "q1", 4).await.unwrap();
    assert!(!out.completed);
    let r = repo::get_response(&pool, id).await.unwrap().unwrap();
    assert_eq!((r.status.as_str(), r.answers.len()), ("in_progress", 1));
    let p = MmRepository::get_profile(&pool, "p1").await.unwrap().unwrap();
    assert!(p.profile.get("big5_extraversion").is_none(), "no score before the questionnaire is finished");

    // invalid answers are refused and change nothing
    assert!(service::record_answer(&pool, id, "q2", 9).await.is_err());
    assert!(service::record_answer(&pool, id, "nope", 3).await.is_err());

    // finishing computes the scores (all midpoints = 12) with questionnaire provenance
    answer_all(&pool, id, |_| 3).await;
    let r = repo::get_response(&pool, id).await.unwrap().unwrap();
    assert_eq!(r.status, "complete");
    assert_eq!(r.scores.as_ref().unwrap()["neuroticism"], 12.0);
    let p = MmRepository::get_profile(&pool, "p1").await.unwrap().unwrap();
    assert_eq!(p.profile.get("big5_neuroticism"), Some(&Value::Num(12.0)));
    assert_eq!(p.profile.fields["big5_neuroticism"].source, Provenance::Questionnaire);
    assert!(p.profile.validate(&MmRepository::registry(&pool).await.unwrap()).is_empty());
    assert!(service::record_answer(&pool, id, "q1", 5).await.is_err(), "a finished questionnaire is closed");

    // a retake (new response) replaces the scores
    let again = repo::assign(&pool, "p1", "mini_ipip", 1).await.unwrap();
    answer_all(&pool, again, |it| if it.reverse { 1 } else { 5 }).await;
    let p = MmRepository::get_profile(&pool, "p1").await.unwrap().unwrap();
    assert_eq!(p.profile.get("big5_neuroticism"), Some(&Value::Num(20.0)));
    assert_eq!(repo::responses_for_profile(&pool, "p1").await.unwrap().len(), 2, "both responses are kept");

    // the audit log names the questionnaire, never an answer or a score
    let rows: Vec<(String, Option<String>)> = sqlx::query_as("SELECT action, detail FROM mm_audit_log WHERE action LIKE 'questionnaire%'").fetch_all(&pool).await.unwrap();
    assert!(rows.iter().any(|(a, d)| a == "questionnaire_completed" && d.as_deref() == Some("mini_ipip")));
    for (_, detail) in &rows {
        assert!(detail.as_deref().map_or(true, |d| d == "mini_ipip"), "audit detail must be the questionnaire key only, got {detail:?}");
    }
}

#[tokio::test]
async fn a_persons_own_entry_is_not_overwritten_by_a_score() {
    let pool = test_pool().await;
    repo::ensure_builtin(&pool).await.unwrap();
    profile(&pool, "p1").await;
    let mut p = MmRepository::get_profile(&pool, "p1").await.unwrap().unwrap();
    p.profile.set("big5_openness", Value::Num(7.0), Provenance::Matchmaker);
    MmRepository::update_profile_data(&pool, &p.profile).await.unwrap();
    let id = repo::assign(&pool, "p1", "mini_ipip", 1).await.unwrap();
    let mut last = None;
    let inst = builtin_mini_ipip();
    for it in &inst.items {
        last = Some(service::record_answer(&pool, id, &it.id, 3).await.unwrap());
    }
    let out = last.unwrap();
    assert!(out.completed && out.fields.iter().any(|(f, ok)| f == "big5_openness" && !ok));
    let p = MmRepository::get_profile(&pool, "p1").await.unwrap().unwrap();
    assert_eq!(p.profile.get("big5_openness"), Some(&Value::Num(7.0)));
}

fn custom(key: &str) -> Instrument {
    Instrument {
        key: key.into(),
        name: "Family views".into(),
        version: 1,
        kind: "custom".into(),
        dimension: "family".into(),
        description: String::new(),
        evidence: Evidence { level: EvidenceLevel::MatchmakerDefined, sources: vec![], limitations: "Not validated.".into(), interpretation: "Higher = more traditional.".into() },
        scale: Scale { min: 1, max: 5, labels: ["Strongly disagree", "Disagree", "Neutral", "Agree", "Strongly agree"].iter().map(|s| s.to_string()).collect() },
        items: (1..=3).map(|n| Item { id: format!("q{n}"), text: format!("Statement {n}"), subscale: "tradition".into(), reverse: n == 2 }).collect(),
        subscales: vec![Subscale { key: "tradition".into(), label: "Traditional family life".into(), field: "family_tradition".into(), method: Method::Mean }],
    }
}

#[tokio::test]
async fn custom_instruments_create_their_field_and_scores_flow_into_it() {
    let pool = test_pool().await;
    profile(&pool, "p1").await;
    let reg = MmRepository::registry(&pool).await.unwrap();
    assert!(reg.get("family_tradition").is_none());
    // what q_save_instrument does: validate, create the missing numeric field, store the definition
    let c = custom("family_views");
    assert!(matchmaking_core::validate_instrument(&c, &reg).is_empty());
    for def in matchmaking_core::missing_fields(&c, &reg) {
        let order = MmRepository::next_field_order(&pool).await.unwrap();
        MmRepository::save_field(&pool, &def, order).await.unwrap();
    }
    repo::insert_instrument(&pool, &c).await.unwrap();
    let reg = MmRepository::registry(&pool).await.unwrap();
    assert!(reg.get("family_tradition").unwrap().sensitive);
    // versions never overwrite: a second version is a new row, and old responses keep reading the version they were given
    let id = repo::assign(&pool, "p1", "family_views", 1).await.unwrap();
    let mut v2 = c.clone();
    v2.version = 2;
    v2.items[0].text = "Changed wording".into();
    repo::insert_instrument(&pool, &v2).await.unwrap();
    assert_eq!(repo::get_instrument(&pool, "family_views", None).await.unwrap().unwrap().instrument.version, 2);
    let r = repo::get_response(&pool, id).await.unwrap().unwrap();
    assert_eq!(service::instrument_for(&pool, &r).await.unwrap().items[0].text, "Statement 1");
    assert!(repo::insert_instrument(&pool, &c).await.is_err(), "the same key and version cannot be stored twice");
    answer_all(&pool, id, |_| 4).await;
    let p = MmRepository::get_profile(&pool, "p1").await.unwrap().unwrap();
    // mean of (4, reversed 4 -> 2, 4) = 3.33
    assert_eq!(p.profile.get("family_tradition"), Some(&Value::Num(3.33)));
}

#[tokio::test]
async fn deleting_a_profile_deletes_its_responses() {
    let pool = test_pool().await;
    repo::ensure_builtin(&pool).await.unwrap();
    profile(&pool, "p1").await;
    let id = repo::assign(&pool, "p1", "mini_ipip", 1).await.unwrap();
    service::record_answer(&pool, id, "q1", 3).await.unwrap();
    MmRepository::delete_profile(&pool, "p1").await.unwrap();
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM mm_questionnaire_responses").fetch_one(&pool).await.unwrap();
    assert_eq!(n, 0);
}

// ------------------------------------------------------------------ through the bot

fn msg(chat: i64, text: &str) -> Update {
    serde_json::from_value(json!({"update_id": 1, "message": {"message_id": 10, "from": {"id": chat, "is_bot": false}, "chat": {"id": chat, "type": "private"}, "text": text}})).unwrap()
}

fn tap(chat: i64, data: &str) -> Update {
    serde_json::from_value(json!({"update_id": 2, "callback_query": {"id": "cb", "from": {"id": chat, "is_bot": false}, "data": data, "message": {"message_id": 77, "chat": {"id": chat, "type": "private"}, "text": "x"}}})).unwrap()
}

fn texts(r: &[Reply]) -> String {
    r.iter().filter_map(|x| match x { Reply::Send { text, .. } => Some(text.clone()), Reply::AnswerCallback { text: Some(t), .. } => Some(t.clone()), _ => None }).collect::<Vec<_>>().join("\n")
}

fn buttons(r: &[Reply]) -> Vec<String> {
    r.iter().filter_map(|x| match x { Reply::Send { keyboard: Some(k), .. } => Some(k.iter().flatten().map(|b| b.callback_data.clone()).collect::<Vec<_>>()), _ => None }).flatten().collect()
}

async fn link_chat(pool: &SqlitePool, profile_id: &str, chat: i64) {
    let invite = tg::create_invite(pool, profile_id).await.unwrap();
    handle_update(pool, msg(chat, &format!("/start {}", invite.code))).await.unwrap();
    handle_update(pool, tap(chat, "consent:y")).await.unwrap();
}

#[tokio::test]
async fn a_candidate_can_complete_a_questionnaire_in_telegram_and_never_sees_scores() {
    let pool = test_pool().await;
    repo::ensure_builtin(&pool).await.unwrap();
    profile(&pool, "p1").await;
    link_chat(&pool, "p1", 100).await;
    // nothing assigned yet
    assert!(texts(&handle_update(&pool, msg(100, "/questionnaires")).await.unwrap()).contains("no questionnaires waiting"));

    let id = repo::assign(&pool, "p1", "mini_ipip", 1).await.unwrap();
    let r = handle_update(&pool, msg(100, "/questionnaires")).await.unwrap();
    assert!(texts(&r).contains("Mini-IPIP") && texts(&r).contains("0 of 20"));
    assert_eq!(buttons(&r), vec![format!("q:start:{id}")]);

    let r = handle_update(&pool, tap(100, &format!("q:start:{id}"))).await.unwrap();
    assert!(texts(&r).contains("Question 1 of 20") && texts(&r).contains("Am the life of the party") && texts(&r).contains("1 = Very inaccurate") && texts(&r).contains("5 = Very accurate"));
    assert_eq!(buttons(&r), (1..=5).map(|v| format!("q:a:{id}:0:{v}")).collect::<Vec<_>>());

    // answer the first question; a second tap on the same (old) message does nothing
    let r = handle_update(&pool, tap(100, &format!("q:a:{id}:0:4"))).await.unwrap();
    assert!(texts(&r).contains("Question 2 of 20"));
    let stale = handle_update(&pool, tap(100, &format!("q:a:{id}:0:1"))).await.unwrap();
    assert!(texts(&stale).contains("already been answered"));
    assert_eq!(repo::get_response(&pool, id).await.unwrap().unwrap().answers["q1"], 4, "the first answer stands");
    // skipping ahead is not possible either
    assert!(texts(&handle_update(&pool, tap(100, &format!("q:a:{id}:9:3"))).await.unwrap()).contains("already been answered"));
    // out-of-range values are refused
    assert!(!texts(&handle_update(&pool, tap(100, &format!("q:a:{id}:1:9"))).await.unwrap()).contains("Question 3"));

    // stopping and resuming keeps progress
    handle_update(&pool, msg(100, "/cancel")).await.unwrap();
    let r = handle_update(&pool, msg(100, "/questionnaires")).await.unwrap();
    assert!(texts(&r).contains("1 of 20") && buttons(&r)[0] == format!("q:start:{id}"));
    assert!(texts(&handle_update(&pool, tap(100, &format!("q:start:{id}"))).await.unwrap()).contains("Question 2 of 20"));

    // finish the rest
    let mut last = vec![];
    for idx in 1..20 {
        last = handle_update(&pool, tap(100, &format!("q:a:{id}:{idx}:3"))).await.unwrap();
    }
    let done = texts(&last);
    assert!(done.contains("finished this questionnaire"), "{done}");
    for score_word in ["12", "score", "neurotic", "extravert"] {
        assert!(!done.to_lowercase().contains(score_word), "the candidate was shown '{score_word}': {done}");
    }
    let p = MmRepository::get_profile(&pool, "p1").await.unwrap().unwrap();
    assert!(p.profile.get("big5_extraversion").is_some());
    assert!(texts(&handle_update(&pool, msg(100, "/questionnaires")).await.unwrap()).contains("no questionnaires waiting"));
    // /profile does not show the (sensitive) scores either
    assert!(!texts(&handle_update(&pool, msg(100, "/profile")).await.unwrap()).contains("Extraversion"));
}

#[tokio::test]
async fn questionnaire_buttons_are_checked_against_the_database() {
    let pool = test_pool().await;
    repo::ensure_builtin(&pool).await.unwrap();
    profile(&pool, "p1").await;
    profile(&pool, "p2").await;
    link_chat(&pool, "p1", 100).await;
    link_chat(&pool, "p2", 200).await;
    let mine = repo::assign(&pool, "p1", "mini_ipip", 1).await.unwrap();
    // another person cannot answer, start or even see someone else's questionnaire
    for data in [format!("q:a:{mine}:0:3"), format!("q:start:{mine}")] {
        let r = handle_update(&pool, tap(200, &data)).await.unwrap();
        assert!(texts(&r).contains("no longer open"), "{data}");
    }
    assert!(repo::get_response(&pool, mine).await.unwrap().unwrap().answers.is_empty());
    assert!(texts(&handle_update(&pool, msg(200, "/questionnaires")).await.unwrap()).contains("no questionnaires waiting"));
    // garbage data and unknown ids are harmless
    for data in ["q", "q:start", "q:start:abc", "q:a:1", "q:a:999:0:3", "q:zzz:1"] {
        let r = handle_update(&pool, tap(100, data)).await.unwrap();
        assert!(!texts(&r).is_empty(), "{data}");
    }
    // before consenting nothing works
    let id2 = repo::assign(&pool, "p2", "mini_ipip", 1).await.unwrap();
    sqlx::query("UPDATE mm_telegram_links SET consented_at = NULL WHERE profile_id = 'p2'").execute(&pool).await.unwrap();
    assert!(texts(&handle_update(&pool, tap(200, &format!("q:start:{id2}"))).await.unwrap()).contains("privacy notice"));
}
