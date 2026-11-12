use super::{notify, render, repo};
use crate::mm::{
    autonomy_repo::AutonomyRepo,
    match_repo::MatchRepo,
    repository::{MmRepository, ORG},
};
use matchmaking_core::{autonomy::Actor, FieldDef, FieldKind, MatchStatus, Value, NEVER_SHARED};
use sqlx::{Row, SqliteConnection, SqlitePool};

fn err(error: impl std::fmt::Display) -> String {
    error.to_string()
}

fn collect_value(value: &Value, protected: &mut Vec<String>) {
    match value {
        Value::Text(text) => protected.push(text.clone()),
        Value::Num(number) => protected.push(number.to_string()),
        Value::Bool(value) => protected.extend(
            if *value {
                ["yes", "true", "بله"]
            } else {
                ["no", "false", "خیر"]
            }
            .map(str::to_owned),
        ),
        Value::List(values) => protected.extend(values.iter().cloned()),
        Value::Records(rows) => {
            for row in rows {
                for value in row.values() {
                    collect_value(value, protected);
                }
            }
        }
    }
}

fn collect_field(
    key: &str,
    value: &Value,
    definition: Option<&FieldDef>,
    protected: &mut Vec<String>,
) {
    if NEVER_SHARED.contains(&key) || definition.is_none_or(|definition| definition.sensitive) {
        collect_value(value, protected);
    } else if let (
        Value::Records(rows),
        Some(FieldDef {
            kind: FieldKind::Records(fields),
            ..
        }),
    ) = (value, definition)
    {
        for row in rows {
            for (key, value) in row {
                collect_field(
                    key,
                    value,
                    fields.iter().find(|field| field.key == *key),
                    protected,
                );
            }
        }
    }
}

fn collect_scores(value: &serde_json::Value, protected: &mut Vec<String>) {
    match value {
        serde_json::Value::Number(number) => {
            protected.push(number.to_string());
            if let Some(number) = number.as_f64() {
                protected.push(number.to_string());
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                collect_scores(value, protected);
            }
        }
        serde_json::Value::Object(values) => {
            for value in values.values() {
                collect_scores(value, protected);
            }
        }
        _ => {}
    }
}

async fn screen_on(connection: &mut SqliteConnection, text: &str) -> Result<(), String> {
    let definitions: Vec<String> =
        sqlx::query_scalar("SELECT definition FROM mm_field_definitions WHERE org_id=?")
            .bind(ORG)
            .fetch_all(&mut *connection)
            .await
            .map_err(err)?;
    let definitions = definitions
        .iter()
        .map(|definition| serde_json::from_str::<FieldDef>(definition))
        .collect::<Result<Vec<_>, _>>()
        .map_err(err)?;
    let mut protected = Vec::new();
    for row in sqlx::query("SELECT l.chat_id,l.telegram_user_id,l.username FROM mm_telegram_links l JOIN mm_profiles p ON p.id=l.profile_id WHERE p.org_id=?")
        .bind(ORG).fetch_all(&mut *connection).await.map_err(err)? {
        protected.extend([row.get::<i64, _>("chat_id").to_string(), row.get::<i64, _>("telegram_user_id").to_string()]);
        if let Some(username) = row.get::<Option<String>, _>("username") {
            protected.push(username);
        }
    }
    for row in sqlx::query("SELECT id,name FROM mm_rule_sets WHERE org_id=?")
        .bind(ORG)
        .fetch_all(&mut *connection)
        .await
        .map_err(err)?
    {
        protected.extend([row.get::<String, _>("id"), row.get::<String, _>("name")]);
    }
    protected.extend(
        sqlx::query_scalar::<_, String>("SELECT id FROM mm_matches WHERE org_id=?")
            .bind(ORG)
            .fetch_all(&mut *connection)
            .await
            .map_err(err)?,
    );
    for row in sqlx::query("SELECT id,data FROM mm_profiles WHERE org_id=?")
        .bind(ORG)
        .fetch_all(&mut *connection)
        .await
        .map_err(err)?
    {
        protected.push(row.get::<String, _>("id"));
        let fields: std::collections::BTreeMap<String, matchmaking_core::profile::Entry> =
            serde_json::from_str(&row.get::<String, _>("data")).map_err(err)?;
        for (key, entry) in fields {
            collect_field(
                &key,
                &entry.value,
                definitions.iter().find(|definition| definition.key == key),
                &mut protected,
            );
        }
    }
    for definition in sqlx::query_scalar::<_, String>("SELECT v.definition FROM mm_rule_set_versions v JOIN mm_rule_sets s ON s.id=v.rule_set_id WHERE s.org_id=?")
        .bind(ORG).fetch_all(&mut *connection).await.map_err(err)? {
        let rules: matchmaking_core::RuleSet = serde_json::from_str(&definition).map_err(err)?;
        protected.push(rules.name);
        for rule in rules.rules {
            protected.extend([rule.id, rule.description]);
        }
    }
    for scorecard in sqlx::query_scalar::<_, String>("SELECT s.scorecard FROM mm_match_snapshots s JOIN mm_matches m ON m.id=s.match_id WHERE m.org_id=?")
        .bind(ORG).fetch_all(&mut *connection).await.map_err(err)? {
        collect_scores(&serde_json::from_str(&scorecard).map_err(err)?, &mut protected);
    }
    matchmaking_core::outbound::screen(text, &protected)
        .map_err(|_| "Outbound message blocked".into())
}

pub(crate) async fn eligible(
    pool: &SqlitePool,
    target: &str,
    recipient: &str,
    match_id: &str,
) -> Result<bool, String> {
    let Some(pair) = MatchRepo::get(pool, match_id).await.map_err(err)? else {
        return Ok(false);
    };
    if pair.hidden
        || pair.hold_reason.is_some()
        || !matches!(
            pair.status,
            MatchStatus::IntroductionProposed
                | MatchStatus::BothInterested
                | MatchStatus::ContactExchanged
                | MatchStatus::Conversation
                | MatchStatus::Meeting
                | MatchStatus::Feedback
        )
        || (pair.profile_a != target && pair.profile_b != target)
        || (pair.profile_a != recipient && pair.profile_b != recipient)
    {
        return Ok(false);
    }
    for profile_id in [&pair.profile_a, &pair.profile_b] {
        if !MmRepository::get_profile(pool, profile_id)
            .await
            .map_err(err)?
            .is_some_and(|profile| profile.status == "active")
        {
            return Ok(false);
        }
    }
    Ok(notify::reachable_link(pool, recipient).await?.is_some())
}

pub(crate) async fn check(
    pool: &SqlitePool,
    target: &str,
    recipient: &str,
    match_id: &str,
    text: &str,
) -> Result<(), String> {
    if !eligible(pool, target, recipient, match_id).await? {
        return Err("Message is not an eligible follow-up".into());
    }
    screen_on(&mut *pool.acquire().await.map_err(err)?, text).await
}

pub(crate) async fn queue(
    pool: &SqlitePool,
    actor: Actor,
    target: &str,
    recipient: &str,
    match_id: &str,
    text: &str,
) -> Result<(), String> {
    queue_message(pool, actor, target, recipient, Some(match_id), text, None).await
}

pub(crate) async fn info_field_on(
    connection: &mut SqliteConnection,
    target: &str,
    field: &str,
) -> Result<FieldDef, String> {
    if matchmaking_core::ai::never_for_ai(field) || NEVER_SHARED.contains(&field) {
        return Err("Field is not available to the agent".into());
    }
    let definition: Option<String> = sqlx::query_scalar("SELECT definition FROM mm_field_definitions WHERE org_id=? AND key=?")
        .bind(ORG).bind(field).fetch_optional(&mut *connection).await.map_err(err)?;
    let definition: FieldDef = serde_json::from_str(&definition.ok_or("Unknown field")?).map_err(err)?;
    if definition.sensitive || matches!(definition.kind, FieldKind::Records(_)) {
        return Err("Field is not available for an information request".into());
    }
    let data: Option<String> = sqlx::query_scalar("SELECT data FROM mm_profiles WHERE id=? AND org_id=? AND status='active'")
        .bind(target).bind(ORG).fetch_optional(&mut *connection).await.map_err(err)?;
    let fields: std::collections::BTreeMap<String, matchmaking_core::profile::Entry> =
        serde_json::from_str(&data.ok_or("Profile unavailable")?).map_err(err)?;
    if fields.contains_key(field) {
        return Err("Field is not missing".into());
    }
    Ok(definition)
}

pub(crate) async fn check_info(pool: &SqlitePool, target: &str, field: &str, text: &str) -> Result<(), String> {
    let link = notify::reachable_link(pool, target).await?.ok_or("Recipient unreachable")?;
    if repo::get_state(pool, link.chat_id).await.map_err(err)?.is_some() {
        return Err("Candidate already has an active question".into());
    }
    let mut connection = pool.acquire().await.map_err(err)?;
    info_field_on(&mut connection, target, field).await?;
    screen_on(&mut connection, text).await
}

pub(crate) async fn queue_info(pool: &SqlitePool, actor: Actor, target: &str, field: &str, text: &str) -> Result<(), String> {
    queue_message(pool, actor, target, target, None, text, Some(field)).await
}

async fn queue_message(
    pool: &SqlitePool,
    actor: Actor,
    target: &str,
    recipient: &str,
    match_id: Option<&str>,
    text: &str,
    field: Option<&str>,
) -> Result<(), String> {
    if !actor.is_agent() {
        return Err("Agent actor required".into());
    }
    if let Some(field) = field {
        check_info(pool, target, field, text).await?;
    } else {
        check(pool, target, recipient, match_id.ok_or("Match required")?, text).await?;
    }
    let policy = AutonomyRepo::load_policy(pool).await.map_err(err)?;
    let link = notify::reachable_link(pool, recipient)
        .await?
        .ok_or("Recipient unreachable")?;
    let mut transaction = pool.begin().await.map_err(err)?;
    let admitted = if field.is_some() {
        sqlx::query("UPDATE mm_profiles SET updated_at=updated_at WHERE id=? AND org_id=? AND status='active'")
            .bind(target).bind(ORG).execute(&mut *transaction).await.map_err(err)?
    } else {
        sqlx::query("UPDATE mm_matches SET updated_at=updated_at WHERE id=? AND org_id=? AND hidden=0 AND hold_reason IS NULL AND status IN ('introduction_proposed','both_interested','contact_exchanged','conversation','meeting','feedback') AND (profile_a=? OR profile_b=?) AND (profile_a=? OR profile_b=?) AND NOT EXISTS (SELECT 1 FROM mm_profiles WHERE id IN (mm_matches.profile_a,mm_matches.profile_b) AND status!='active')")
        .bind(match_id).bind(ORG).bind(target).bind(target).bind(recipient).bind(recipient)
        .execute(&mut *transaction).await.map_err(err)?
    };
    if admitted.rows_affected() != 1 {
        return Err("Follow-up changed".into());
    }
    let reachable: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM mm_telegram_links WHERE profile_id=? AND consent_version=? AND consented_at IS NOT NULL AND notifications_enabled=1 AND linked_at=?")
        .bind(recipient).bind(repo::CONSENT_VERSION).bind(&link.linked_at).fetch_one(&mut *transaction).await.map_err(err)?;
    if reachable != 1 {
        return Err("Recipient unreachable".into());
    }
    screen_on(&mut transaction, text).await?;
    if let Some(field) = field {
        info_field_on(&mut transaction, target, field).await?;
    }
    let now = chrono::Utc::now();
    let used: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM mm_agent_messages WHERE profile_id=? AND reserved_at>?",
    )
    .bind(recipient)
    .bind(now.timestamp() - 86400)
    .fetch_one(&mut *transaction)
    .await
    .map_err(err)?;
    if !policy.budgets.message_allowed(used as u64) {
        return Err("message_limit".into());
    }
    sqlx::query(
        "INSERT INTO mm_agent_messages(profile_id,match_id,actor,reserved_at) VALUES (?,?,?,?)",
    )
    .bind(recipient)
    .bind(match_id)
    .bind(actor.as_text())
    .bind(now.timestamp())
    .execute(&mut *transaction)
    .await
    .map_err(err)?;
    let outbox_id = repo::enqueue_on_as(
        &mut transaction,
        actor,
        recipient,
        if field.is_some() { "request_info" } else { "agent_message" },
        &render::agent_message_text(link.locale, text),
        None,
        match_id,
    )
    .await
    .map_err(err)?;
    if let Some(field) = field {
        let state = serde_json::json!({"flow":"request_info","field":field,"outbox_id":outbox_id,"profile_id":target,"linked_at":link.linked_at});
        sqlx::query("INSERT INTO mm_telegram_state(chat_id,state,updated_at) VALUES (?,?,?)")
            .bind(link.chat_id).bind(state.to_string()).bind(now.to_rfc3339()).execute(&mut *transaction).await.map_err(err)?;
    }
    sqlx::query("INSERT INTO mm_audit_log(org_id,actor,action,entity,entity_id,detail,at) VALUES (?,?,?,'profile',?,NULL,?)")
        .bind(ORG).bind(actor.as_text()).bind(if field.is_some() { "request_info" } else { "send_message" }).bind(recipient).bind(now.to_rfc3339()).execute(&mut *transaction).await.map_err(err)?;
    transaction.commit().await.map_err(err)?;
    Ok(())
}

pub(crate) async fn info_delivery_allowed(pool: &SqlitePool, recipient: &str, outbox_id: i64, rendered: &str) -> Result<bool, String> {
    let Some(link) = notify::reachable_link(pool, recipient).await? else { return Ok(false) };
    let Some(state) = repo::get_state(pool, link.chat_id).await.map_err(err)? else { return Ok(false) };
    if state["flow"] != "request_info" || state["outbox_id"].as_i64() != Some(outbox_id)
        || state["profile_id"] != recipient || state["linked_at"] != link.linked_at {
        return Ok(false);
    }
    let mut connection = pool.acquire().await.map_err(err)?;
    if info_field_on(&mut connection, recipient, state["field"].as_str().unwrap_or("")).await.is_err() {
        return Ok(false);
    }
    let text = rendered.trim_matches(['\u{2068}', '\u{2069}']).replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&");
    Ok(screen_on(&mut connection, &text).await.is_ok())
}

pub(crate) async fn clear_info(pool: &SqlitePool, outbox_id: i64) -> Result<(), String> {
    sqlx::query("DELETE FROM mm_telegram_state WHERE json_extract(state,'$.flow')='request_info' AND json_extract(state,'$.outbox_id')=?")
        .bind(outbox_id).execute(pool).await.map_err(err)?;
    Ok(())
}

pub(crate) async fn delivery_allowed(
    pool: &SqlitePool,
    recipient: &str,
    match_id: &str,
    rendered: &str,
) -> Result<bool, String> {
    if !eligible(pool, recipient, recipient, match_id).await? {
        return Ok(false);
    }
    let text = rendered
        .trim_matches(['\u{2068}', '\u{2069}'])
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&");
    Ok(screen_on(&mut *pool.acquire().await.map_err(err)?, &text)
        .await
        .is_ok())
}
