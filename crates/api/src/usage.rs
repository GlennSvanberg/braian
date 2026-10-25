//! Usage ingest and the task aggregate.
//!
//! `POST /v1/telemetry` only enqueues. It does not write Postgres and it does
//! not touch the change log. A full queue is dropped and the handler still
//! returns.

use std::time::Duration;

use axum::Json;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use braian_store::PrincipalKind;
use braian_telemetry::{ActorKind, Explicit, Observe, UsageEvent, canonical_task_type, enqueue};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::auth::{Auth, Identity, Role};
use crate::problem::Problem;
use crate::state::AppState;

pub const CLIENT_HEADER: &str = "x-braian-telemetry";
const MAX_EVENTS: usize = 100;
const NOTE: &str = "Work is measured per task, not per person.";

pub fn routes() -> axum::Router<AppState> {
    axum::Router::new()
        .route("/v1/telemetry", axum::routing::post(ingest))
        .route("/v1/usage", axum::routing::get(usage))
        .route(
            "/v1/usage/settings",
            axum::routing::get(get_settings).put(put_settings),
        )
}

pub fn client_reports(headers: &HeaderMap) -> bool {
    headers
        .get(CLIENT_HEADER)
        .and_then(|value| value.to_str().ok())
        == Some("client")
}

#[derive(Deserialize)]
struct IngestBody {
    #[serde(default)]
    events: Vec<Incoming>,
}

#[derive(Deserialize)]
struct Incoming {
    id: Uuid,
    occurred_at: DateTime<Utc>,
    actor_type: String,
    task_id: Uuid,
    task_type: String,
    session_id: Uuid,
    operation: String,
    #[serde(default)]
    object_ids: Vec<Uuid>,
    #[serde(default)]
    object_count: u32,
    result: String,
    #[serde(default)]
    duration_ms: Option<i64>,
    #[serde(default)]
    audit_batch_id: Option<Uuid>,
    #[serde(default)]
    audit_seq: Option<i64>,
    #[serde(default)]
    ambiguous: bool,
}

#[derive(Deserialize)]
struct DaysQuery {
    days: Option<i32>,
}

#[derive(Deserialize)]
struct SettingsBody {
    raw_retention_days: i32,
    inactivity_gap_seconds: i32,
}

pub struct OperationNote<'a> {
    pub name: &'a str,
    pub args: &'a Value,
    pub result: &'a str,
    pub outcome: &'a Value,
    pub elapsed: Duration,
    pub explicit: Option<Explicit>,
    pub actor_override: Option<ActorKind>,
    pub object_count: Option<u32>,
}

/// Records one registry operation for a caller that is not the browser.
/// Returns immediately. The change queue is not involved.
pub fn note_operation(state: &AppState, identity: &Identity, note: OperationNote<'_>) {
    if state.usage_tx.is_none() {
        return;
    }
    let actor = note.actor_override.unwrap_or(match identity.kind {
        PrincipalKind::Human => ActorKind::Human,
        PrincipalKind::Agent => ActorKind::Agent,
        PrincipalKind::Integration => ActorKind::Integration,
    });
    let ids = object_ids(note.args);
    let object_count = note
        .object_count
        .unwrap_or_else(|| u32::try_from(ids.len()).unwrap_or(0));
    let (audit_batch_id, audit_seq) = audit(note.outcome);
    let on_behalf = identity
        .actor
        .on_behalf_of
        .map(|id| id.as_uuid())
        .or_else(|| (actor == ActorKind::Agent).then_some(identity.actor.actor.as_uuid()));
    emit(
        state,
        Emit {
            principal: identity.actor.actor.as_uuid(),
            on_behalf_of: on_behalf,
            actor,
            operation: note.name,
            object_ids: &ids,
            object_count,
            result: note.result,
            elapsed: note.elapsed,
            explicit: note.explicit,
            audit_batch_id,
            audit_seq,
        },
    );
}

pub struct BulkNote<'a> {
    pub principal: Uuid,
    pub on_behalf_of: Option<Uuid>,
    pub kind: PrincipalKind,
    pub object_count: u32,
    pub result: &'a str,
    pub elapsed: Duration,
    pub audit_batch_id: Option<Uuid>,
}

pub fn note_bulk(state: &AppState, note: BulkNote<'_>) {
    if state.usage_tx.is_none() {
        return;
    }
    let actor = match note.kind {
        PrincipalKind::Human => ActorKind::Human,
        PrincipalKind::Agent => ActorKind::Agent,
        PrincipalKind::Integration => ActorKind::Integration,
    };
    emit(
        state,
        Emit {
            principal: note.principal,
            on_behalf_of: note
                .on_behalf_of
                .or_else(|| (actor == ActorKind::Agent).then_some(note.principal)),
            actor,
            operation: "bulk",
            object_ids: &[],
            object_count: note.object_count,
            result: note.result,
            elapsed: note.elapsed,
            explicit: Some(Explicit::Import),
            audit_batch_id: note.audit_batch_id,
            audit_seq: None,
        },
    );
}

struct Emit<'a> {
    principal: Uuid,
    on_behalf_of: Option<Uuid>,
    actor: ActorKind,
    operation: &'a str,
    object_ids: &'a [Uuid],
    object_count: u32,
    result: &'a str,
    elapsed: Duration,
    explicit: Option<Explicit>,
    audit_batch_id: Option<Uuid>,
    audit_seq: Option<i64>,
}

fn emit(state: &AppState, note: Emit<'_>) {
    let Emit {
        principal,
        on_behalf_of,
        actor,
        operation,
        object_ids,
        object_count,
        result,
        elapsed,
        explicit,
        audit_batch_id,
        audit_seq,
    } = note;
    let Some(operation) = clean_operation(operation) else {
        return;
    };
    let observe = Observe {
        at_ms: Utc::now().timestamp_millis(),
        operation: operation.clone(),
        actor,
        object_ids: object_ids.iter().map(ToString::to_string).collect(),
        explicit,
    };
    let assigned = {
        let mut assigner = state
            .usage_assign
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        assigner.observe(principal, &observe)
    };
    let event = UsageEvent {
        id: Uuid::now_v7(),
        occurred_at: Utc::now(),
        actor_type: actor.as_str().to_owned(),
        task_id: assigned.task_id,
        task_type: assigned.task_type,
        session_id: assigned.session_id,
        operation,
        object_ids: object_ids.iter().copied().take(64).collect(),
        object_count,
        result: clean_result(result),
        duration_ms: duration_ms(elapsed),
        audit_batch_id,
        audit_seq,
        actor_id: Some(principal),
        on_behalf_of,
        ambiguous: assigned.ambiguous,
    };
    let _ = enqueue(state.usage_tx.as_ref(), vec![event]);
}

async fn ingest(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Json(body): Json<IngestBody>,
) -> (StatusCode, Json<Value>) {
    let overflow = body.events.len().saturating_sub(MAX_EVENTS);
    let mut invalid = 0usize;
    let mut batch = Vec::new();
    for event in body.events.into_iter().take(MAX_EVENTS) {
        match accept(&identity, event) {
            Some(event) => batch.push(event),
            None => invalid += 1,
        }
    }
    let queued = batch.len();
    let queue_dropped = enqueue(state.usage_tx.as_ref(), batch);
    let accepted = queued.saturating_sub(queue_dropped);
    let dropped = overflow + invalid + queue_dropped;
    (
        StatusCode::ACCEPTED,
        Json(json!({ "accepted": accepted, "dropped": dropped })),
    )
}

fn accept(identity: &Identity, event: Incoming) -> Option<UsageEvent> {
    let operation = clean_operation(&event.operation)?;
    let actor = match identity.kind {
        PrincipalKind::Agent => ActorKind::Agent,
        PrincipalKind::Integration => ActorKind::Integration,
        PrincipalKind::Human => ActorKind::parse(&event.actor_type).unwrap_or(ActorKind::Human),
    };
    let actor = match actor {
        ActorKind::Integration if identity.kind == PrincipalKind::Human => ActorKind::Human,
        other => other,
    };
    let on_behalf = identity
        .actor
        .on_behalf_of
        .map(|id| id.as_uuid())
        .or_else(|| (actor == ActorKind::Agent).then_some(identity.actor.actor.as_uuid()));
    let mut object_ids = event.object_ids;
    object_ids.truncate(64);
    let object_count = event
        .object_count
        .max(u32::try_from(object_ids.len()).unwrap_or(0))
        .min(1_000_000_000);
    Some(UsageEvent {
        id: event.id,
        occurred_at: event.occurred_at,
        actor_type: actor.as_str().to_owned(),
        task_id: event.task_id,
        task_type: canonical_task_type(&event.task_type).to_owned(),
        session_id: event.session_id,
        operation,
        object_ids,
        object_count,
        result: clean_result(&event.result),
        duration_ms: event.duration_ms.and_then(|value| {
            if (0..=86_400_000).contains(&value) {
                i32::try_from(value).ok()
            } else {
                None
            }
        }),
        audit_batch_id: event.audit_batch_id,
        audit_seq: event.audit_seq,
        actor_id: Some(identity.actor.actor.as_uuid()),
        on_behalf_of: on_behalf,
        ambiguous: event.ambiguous,
    })
}

async fn usage(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Query(query): Query<DaysQuery>,
) -> Result<Json<Value>, Problem> {
    require(&identity, Role::Viewer)?;
    let days = query.days.unwrap_or(30).clamp(1, 366);
    let settings = state.store.usage_settings().await.map_err(store_problem)?;
    let summary = state
        .store
        .usage_summary(days)
        .await
        .map_err(store_problem)?;
    let mut human = 0i64;
    let mut agent = 0i64;
    let mut integration = 0i64;
    let by_type = summary
        .by_type
        .iter()
        .map(|row| {
            human += row.changes_human;
            agent += row.changes_agent;
            integration += row.changes_integration;
            json!({
                "task_type": row.task_type,
                "tasks": row.tasks,
                "human_time_ms": row.human_time_ms,
                "changes_human": row.changes_human,
                "changes_agent": row.changes_agent,
                "changes_integration": row.changes_integration,
            })
        })
        .collect::<Vec<_>>();
    let trend = summary
        .trend
        .iter()
        .map(|row| {
            json!({
                "day": row.day,
                "task_type": row.task_type,
                "tasks": row.tasks,
                "human_time_ms": row.human_time_ms,
            })
        })
        .collect::<Vec<_>>();
    Ok(Json(json!({
        "note": NOTE,
        "days": days,
        "retention_days": settings.raw_retention_days,
        "inactivity_gap_seconds": settings.inactivity_gap_seconds,
        "by_type": by_type,
        "share": { "human": human, "agent": agent, "integration": integration },
        "trend": trend,
    })))
}

async fn get_settings(
    State(state): State<AppState>,
    Auth(identity): Auth,
) -> Result<Json<Value>, Problem> {
    require(&identity, Role::Viewer)?;
    let settings = state.store.usage_settings().await.map_err(store_problem)?;
    Ok(Json(json!({
        "note": NOTE,
        "raw_retention_days": settings.raw_retention_days,
        "inactivity_gap_seconds": settings.inactivity_gap_seconds,
    })))
}

async fn put_settings(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Json(body): Json<SettingsBody>,
) -> Result<Json<Value>, Problem> {
    require(&identity, Role::Editor)?;
    if !(1..=3650).contains(&body.raw_retention_days)
        || !(30..=86_400).contains(&body.inactivity_gap_seconds)
    {
        return Err(Problem::new(
            StatusCode::BAD_REQUEST,
            "invalid_settings",
            "retention is 1 to 3650 days and the inactivity gap is 30 to 86400 seconds",
        ));
    }
    let settings = braian_store::UsageSettings {
        raw_retention_days: body.raw_retention_days,
        inactivity_gap_seconds: body.inactivity_gap_seconds,
    };
    state
        .store
        .put_usage_settings(&settings)
        .await
        .map_err(store_problem)?;
    if let Ok(mut assigner) = state.usage_assign.lock() {
        assigner.set_gap(i64::from(settings.inactivity_gap_seconds) * 1000);
    }
    Ok(Json(json!({
        "note": NOTE,
        "raw_retention_days": settings.raw_retention_days,
        "inactivity_gap_seconds": settings.inactivity_gap_seconds,
    })))
}

fn require(identity: &Identity, role: Role) -> Result<(), Problem> {
    if identity.role.allows(role) {
        Ok(())
    } else {
        Err(Problem::new(
            StatusCode::FORBIDDEN,
            "forbidden",
            "the role does not allow this",
        ))
    }
}

fn store_problem(error: braian_store::Error) -> Problem {
    crate::problem::store_status(error)
}

fn clean_operation(value: &str) -> Option<String> {
    let ok = !value.is_empty()
        && value.len() <= 64
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric()
                || character == '.'
                || character == '_'
                || character == '-'
        })
        && value
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_alphabetic());
    ok.then(|| value.to_owned())
}

fn clean_result(value: &str) -> String {
    match value {
        "ok" | "error" | "cancelled" => value.to_owned(),
        _ => "error".to_owned(),
    }
}

fn duration_ms(elapsed: Duration) -> Option<i32> {
    let ms = i32::try_from(elapsed.as_millis()).ok()?;
    (ms >= 0).then_some(ms)
}

fn object_ids(args: &Value) -> Vec<Uuid> {
    let mut out = Vec::new();
    let mut push = |value: &Value| {
        let Some(text) = value.as_str() else {
            return;
        };
        if let Ok(id) = Uuid::parse_str(text)
            && !out.contains(&id)
            && out.len() < 64
        {
            out.push(id);
        }
    };
    if let Some(value) = args.get("id") {
        push(value);
    }
    if let Some(value) = args.get("object_id") {
        push(value);
    }
    for key in ["ids", "object_ids"] {
        if let Some(list) = args.get(key).and_then(Value::as_array) {
            for value in list {
                push(value);
            }
        }
    }
    out
}

fn audit(outcome: &Value) -> (Option<Uuid>, Option<i64>) {
    let batch = outcome
        .get("batch_id")
        .and_then(Value::as_str)
        .and_then(|text| Uuid::parse_str(text).ok());
    let seq = outcome.get("seq").and_then(Value::as_i64);
    (batch, seq)
}
