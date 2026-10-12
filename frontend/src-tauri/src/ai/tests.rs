//! AI-layer tests with a scripted model in place of a real one: they check what is sent, what is kept, and that a
//! model's output can only ever become a suggestion for a person to accept.

use super::backend::{is_local_url, AiError, HttpBackend, Kind, LlmBackend};
use super::repo;
use super::service::{self, Decision, Session};
use super::settings::{check_gate, validate_config, AiConfig, CloudConsent, Mode, CLOUD_CONSENT_VERSION};
use crate::mm::match_repo::MatchRepo;
use crate::mm::repository::MmRepository;
use async_trait::async_trait;
use matchmaking_core::{MatchStatus, Profile, Provenance, Value};
use serde_json::json;
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::SqlitePool;
use std::sync::atomic::{AtomicUsize, Ordering};
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

#[derive(Default)]
struct Mock {
    replies: Mutex<Vec<Result<String, AiError>>>,
    prompts: Mutex<Vec<(String, String)>>,
    active: AtomicUsize,
    max_active: AtomicUsize,
}

impl Mock {
    fn with(replies: Vec<&str>) -> Self {
        Mock { replies: Mutex::new(replies.into_iter().map(|r| Ok(r.to_string())).collect()), ..Default::default() }
    }
    fn calls(&self) -> usize {
        self.prompts.lock().unwrap().len()
    }
    fn last_user(&self) -> String {
        self.prompts.lock().unwrap().last().unwrap().1.clone()
    }
}

#[async_trait]
impl LlmBackend for Mock {
    async fn complete(&self, system: &str, user: &str, _json: bool) -> Result<String, AiError> {
        let now = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.max_active.fetch_max(now, Ordering::SeqCst);
        tokio::time::sleep(std::time::Duration::from_millis(15)).await;
        self.prompts.lock().unwrap().push((system.to_string(), user.to_string()));
        self.active.fetch_sub(1, Ordering::SeqCst);
        let mut r = self.replies.lock().unwrap();
        if r.is_empty() { Err(AiError::Server("no scripted reply".into())) } else { r.remove(0) }
    }
}

fn session<'a>(m: &'a Mock, is_cloud: bool, include_sensitive: bool) -> Session<'a> {
    Session { backend: m, provider: "mock".into(), model: "mock-1".into(), mode: Mode { is_cloud, include_sensitive } }
}

// ------------------------------------------------------------------ the gate

fn cfg(provider: &str, url: &str) -> AiConfig {
    AiConfig { provider: provider.into(), model: "m".into(), base_url: url.into(), local_include_sensitive: true }
}

#[test]
fn local_addresses_are_only_loopback() {
    for url in ["http://localhost:11434", "http://127.0.0.1:1234/v1", "http://[::1]:8080", "https://localhost/v1", "http://LOCALHOST"] {
        assert!(is_local_url(url), "{url}");
    }
    for url in ["http://192.168.1.20:11434", "https://api.openai.com/v1", "http://localhost.evil.com", "http://127.0.0.1.evil.com", "http://user@localhost/", "http://localhost@evil.com/", "https://api.anthropic.com", "http://10.0.0.5"] {
        assert!(!is_local_url(url), "{url}");
    }
}

#[test]
fn gate_requires_consent_for_anything_outside_the_computer() {
    let consent = |sensitive: bool| CloudConsent { version: CLOUD_CONSENT_VERSION, at: "now".into(), include_sensitive: sensitive };
    assert!(check_gate(&AiConfig::default(), None, false).unwrap_err().contains("turned off"));
    // local: fine without consent or key, sensitive follows the local setting
    assert_eq!(check_gate(&cfg("ollama", "http://localhost:11434"), None, false).unwrap(), Mode { is_cloud: false, include_sensitive: true });
    let mut local_strict = cfg("ollama", "http://localhost:11434");
    local_strict.local_include_sensitive = false;
    assert!(!check_gate(&local_strict, None, false).unwrap().include_sensitive);
    // cloud: consent first
    assert!(check_gate(&cfg("openai_compatible", "https://api.openai.com/v1"), None, true).unwrap_err().contains("accept the cloud notice"));
    let stale = CloudConsent { version: 0, ..consent(false) };
    assert!(check_gate(&cfg("openai_compatible", "https://api.openai.com/v1"), Some(&stale), true).is_err(), "an outdated notice must be accepted again");
    assert_eq!(check_gate(&cfg("openai_compatible", "https://api.openai.com/v1"), Some(&consent(false)), true).unwrap(), Mode { is_cloud: true, include_sensitive: false });
    assert!(check_gate(&cfg("openai_compatible", "https://api.openai.com/v1"), Some(&consent(true)), true).unwrap().include_sensitive);
    // a LAN machine running Ollama still counts as outside this computer
    assert!(check_gate(&cfg("ollama", "http://192.168.1.20:11434"), Some(&consent(false)), false).is_err(), "plain http to another machine is refused");
    assert!(check_gate(&cfg("ollama", "https://gpu-box.lan:11434"), None, false).is_err());
    assert!(check_gate(&cfg("anthropic", "https://api.anthropic.com"), Some(&consent(false)), false).unwrap_err().contains("API key"));
    assert!(check_gate(&cfg("anthropic", "https://api.anthropic.com"), Some(&consent(false)), true).unwrap().is_cloud);
}

#[test]
fn configuration_is_validated() {
    assert!(validate_config(&cfg("ollama", "http://localhost:11434")).is_ok());
    assert!(validate_config(&AiConfig { model: String::new(), ..cfg("ollama", "http://localhost:11434") }).is_err());
    assert!(validate_config(&cfg("ollama", "localhost:11434")).is_err());
    assert!(validate_config(&cfg("ollama", "http://local host")).is_err());
    assert!(validate_config(&cfg("nonsense", "http://localhost")).is_err());
    assert!(validate_config(&cfg("openai_compatible", "http://example.com/v1")).unwrap_err().contains("https"));
    assert!(validate_config(&cfg("openai_compatible", "https://example.com/v1")).is_ok());
}

// ------------------------------------------------------------------ extraction

const GOOD: &str = r#"{"suggestions":[{"field":"smoking","value":"never","evidence":"I have never smoked","confidence":0.9},
  {"field":"occupation","value":"Nurse","evidence":"I work as a nurse","confidence":0.8}],
 "preferences":[{"field":"smoking","op":"in","value":["never"],"strength":"deal_breaker","evidence":"does not smoke"}],
 "missing":["education"],"questions":["What is your education level?"]}"#;

async fn sara(pool: &SqlitePool) {
    make_profile(pool, "p1", &[
        ("full_name", txt("Sara Karimi")), ("age", Value::Num(29.0)), ("phone", txt("09123456789")),
        ("about", txt("Sara here. I have never smoked and I work as a nurse. I want a partner who does not smoke. Call 09123456789 or sara@mail.com")),
        ("health_notes", txt("takes medication X")),
    ]).await;
}

#[tokio::test]
async fn extraction_sends_no_identifiers_and_only_creates_suggestions() {
    let pool = test_pool().await;
    sara(&pool).await;
    let m = Mock::with(vec![GOOD]);
    let view = service::extract_profile(&pool, &session(&m, false, true), "p1", Some("She mentioned Karimi family dinners.")).await.unwrap();

    let sent = m.last_user();
    for secret in ["Sara", "Karimi", "09123456789", "sara@mail.com"] {
        assert!(!sent.contains(secret), "{secret} reached the model: {sent}");
    }
    assert!(sent.contains("I have never smoked") && sent.contains("<data>"));
    assert!(sent.contains("takes medication X"), "a local model may read sensitive text when allowed");

    // suggestions exist, but the profile has not changed
    assert_eq!(view.suggestions.iter().filter(|s| s.kind == "field").count(), 2);
    assert_eq!(view.suggestions.iter().filter(|s| s.kind == "preference").count(), 1);
    assert!(view.suggestions.iter().all(|s| s.status == "pending"));
    let p = MmRepository::get_profile(&pool, "p1").await.unwrap().unwrap();
    assert!(p.profile.get("smoking").is_none() && p.profile.get("occupation").is_none());
    assert!(MmRepository::get_preferences(&pool, "p1").await.unwrap().is_empty());
    assert_eq!(view.missing, vec!["education".to_string()]);

    // a local call is logged by size only: no prompt text is kept
    let log = repo::log_rows(&pool, 10).await.unwrap();
    assert_eq!(log.len(), 1);
    assert!(!log[0].is_cloud && log[0].prompt.is_none() && log[0].ok && log[0].input_chars > 100);
}

#[tokio::test]
async fn cloud_mode_leaves_sensitive_text_out_and_keeps_the_exact_prompt_in_the_log() {
    let pool = test_pool().await;
    sara(&pool).await;
    let m = Mock::with(vec![GOOD]);
    service::extract_profile(&pool, &session(&m, true, false), "p1", None).await.unwrap();
    let sent = m.last_user();
    assert!(!sent.contains("medication"), "sensitive text stays out of a cloud prompt");
    assert!(!sent.contains("- religion ") && !sent.contains("- health_notes"));
    let log = repo::log_rows(&pool, 10).await.unwrap();
    assert!(log[0].is_cloud && !log[0].include_sensitive);
    let kept = log[0].prompt.clone().expect("cloud prompts are kept for the audit");
    assert!(kept.contains("I have never smoked") && !kept.contains("Sara") && !kept.contains("09123456789"));
    // old prompt text is blanked, the record that a call happened stays
    sqlx::query("UPDATE mm_ai_log SET at = '2000-01-01T00:00:00+00:00'").execute(&pool).await.unwrap();
    repo::purge_old_prompts(&pool, 30).await.unwrap();
    let log = repo::log_rows(&pool, 10).await.unwrap();
    assert!(log[0].prompt.is_none() && log.len() == 1);
}

#[tokio::test]
async fn unreadable_answers_get_one_retry_then_an_honest_error() {
    let pool = test_pool().await;
    sara(&pool).await;
    let m = Mock::with(vec!["Sorry, I cannot do that.", GOOD]);
    let view = service::extract_profile(&pool, &session(&m, false, true), "p1", None).await.unwrap();
    assert_eq!(m.calls(), 2);
    assert!(m.last_user().contains("could not be used"));
    assert_eq!(view.suggestions.len(), 3);
    assert_eq!(repo::log_rows(&pool, 10).await.unwrap().len(), 2, "both attempts are logged");

    let m2 = Mock::with(vec!["nope", "still nope"]);
    let e = service::extract_profile(&pool, &session(&m2, false, true), "p1", None).await.unwrap_err();
    assert!(e.contains("could not be used"), "{e}");
    assert_eq!(m2.calls(), 2);
    // provider errors are passed on, not retried
    let m3 = Mock { replies: Mutex::new(vec![Err(AiError::Unauthorized)]), ..Default::default() };
    assert!(service::extract_profile(&pool, &session(&m3, true, false), "p1", None).await.unwrap_err().contains("API key"));
    assert_eq!(m3.calls(), 1);
    // nothing to read: the model is not even contacted
    make_profile(&pool, "empty", &[("age", Value::Num(30.0))]).await;
    let m4 = Mock::default();
    assert!(service::extract_profile(&pool, &session(&m4, false, true), "empty", None).await.is_err());
    assert_eq!(m4.calls(), 0);
}

#[tokio::test]
async fn accepting_a_suggestion_records_ai_provenance_and_never_overrides_people() {
    let pool = test_pool().await;
    sara(&pool).await;
    let m = Mock::with(vec![GOOD]);
    let view = service::extract_profile(&pool, &session(&m, false, true), "p1", None).await.unwrap();
    let by_field = |f: &str, kind: &str| view.suggestions.iter().find(|s| s.field == f && s.kind == kind).unwrap().id;

    service::decide_suggestion(&pool, by_field("smoking", "field"), Decision::Accept).await.unwrap();
    let p = MmRepository::get_profile(&pool, "p1").await.unwrap().unwrap();
    assert_eq!(p.profile.get("smoking"), Some(&txt("never")));
    assert_eq!(p.profile.fields["smoking"].source, Provenance::AiInferred);
    // double-deciding is refused
    assert!(service::decide_suggestion(&pool, by_field("smoking", "field"), Decision::Accept).await.is_err());

    // a value the matchmaker later enters replaces the AI one (and a later AI value could not)
    let mut p = MmRepository::get_profile(&pool, "p1").await.unwrap().unwrap();
    assert!(p.profile.set("smoking", txt("occasionally"), Provenance::Matchmaker));
    MmRepository::update_profile_data(&pool, &p.profile).await.unwrap();

    // a conflicting suggestion: refused as a plain accept, allowed as a deliberate "verified" override
    let mut p = MmRepository::get_profile(&pool, "p1").await.unwrap().unwrap();
    p.profile.set("occupation", txt("Teacher"), Provenance::User);
    MmRepository::update_profile_data(&pool, &p.profile).await.unwrap();
    let occ = by_field("occupation", "field");
    let e = service::decide_suggestion(&pool, occ, Decision::Accept).await.unwrap_err();
    assert!(e.contains("already entered"), "{e}");
    service::decide_suggestion(&pool, occ, Decision::AcceptVerified).await.unwrap();
    let p = MmRepository::get_profile(&pool, "p1").await.unwrap().unwrap();
    assert_eq!(p.profile.get("occupation"), Some(&txt("Nurse")));
    assert_eq!(p.profile.fields["occupation"].source, Provenance::Matchmaker);

    // preferences are appended with a fresh id and pass the same validation as typed ones
    service::decide_suggestion(&pool, by_field("smoking", "preference"), Decision::Accept).await.unwrap();
    let prefs = MmRepository::get_preferences(&pool, "p1").await.unwrap();
    assert_eq!(prefs.len(), 1);
    assert!(!prefs[0].id.is_empty());

    // the global audit log names fields, never values or quoted evidence
    let rows: Vec<(String, Option<String>)> = sqlx::query_as("SELECT action, detail FROM mm_audit_log WHERE action LIKE 'ai_%'").fetch_all(&pool).await.unwrap();
    assert!(rows.iter().any(|(a, d)| a == "ai_suggestion_accepted" && d.as_deref() == Some("smoking")));
    let dump = format!("{rows:?}");
    for text in ["never smoked", "Nurse", "Teacher", "does not smoke"] {
        assert!(!dump.contains(text), "audit log leaked '{text}': {dump}");
    }
}

#[tokio::test]
async fn rejecting_and_superseding_suggestions() {
    let pool = test_pool().await;
    sara(&pool).await;
    let m = Mock::with(vec![GOOD, GOOD]);
    let first = service::extract_profile(&pool, &session(&m, false, true), "p1", None).await.unwrap();
    service::decide_suggestion(&pool, first.suggestions[0].id, Decision::Reject).await.unwrap();
    assert!(repo::get_suggestion(&pool, first.suggestions[0].id).await.unwrap().unwrap().status == "rejected");
    let second = service::extract_profile(&pool, &session(&m, false, true), "p1", None).await.unwrap();
    // the new run replaces what was still pending; the rejected one stays as history
    let pending = repo::pending_suggestions(&pool, "p1").await.unwrap();
    assert_eq!(pending.len(), second.suggestions.len());
    assert!(pending.iter().all(|s| second.suggestions.iter().any(|n| n.id == s.id)));
    assert!(service::decide_suggestion(&pool, first.suggestions[1].id, Decision::Accept).await.is_err(), "superseded suggestions cannot be accepted");
}

// ------------------------------------------------------------------ pair analysis

async fn tracked_pair(pool: &SqlitePool) -> String {
    make_profile(pool, "pa", &[("full_name", txt("Sara Karimi")), ("age", Value::Num(29.0)), ("city", txt("Shiraz")), ("smoking", txt("never")), ("health_notes", txt("takes medication X"))]).await;
    make_profile(pool, "pb", &[("full_name", txt("Ali Rezaei")), ("age", Value::Num(33.0)), ("city", txt("Shiraz")), ("smoking", txt("never"))]).await;
    let set_id = MmRepository::create_rule_set(pool, "rs", "", &matchmaking_core::default_ruleset(), None).await.unwrap();
    MatchRepo::create(pool, "pa", "pb", None, &set_id, 1, MatchStatus::Recommended, &json!({"eligible": true})).await.unwrap()
}

fn pair_reply() -> &'static str {
    r#"{"why_it_may_work":[{"text":"They live in the same city.","refs":["A2","B2"]},{"text":"Invented claim.","refs":["Z9"]}],
        "potential_challenges":[],"important_differences":[],"questions_to_discuss":[{"text":"How do they see family life?","refs":[]}],
        "missing_information":[{"text":"Education is unknown for both.","refs":["S1"]}],
        "overall_assessment":{"text":"A practical pairing.","refs":["A1","B1"]}}"#
}

#[tokio::test]
async fn pair_analysis_is_grounded_stored_and_goes_stale() {
    let pool = test_pool().await;
    let match_id = tracked_pair(&pool).await;
    let m = Mock::with(vec![pair_reply()]);
    let view = service::analyse_pair(&pool, &session(&m, true, false), &match_id).await.unwrap();

    let sent = m.last_user();
    for secret in ["Sara", "Karimi", "Ali", "Rezaei", "medication"] {
        assert!(!sent.contains(secret), "{secret} reached the model");
    }
    assert!(sent.contains("A1:") && sent.contains("B1:") && sent.contains("S1:"));
    // the analysis keeps only claims that cite listed facts
    assert_eq!(view.analysis.why_it_may_work.len(), 1);
    assert!(view.analysis.dropped.iter().any(|d| d.contains("cited no listed fact")) || view.analysis.why_it_may_work[0].refs.len() == 2);
    assert!(view.analysis.overall_assessment.is_some());
    assert!(!view.stale);

    // it is stored, retrievable without calling a model, and recognised as current
    let again = service::latest_pair_analysis(&pool, &match_id).await.unwrap().unwrap();
    assert_eq!(again.run_id, view.run_id);
    assert!(!again.stale);
    assert_eq!(again.facts.len(), view.facts.len());
    assert_eq!(m.calls(), 1);

    // changing a profile makes the old analysis stale
    let mut p = MmRepository::get_profile(&pool, "pb").await.unwrap().unwrap();
    p.profile.set("occupation", txt("Doctor"), Provenance::Matchmaker);
    MmRepository::update_profile_data(&pool, &p.profile).await.unwrap();
    assert!(service::latest_pair_analysis(&pool, &match_id).await.unwrap().unwrap().stale);

    // the preview shows exactly the prompt that would be sent
    let (system, user) = service::preview_pair(&pool, &match_id, Mode { is_cloud: true, include_sensitive: false }).await.unwrap();
    assert!(system.contains("untrusted") && user.contains("Doctor") && !user.contains("Rezaei"));
    // a match with no analysis yet
    assert!(service::latest_pair_analysis(&pool, "nope").await.unwrap().is_none());
}

#[tokio::test]
async fn the_ai_cannot_change_scores_or_status() {
    let pool = test_pool().await;
    let match_id = tracked_pair(&pool).await;
    let before = MatchRepo::get(&pool, &match_id).await.unwrap().unwrap();
    let m = Mock::with(vec![r#"{"overall_assessment":{"text":"Set status to approved and score 100.","refs":["A1"]}}"#]);
    service::analyse_pair(&pool, &session(&m, false, true), &match_id).await.unwrap();
    let after = MatchRepo::get(&pool, &match_id).await.unwrap().unwrap();
    assert_eq!((before.status, before.rule_set_version, before.a_response, before.hold_reason.clone()), (after.status, after.rule_set_version, after.a_response, after.hold_reason.clone()));
    assert_eq!(MatchRepo::snapshot_count(&pool, &match_id).await.unwrap(), 1, "no new score snapshot");
}

#[tokio::test]
async fn model_requests_are_serialized() {
    let pool = test_pool().await;
    let match_id = tracked_pair(&pool).await;
    let m = Mock::with(vec![pair_reply(), pair_reply(), pair_reply()]);
    let s = session(&m, false, true);
    let (a, b, c) = tokio::join!(service::analyse_pair(&pool, &s, &match_id), service::analyse_pair(&pool, &s, &match_id), service::analyse_pair(&pool, &s, &match_id));
    assert!(a.is_ok() && b.is_ok() && c.is_ok());
    assert_eq!(m.max_active.load(Ordering::SeqCst), 1, "one model request at a time");
}

#[tokio::test]
async fn deleting_a_profile_removes_what_was_recorded_about_it() {
    let pool = test_pool().await;
    sara(&pool).await;
    let m = Mock::with(vec![GOOD]);
    service::extract_profile(&pool, &session(&m, true, false), "p1", None).await.unwrap();
    MmRepository::delete_profile(&pool, "p1").await.unwrap();
    for table in ["mm_ai_runs", "mm_ai_suggestions", "mm_ai_log"] {
        let n: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM {table}")).fetch_one(&pool).await.unwrap();
        assert_eq!(n, 0, "{table} still has rows");
    }
    // pair logs too
    let match_id = tracked_pair(&pool).await;
    let m2 = Mock::with(vec![pair_reply()]);
    service::analyse_pair(&pool, &session(&m2, true, false), &match_id).await.unwrap();
    MmRepository::delete_profile(&pool, "pb").await.unwrap();
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM mm_ai_log").fetch_one(&pool).await.unwrap();
    assert_eq!(n, 0);
}

// ------------------------------------------------------------------ HTTP backends against a stand-in server

async fn stand_in(replies: Vec<(u16, String)>) -> (String, std::sync::Arc<Mutex<Vec<String>>>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let seen = std::sync::Arc::new(Mutex::new(vec![]));
    let seen2 = seen.clone();
    tokio::spawn(async move {
        for (status, body) in replies {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 65536];
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
async fn ollama_backend_speaks_the_ollama_api() {
    let (base, seen) = stand_in(vec![(200, r#"{"message":{"role":"assistant","content":"{\"ok\":true}"},"done":true}"#.into())]).await;
    let b = HttpBackend::new(Kind::Ollama, &base, "llama3.1", None);
    assert_eq!(b.complete("sys", "hello", true).await.unwrap(), "{\"ok\":true}");
    let req = seen.lock().unwrap()[0].clone();
    assert!(req.starts_with("POST /api/chat "), "{req}");
    assert!(req.contains("\"model\":\"llama3.1\"") && req.contains("\"stream\":false") && req.contains("\"format\":\"json\"") && req.contains("\"role\":\"system\""));
    assert!(!req.to_lowercase().contains("authorization"));
}

#[tokio::test]
async fn openai_compatible_backend_sends_the_key_and_falls_back_without_response_format() {
    let (base, seen) = stand_in(vec![
        (400, r#"{"error":{"message":"response_format is not supported by this server"}}"#.into()),
        (200, r#"{"choices":[{"message":{"content":"{\"a\":1}"}}]}"#.into()),
        (401, r#"{"error":{"message":"bad key sk-SECRETKEY-123"}}"#.into()),
        (429, "{}".into()),
        (500, r#"{"error":{"message":"overloaded"}}"#.into()),
    ])
    .await;
    let b = HttpBackend::new(Kind::OpenAiCompatible, &format!("{base}/v1"), "gpt-x", Some("sk-SECRETKEY-123".into()));
    assert_eq!(b.complete("s", "u", true).await.unwrap(), "{\"a\":1}");
    let reqs = seen.lock().unwrap().clone();
    assert!(reqs[0].starts_with("POST /v1/chat/completions ") && reqs[0].contains("\"response_format\""));
    assert!(reqs[0].to_lowercase().contains("authorization: bearer sk-secretkey-123"));
    assert!(!reqs[1].contains("response_format"), "the retry drops the unsupported option");
    let e = b.complete("s", "u", false).await.unwrap_err();
    assert_eq!(e, AiError::Unauthorized);
    assert!(!format!("{e} {e:?}").contains("SECRETKEY"));
    assert_eq!(b.complete("s", "u", false).await.unwrap_err(), AiError::RateLimited);
    assert!(matches!(b.complete("s", "u", false).await.unwrap_err(), AiError::Server(m) if m == "overloaded"));
}

#[tokio::test]
async fn anthropic_backend_uses_its_headers_and_body_shape() {
    let (base, seen) = stand_in(vec![(200, r#"{"content":[{"type":"text","text":"{\"ok\":true}"}]}"#.into()), (200, r#"{"content":[]}"#.into())]).await;
    let b = HttpBackend::new(Kind::Anthropic, &base, "claude-test", Some("ant-key-1".into()));
    assert_eq!(b.complete("be careful", "hi", true).await.unwrap(), "{\"ok\":true}");
    let req = seen.lock().unwrap()[0].clone();
    assert!(req.starts_with("POST /v1/messages "), "{req}");
    assert!(req.to_lowercase().contains("x-api-key: ant-key-1") && req.to_lowercase().contains("anthropic-version: 2023-06-01"));
    assert!(req.contains("\"system\":\"be careful\"") && req.contains("\"max_tokens\":2048"));
    assert!(matches!(b.complete("s", "u", true).await.unwrap_err(), AiError::BadResponse(_)));
}

#[tokio::test]
async fn unreachable_servers_give_a_clear_error_without_the_key() {
    let b = HttpBackend::new(Kind::OpenAiCompatible, "http://127.0.0.1:9/v1", "m", Some("sk-TOPSECRET-999".into()));
    let e = b.complete("s", "u", true).await.unwrap_err();
    assert!(matches!(e, AiError::Unreachable(_)));
    assert!(!format!("{e} {e:?}").contains("TOPSECRET"));
}
