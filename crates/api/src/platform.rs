//! Product surface on top of the object log: people, operations, workspaces,
//! search, jobs, proposals, history and the live stream.
//!
//! Buttons, the command palette, the in-app agent and the MCP endpoint call
//! [`execute`]. An agent principal never writes directly; the call becomes a
//! proposal the person can accept in full or in part.

use std::collections::{BTreeMap, BTreeSet};
use std::convert::Infallible;
use std::time::Duration;

use axum::Json;
use axum::body::{Body, Bytes};
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use braian_model::{AttributeId, AttributeKind, DefinitionSet, Object, ObjectId};
use braian_search::SearchQuery;
use braian_store::{Actor, BatchKind, HistoryEntry, ObjectOp, ProposalRow, Store};
use futures::stream;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::auth::{self, Auth, Identity, Role};
use crate::objects::CreateObject;
use crate::problem::{Problem, store_status};
use crate::state::AppState;
use crate::wire::{self, create_values, groups_to_wire, patch_from_wire, values_to_wire};

pub fn routes() -> axum::Router<AppState> {
    axum::Router::new()
        .route("/v1/login", post(login))
        .route("/v1/logout", post(logout))
        .route("/v1/tokens", post(create_token))
        .route("/v1/operations", get(list_operations))
        .route("/v1/operations/{name}", post(call_operation))
        .route("/mcp", post(mcp))
        .route("/v1/agent", post(agent))
        .route("/v1/stream", get(stream))
        .route("/v1/search", get(search))
        .route("/v1/objects", get(list_objects))
        .route("/v1/export", post(export_xlsx))
        .route("/v1/stats", get(stats))
        .route("/v1/workspaces", get(list_workspaces).post(save_workspace))
        .route("/v1/workspaces/{id}", get(get_workspace))
        .route(
            "/v1/workspaces/{id}/state",
            get(get_workspace_state)
                .put(put_workspace_state)
                .delete(delete_workspace_state),
        )
        .route("/v1/workspaces/{id}/assign", post(assign_workspace))
        .route("/v1/share/{token}", post(open_share))
        .route("/v1/views", post(save_view))
        .route(
            "/v1/view-types",
            get(list_view_types).post(register_view_type),
        )
        .route(
            "/v1/assortments",
            get(list_assortments).post(save_assortment),
        )
        .route("/v1/job-types", get(list_job_types).post(register_job_type))
        .route("/v1/job-types/{key}/runs", get(job_runs))
        .route("/v1/jobs/{id}", get(job_run))
        .route("/v1/bulk", post(bulk))
        .route("/v1/proposals", get(list_proposals).post(create_proposal))
        .route("/v1/proposals/{id}", get(get_proposal))
        .route("/v1/proposals/{id}/accept", post(accept_proposal))
        .route("/v1/proposals/{id}/discard", post(discard_proposal))
        .route("/v1/frecency", get(get_frecency).post(post_frecency))
        .route("/v1/history", get(history))
        .route("/v1/objects/{id}/as-of", get(as_of))
        .route("/v1/objects/{id}/rollback", post(rollback))
        .route("/v1/objects/{id}/graph", get(graph))
        .route("/v1/objects/{id}/relations", post(set_relation))
        .route("/v1/impact", post(impact))
        .route("/v1/webhooks", get(list_webhooks).post(create_webhook))
}

struct Operation {
    name: &'static str,
    title: &'static str,
    permission: Role,
    needs_approval: bool,
}

fn registry() -> &'static [Operation] {
    &[
        Operation {
            name: "search",
            title: "Search objects",
            permission: Role::Viewer,
            needs_approval: false,
        },
        Operation {
            name: "get_object",
            title: "Open object",
            permission: Role::Viewer,
            needs_approval: false,
        },
        Operation {
            name: "list_objects",
            title: "List objects",
            permission: Role::Viewer,
            needs_approval: false,
        },
        Operation {
            name: "create_object",
            title: "Create object",
            permission: Role::Editor,
            needs_approval: false,
        },
        Operation {
            name: "patch_object",
            title: "Edit object",
            permission: Role::Editor,
            needs_approval: false,
        },
        Operation {
            name: "mass_edit",
            title: "Mass edit",
            permission: Role::Editor,
            needs_approval: true,
        },
        Operation {
            name: "rollback",
            title: "Restore",
            permission: Role::Editor,
            needs_approval: true,
        },
        Operation {
            name: "accept_proposal",
            title: "Accept proposal",
            permission: Role::Editor,
            needs_approval: false,
        },
        Operation {
            name: "discard_proposal",
            title: "Discard proposal",
            permission: Role::Editor,
            needs_approval: false,
        },
        Operation {
            name: "history",
            title: "History",
            permission: Role::Viewer,
            needs_approval: false,
        },
        Operation {
            name: "impact",
            title: "Impact analysis",
            permission: Role::Viewer,
            needs_approval: false,
        },
        Operation {
            name: "set_relation",
            title: "Set relation",
            permission: Role::Editor,
            needs_approval: false,
        },
        Operation {
            name: "save_workspace",
            title: "Save workspace",
            permission: Role::Editor,
            needs_approval: false,
        },
        Operation {
            name: "export",
            title: "Export table",
            permission: Role::Viewer,
            needs_approval: false,
        },
        Operation {
            name: "ui.open_view",
            title: "Open view",
            permission: Role::Viewer,
            needs_approval: false,
        },
        Operation {
            name: "ui.open_template",
            title: "Open workspace template",
            permission: Role::Viewer,
            needs_approval: false,
        },
        Operation {
            name: "ui.open_workspace",
            title: "Open workspace",
            permission: Role::Viewer,
            needs_approval: false,
        },
        Operation {
            name: "ui.place",
            title: "Place view",
            permission: Role::Viewer,
            needs_approval: false,
        },
        Operation {
            name: "ui.link",
            title: "Link views",
            permission: Role::Viewer,
            needs_approval: false,
        },
        Operation {
            name: "ui.filter",
            title: "Filter table",
            permission: Role::Viewer,
            needs_approval: false,
        },
        Operation {
            name: "ui.group",
            title: "Group table",
            permission: Role::Viewer,
            needs_approval: false,
        },
        Operation {
            name: "ui.sort",
            title: "Sort table",
            permission: Role::Viewer,
            needs_approval: false,
        },
        Operation {
            name: "ui.select",
            title: "Select object",
            permission: Role::Viewer,
            needs_approval: false,
        },
        Operation {
            name: "ui.select_rows",
            title: "Select rows",
            permission: Role::Viewer,
            needs_approval: false,
        },
        Operation {
            name: "ui.show_column",
            title: "Show column",
            permission: Role::Viewer,
            needs_approval: false,
        },
        Operation {
            name: "ui.hide_column",
            title: "Hide column",
            permission: Role::Viewer,
            needs_approval: false,
        },
        Operation {
            name: "ui.move_column",
            title: "Move column",
            permission: Role::Viewer,
            needs_approval: false,
        },
        Operation {
            name: "ui.set_theme",
            title: "Set theme",
            permission: Role::Viewer,
            needs_approval: false,
        },
        Operation {
            name: "ui.export",
            title: "Export visible table",
            permission: Role::Viewer,
            needs_approval: false,
        },
        Operation {
            name: "open_view",
            title: "Open view",
            permission: Role::Viewer,
            needs_approval: false,
        },
        Operation {
            name: "move_view",
            title: "Move view",
            permission: Role::Viewer,
            needs_approval: false,
        },
        Operation {
            name: "split",
            title: "Split view",
            permission: Role::Viewer,
            needs_approval: false,
        },
        Operation {
            name: "resize",
            title: "Resize pane",
            permission: Role::Viewer,
            needs_approval: false,
        },
        Operation {
            name: "swap",
            title: "Swap views",
            permission: Role::Viewer,
            needs_approval: false,
        },
        Operation {
            name: "focus",
            title: "Focus pane",
            permission: Role::Viewer,
            needs_approval: false,
        },
        Operation {
            name: "move_view_left",
            title: "Move view left",
            permission: Role::Viewer,
            needs_approval: false,
        },
        Operation {
            name: "move_view_right",
            title: "Move view right",
            permission: Role::Viewer,
            needs_approval: false,
        },
        Operation {
            name: "move_view_up",
            title: "Move view up",
            permission: Role::Viewer,
            needs_approval: false,
        },
        Operation {
            name: "move_view_down",
            title: "Move view down",
            permission: Role::Viewer,
            needs_approval: false,
        },
        Operation {
            name: "zoom",
            title: "Maximize pane",
            permission: Role::Viewer,
            needs_approval: false,
        },
        Operation {
            name: "close_view",
            title: "Close view",
            permission: Role::Viewer,
            needs_approval: false,
        },
        Operation {
            name: "get_layout",
            title: "Get layout",
            permission: Role::Viewer,
            needs_approval: false,
        },
        Operation {
            name: "reset_layout",
            title: "Reset layout",
            permission: Role::Viewer,
            needs_approval: false,
        },
        Operation {
            name: "model.move_class",
            title: "Move class",
            permission: Role::Editor,
            needs_approval: true,
        },
        Operation {
            name: "model.add_attribute",
            title: "Add attribute",
            permission: Role::Editor,
            needs_approval: true,
        },
        Operation {
            name: "model.remove_attribute",
            title: "Remove attribute",
            permission: Role::Editor,
            needs_approval: true,
        },
        Operation {
            name: "model.reorder_attributes",
            title: "Reorder attributes",
            permission: Role::Editor,
            needs_approval: true,
        },
        Operation {
            name: "model.create_attribute",
            title: "Create attribute",
            permission: Role::Editor,
            needs_approval: true,
        },
        Operation {
            name: "model.create_class",
            title: "Create class",
            permission: Role::Editor,
            needs_approval: true,
        },
        Operation {
            name: "model.set_parent",
            title: "Set class parent",
            permission: Role::Editor,
            needs_approval: true,
        },
        Operation {
            name: "model.draft",
            title: "Draft data model",
            permission: Role::Editor,
            needs_approval: true,
        },
        Operation {
            name: "model.command",
            title: "Run a data model command",
            permission: Role::Editor,
            needs_approval: true,
        },
    ]
}

fn ui_command(name: &str, args: &Value) -> Value {
    json!({
        "ui": true,
        "name": name,
        "args": args,
        "applied": false,
    })
}

/// Layout commands are applied in the browser. `get_layout` is the exception:
/// it reads the saved document on the server.
fn is_layout_command(name: &str) -> bool {
    matches!(
        name,
        "open_view"
            | "move_view"
            | "split"
            | "resize"
            | "swap"
            | "focus"
            | "move_view_left"
            | "move_view_right"
            | "move_view_up"
            | "move_view_down"
            | "zoom"
            | "close_view"
            | "reset_layout"
    )
}

fn layout_owner(identity: &Identity) -> Result<Uuid, Problem> {
    identity.user_id.ok_or_else(|| {
        Problem::new(
            StatusCode::BAD_REQUEST,
            "no_user",
            "sign in to keep a layout",
        )
    })
}

fn layout_environment(raw: Option<&str>) -> String {
    let raw = raw.filter(|value| !value.is_empty()).unwrap_or("default");
    let cleaned: String = raw
        .chars()
        .filter(|character| {
            character.is_ascii_alphanumeric() || *character == '_' || *character == '-'
        })
        .take(64)
        .collect();
    if cleaned.is_empty() {
        "default".to_owned()
    } else {
        cleaned
    }
}

fn collect_layout_views(value: &Value, out: &mut Vec<String>) {
    let tabs = value
        .get("type")
        .and_then(Value::as_str)
        .is_some_and(|kind| kind == "tabs");
    if tabs {
        if let Some(views) = value.get("views").and_then(Value::as_array) {
            for id in views {
                if let Some(id) = id.as_str() {
                    out.push(id.to_owned());
                }
            }
        }
    } else if let Some(tabs) = value.get("tabs").and_then(Value::as_array) {
        for id in tabs {
            if let Some(id) = id.as_str() {
                out.push(id.to_owned());
            }
        }
    }
    if let Some(children) = value.get("children").and_then(Value::as_array) {
        for child in children {
            collect_layout_views(child, out);
        }
    }
    for key in ["first", "second"] {
        if let Some(child) = value.get(key) {
            collect_layout_views(child, out);
        }
    }
}

fn layout_snapshot(workspace: Uuid, document: &Value) -> Value {
    let layout = document
        .get("layout")
        .cloned()
        .unwrap_or_else(|| document.clone());
    let mut views = Vec::new();
    collect_layout_views(&layout, &mut views);
    json!({
        "workspace": workspace,
        "layout": layout,
        "focus": document.get("focus").cloned().unwrap_or(Value::Null),
        "zoom": document.get("zoom").cloned().unwrap_or(Value::Null),
        "views": views,
    })
}

fn find_op(name: &str) -> Option<&'static Operation> {
    registry()
        .iter()
        .find(|op| op.name == name || tool_name(op.name) == name)
}

/// OpenAI tool names allow letters, digits, underscore and hyphen. Dots in an
/// operation name become underscores. The mapping is stable: the same
/// operation always produces the same tool name, and the tool name resolves
/// back to that operation.
fn tool_name(operation: &str) -> String {
    let mut name = String::with_capacity(operation.len());
    for character in operation.chars() {
        if character.is_ascii_alphanumeric() || character == '_' || character == '-' {
            name.push(character);
        } else {
            name.push('_');
        }
    }
    if !name
        .chars()
        .next()
        .is_some_and(|character| character.is_ascii_alphabetic() || character == '_')
    {
        name.insert_str(0, "op_");
    }
    name
}

fn operation_for_tool(tool: &str) -> Option<&'static str> {
    find_op(tool).map(|op| op.name)
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

fn agent_must_propose(identity: &Identity, op: &Operation) -> bool {
    identity.kind == braian_store::PrincipalKind::Agent
        && matches!(op.name, "create_object" | "patch_object" | "set_relation")
}

async fn login(
    State(state): State<AppState>,
    Json(body): Json<LoginBody>,
) -> Result<Response, Problem> {
    let Some(user) = state
        .store
        .user_by_email(&body.email)
        .await
        .map_err(store_status)?
    else {
        return Err(Problem::unauthorized());
    };
    if !auth::verify_password(&user.password_hash, &body.password) {
        return Err(Problem::unauthorized());
    }
    let token = auth::random_token();
    let expires = chrono::Utc::now() + chrono::Duration::days(14);
    state
        .store
        .create_session(user.id, &auth::token_hash(&token), expires)
        .await
        .map_err(store_status)?;
    let mut response = Json(json!({
        "email": user.email,
        "role": user.role,
        "display_name": user.display_name,
        "user_id": user.id,
    }))
    .into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        auth::session_cookie(&token, state.cookie_secure)
            .parse()
            .unwrap_or_else(|_| header::HeaderValue::from_static("")),
    );
    Ok(response)
}

async fn logout(State(state): State<AppState>, headers: HeaderMap) -> Result<Response, Problem> {
    if let Some(token) = cookie_from(&headers, auth::SESSION_COOKIE) {
        state
            .store
            .delete_session(&auth::token_hash(&token))
            .await
            .map_err(store_status)?;
    }
    let mut response = StatusCode::NO_CONTENT.into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        auth::clear_session_cookie(state.cookie_secure)
            .parse()
            .unwrap_or_else(|_| header::HeaderValue::from_static("")),
    );
    Ok(response)
}

async fn create_token(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Json(body): Json<TokenBody>,
) -> Result<Json<Value>, Problem> {
    require(&identity, Role::Editor)?;
    let Some(user_id) = identity.user_id else {
        return Err(Problem::new(
            StatusCode::BAD_REQUEST,
            "no_user",
            "a token has to belong to a person; sign in with a user",
        ));
    };
    let Some(owner) = state
        .store
        .user_by_id(user_id)
        .await
        .map_err(store_status)?
    else {
        return Err(Problem::unauthorized());
    };
    let kind = if body.kind == "agent" { "agent" } else { "api" };
    let token = auth::random_token();
    state
        .store
        .insert_api_token(&owner, &body.name, kind, &auth::token_hash(&token))
        .await
        .map_err(store_status)?;
    Ok(Json(json!({
        "token": token,
        "kind": kind,
        "name": body.name,
        "owner": owner.email,
    })))
}

async fn list_operations(Auth(identity): Auth) -> Result<Json<Value>, Problem> {
    require(&identity, Role::Viewer)?;
    let ops = registry()
        .iter()
        .map(|op| {
            json!({
                "name": op.name,
                "title": op.title,
                "permission": op.permission.as_str(),
                "needs_approval": op.needs_approval || agent_must_propose(&identity, op),
            })
        })
        .collect::<Vec<_>>();
    Ok(Json(json!({ "operations": ops })))
}

async fn call_operation(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Path(name): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, Problem> {
    Ok(Json(execute(&state, &identity, &name, &body).await?))
}

async fn mcp(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Json(body): Json<Value>,
) -> Result<Json<Value>, Problem> {
    require(&identity, Role::Viewer)?;
    let id = body.get("id").cloned().unwrap_or(Value::Null);
    let method = body.get("method").and_then(Value::as_str).unwrap_or("");
    let result = match method {
        "initialize" => json!({
            "protocolVersion": "2025-06-18",
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "braian", "version": "0.0.0" }
        }),
        "tools/list" => json!({
            "tools": registry().iter().map(|op| json!({
                "name": tool_name(op.name),
                "description": op.title,
                "inputSchema": { "type": "object" }
            })).collect::<Vec<_>>()
        }),
        "tools/call" => {
            let params = body.get("params").cloned().unwrap_or(Value::Null);
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let name = operation_for_tool(name).unwrap_or(name);
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            match execute(&state, &identity, name, &args).await {
                Ok(value) => json!({
                    "content": [{ "type": "text", "text": value.to_string() }],
                    "structuredContent": value,
                    "isError": false
                }),
                Err(problem) => json!({
                    "content": [{ "type": "text", "text": problem.detail }],
                    "isError": true
                }),
            }
        }
        other => {
            return Ok(Json(json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": { "code": -32601, "message": format!("unknown method {other}") }
            })));
        }
    };
    Ok(Json(
        json!({ "jsonrpc": "2.0", "id": id, "result": result }),
    ))
}

async fn agent(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Json(body): Json<AgentBody>,
) -> Result<Response, Problem> {
    require(&identity, Role::Viewer)?;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    if body.stream {
        let state = state.clone();
        let identity = identity.clone();
        let messages = body.messages;
        tokio::spawn(async move {
            drive_agent(&state, &identity, messages, &tx).await;
        });
        let frames = stream::unfold(rx, |mut rx| async move {
            let event = rx.recv().await?;
            let frame = Bytes::from(format!("data: {event}\n\n"));
            Some((Ok::<Bytes, Infallible>(frame), rx))
        });
        return Ok(Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/event-stream")
            .header(header::CACHE_CONTROL, "no-cache")
            .header("x-accel-buffering", "no")
            .body(Body::from_stream(frames))
            .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response()));
    }
    drive_agent(&state, &identity, body.messages, &tx).await;
    drop(tx);
    let mut reply = String::new();
    let mut available = true;
    let mut tools = Vec::new();
    while let Ok(event) = rx.try_recv() {
        match event.get("type").and_then(Value::as_str) {
            Some("answer") => {
                reply = event
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned();
                available = event
                    .get("available")
                    .and_then(Value::as_bool)
                    .unwrap_or(true);
            }
            Some("error") => {
                reply = event
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned();
                available = false;
            }
            Some("tool") => tools.push(json!({
                "name": event.get("name").cloned().unwrap_or(Value::Null),
                "result": event.get("result").cloned().unwrap_or(Value::Null),
            })),
            _ => {}
        }
    }
    Ok(Json(json!({
        "available": available,
        "model": std::env::var("BRAIAN_AGENT_MODEL").unwrap_or_else(|_| "gpt-6-luna".into()),
        "reply": reply,
        "tools": tools
    }))
    .into_response())
}

async fn drive_agent(
    state: &AppState,
    identity: &Identity,
    incoming: Vec<Value>,
    tx: &tokio::sync::mpsc::UnboundedSender<Value>,
) {
    let key = std::env::var("OPENAI_API_KEY").unwrap_or_default();
    let model = std::env::var("BRAIAN_AGENT_MODEL").unwrap_or_else(|_| "gpt-6-luna".into());
    let unavailable =
        key.trim().is_empty() || std::env::var("BRAIAN_AGENT_MOCK").ok().as_deref() == Some("1");
    if unavailable {
        let _ = tx.send(json!({
            "type": "answer",
            "available": false,
            "text": "No model is connected. Set OPENAI_API_KEY to talk to the agent. Proposals, mass edit, and restore still work via operations."
        }));
        return;
    }
    let mut messages = vec![json!({
        "role": "system",
        "content": "You are the Braian agent. You always act for the signed-in person. Write changes must become proposals, not direct writes. Reply in English. The one-sentence answer and every explanation the person reads must be English."
    })];
    messages.extend(incoming);
    let client = reqwest::Client::new();
    let url = openai_responses_url();
    let payload = match post_openai(&client, &url, &key, &responses_body(&model, &messages)).await {
        Ok(payload) => payload,
        Err(problem) => {
            let _ = tx.send(json!({ "type": "error", "text": problem.detail }));
            return;
        }
    };
    let calls = function_calls(&payload);
    let mut outputs = Vec::new();
    for call in &calls {
        let operation = operation_for_tool(&call.name).unwrap_or(call.name.as_str());
        let args: Value = serde_json::from_str(&call.arguments).unwrap_or(json!({}));
        let step_id = if call.call_id.is_empty() {
            operation.to_owned()
        } else {
            call.call_id.clone()
        };
        let _ = tx.send(json!({
            "type": "step",
            "id": step_id,
            "op": operation,
            "args": args,
            "state": "run"
        }));
        let result = run_operation(state, identity, operation, &args, true)
            .await
            .unwrap_or_else(|problem| json!({ "error": problem.detail, "code": problem.code }));
        let _ = tx.send(json!({ "type": "tool", "name": operation, "result": result }));
        let proposed = result.get("proposal_id").is_some()
            || result.get("needs_approval").and_then(Value::as_bool) == Some(true)
            || result.get("id").is_some() && result.get("rows").is_some();
        if proposed {
            let _ = tx.send(json!({ "type": "proposal", "proposal": result }));
        }
        let _ = tx.send(json!({
            "type": "step",
            "id": step_id,
            "op": operation,
            "state": "done"
        }));
        if !call.call_id.is_empty() {
            outputs.push(json!({
                "type": "function_call_output",
                "call_id": call.call_id,
                "output": result.to_string(),
            }));
        }
    }
    let mut reply = reply_text(&payload);
    if !outputs.is_empty()
        && let Some(id) = payload.get("id").and_then(Value::as_str)
    {
        let follow_up = json!({
            "model": model,
            "previous_response_id": id,
            "input": outputs,
            "reasoning": { "effort": "none" },
        });
        match post_openai(&client, &url, &key, &follow_up).await {
            Ok(next) => {
                let text = reply_text(&next);
                if !text.is_empty() {
                    reply = text;
                }
            }
            Err(problem) => {
                tracing::error!(detail = %problem.detail, "openai follow-up failed");
                if reply.is_empty() {
                    reply = problem.detail;
                }
            }
        }
    }
    if reply.is_empty() {
        reply = if calls.is_empty() {
            "The model replied without text.".into()
        } else {
            "The agent ran tools but the model returned no text.".into()
        };
    }
    let _ = tx.send(json!({ "type": "answer", "text": reply, "available": true }));
}

fn openai_responses_url() -> String {
    let base = std::env::var("OPENAI_BASE_URL")
        .unwrap_or_else(|_| "https://api.openai.com".into())
        .trim_end_matches('/')
        .to_owned();
    format!("{base}/v1/responses")
}

fn responses_body(model: &str, messages: &[Value]) -> Value {
    let tools = registry()
        .iter()
        .map(|op| {
            json!({
                "type": "function",
                "name": tool_name(op.name),
                "description": op.title,
                "parameters": {
                    "type": "object",
                    "properties": {},
                    "additionalProperties": true
                }
            })
        })
        .collect::<Vec<_>>();
    json!({
        "model": model,
        "input": messages,
        "tools": tools,
        "reasoning": { "effort": "none" }
    })
}

struct FunctionCall {
    name: String,
    call_id: String,
    arguments: String,
}

fn function_calls(payload: &Value) -> Vec<FunctionCall> {
    let Some(output) = payload.get("output").and_then(Value::as_array) else {
        return Vec::new();
    };
    output
        .iter()
        .filter(|item| item.get("type").and_then(Value::as_str) == Some("function_call"))
        .map(|item| FunctionCall {
            name: item
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            call_id: item
                .get("call_id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            arguments: item
                .get("arguments")
                .and_then(Value::as_str)
                .unwrap_or("{}")
                .to_owned(),
        })
        .collect()
}

fn reply_text(payload: &Value) -> String {
    if let Some(text) = payload.get("output_text").and_then(Value::as_str)
        && !text.is_empty()
    {
        return text.to_owned();
    }
    let Some(output) = payload.get("output").and_then(Value::as_array) else {
        return String::new();
    };
    let mut parts = Vec::new();
    for item in output {
        let Some(content) = item.get("content").and_then(Value::as_array) else {
            continue;
        };
        for block in content {
            if let Some(text) = block.get("text").and_then(Value::as_str)
                && !text.is_empty()
            {
                parts.push(text);
            }
        }
    }
    parts.join("")
}

async fn post_openai(
    client: &reqwest::Client,
    url: &str,
    key: &str,
    body: &Value,
) -> Result<Value, Problem> {
    let response = client
        .post(url)
        .bearer_auth(key)
        .json(body)
        .send()
        .await
        .map_err(|error| {
            tracing::error!(%error, "openai request failed before a response");
            Problem::new(
                StatusCode::BAD_GATEWAY,
                "agent",
                "The model could not be reached. Check the network and try again.",
            )
        })?;
    let status = response.status();
    let text = response.text().await.map_err(|error| {
        tracing::error!(%error, "openai response body could not be read");
        Problem::new(
            StatusCode::BAD_GATEWAY,
            "agent",
            "The model replied but the response could not be read.",
        )
    })?;
    if !status.is_success() {
        let detail = openai_error(status, &text);
        tracing::error!(%status, body = %clip(&text, 500), "openai responses request failed");
        return Err(Problem::new(StatusCode::BAD_GATEWAY, "agent", detail));
    }
    serde_json::from_str(&text).map_err(|error| {
        tracing::error!(%error, body = %clip(&text, 500), "openai response was not json");
        Problem::new(
            StatusCode::BAD_GATEWAY,
            "agent",
            "The model replied with something that was not JSON.",
        )
    })
}

fn openai_error(status: StatusCode, body: &str) -> String {
    let message = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|value| {
            value
                .pointer("/error/message")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .filter(|message| !message.is_empty())
        .unwrap_or_else(|| "unknown error".into());
    format!(
        "The model did not reply. OpenAI ({status}) said: {}",
        clip(&message, 240)
    )
}

fn clip(text: &str, max: usize) -> String {
    let mut clipped = String::new();
    for character in text.chars().take(max) {
        clipped.push(character);
    }
    clipped
}

async fn stream(State(state): State<AppState>, Auth(identity): Auth) -> Result<Response, Problem> {
    require(&identity, Role::Viewer)?;
    // The body stream is only polled when a frame is ready. A broadcast
    // receiver parked inside that stream did not get woken, so the browser
    // stayed on the greeting comment. A task owns the receiver and pushes
    // frames into a channel the body forwards.
    let (frames, rx) = tokio::sync::mpsc::channel(64);
    let mut live = state.live.subscribe();
    tokio::spawn(async move {
        if frames.send(Bytes::from(": connected\n\n")).await.is_err() {
            return;
        }
        let mut tick = tokio::time::interval(Duration::from_secs(15));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            let frame = tokio::select! {
                biased;
                value = live.recv() => match value {
                    Ok(value) => Bytes::from(format!("data: {value}\n\n")),
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                        Bytes::from(format!(
                            "data: {{\"op\":\"lagged\",\"skipped\":{skipped}}}\n\n"
                        ))
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                },
                _ = tick.tick() => Bytes::from(": ping\n\n"),
            };
            if frames.send(frame).await.is_err() {
                break;
            }
        }
    });
    let body = stream::unfold(rx, |mut rx| async move {
        let frame = rx.recv().await?;
        Some((Ok::<Bytes, Infallible>(frame), rx))
    });
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-cache")
        .header("x-accel-buffering", "no")
        .body(Body::from_stream(body))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response()))
}

async fn search(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Query(query): Query<SearchParams>,
) -> Result<Json<Value>, Problem> {
    require(&identity, Role::Viewer)?;
    if let Some(seq) = query.wait_seq {
        wait_for_seq(&state.store, seq).await;
    }
    let Some(search) = &state.search else {
        return Err(Problem::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "search_unavailable",
            "TYPESENSE_URL is not configured",
        ));
    };
    let page = search
        .search(&SearchQuery {
            text: query.q.unwrap_or_default(),
            language: query.lang.unwrap_or_else(|| "sv".into()),
            filter_by: query.filter.unwrap_or_default(),
            sort_by: query.sort.unwrap_or_default(),
            group_by: query.group.unwrap_or_default(),
            page: query.page.unwrap_or(1),
            per_page: query.per_page.unwrap_or(50),
        })
        .await
        .map_err(|error| Problem::new(StatusCode::BAD_GATEWAY, "search", error.to_string()))?;
    Ok(Json(json!({
        "found": page.found,
        "hits": page.hits,
        "groups": page.groups,
        "indexed_seq": state.store.indexer_seq().await.map_err(store_status)?,
    })))
}

async fn list_objects(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Query(query): Query<ListParams>,
) -> Result<Json<Value>, Problem> {
    require(&identity, Role::Viewer)?;
    let limit = query.limit.unwrap_or(200).clamp(1, 2000);
    let heads = state
        .store
        .list_object_heads(limit)
        .await
        .map_err(store_status)?;
    let cached = state.models.get(&state.store).await.ok();
    let mut rows = Vec::new();
    for (id, key, type_id, version) in heads {
        let type_key = cached.as_ref().map(|cached| {
            cached
                .defs
                .types
                .iter()
                .find(|object_type| object_type.id.as_uuid() == type_id)
                .map(|object_type| object_type.key.clone())
                .unwrap_or_default()
        });
        if let Some(want) = &query.r#type
            && type_key.as_deref() != Some(want.as_str())
        {
            continue;
        }
        if let Some(q) = &query.q
            && !key.to_lowercase().contains(&q.to_lowercase())
        {
            continue;
        }
        rows.push(json!({
            "id": id,
            "key": key,
            "type": type_key,
            "version": version,
        }));
    }
    Ok(Json(json!({ "objects": rows })))
}

async fn export_xlsx(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Json(body): Json<ExportBody>,
) -> Result<Response, Problem> {
    require(&identity, Role::Viewer)?;
    let columns = if body.columns.is_empty() {
        vec!["key".into(), "type".into()]
    } else {
        body.columns
    };
    let mut records = Vec::new();
    if let Some(search) = &state.search {
        let page = search
            .search(&SearchQuery {
                text: body.q.unwrap_or_else(|| "*".into()),
                language: body.lang.unwrap_or_else(|| "sv".into()),
                filter_by: body.filter.unwrap_or_default(),
                sort_by: String::new(),
                group_by: String::new(),
                page: 1,
                per_page: 250,
            })
            .await
            .map_err(|error| Problem::new(StatusCode::BAD_GATEWAY, "search", error.to_string()))?;
        for hit in page.hits {
            records.push(hit.document);
        }
    } else {
        let listed = list_objects(
            State(state.clone()),
            Auth(identity.clone()),
            Query(ListParams {
                q: body.q,
                r#type: body.r#type,
                limit: Some(2000),
            }),
        )
        .await?;
        if let Some(rows) = listed.0.get("objects").and_then(Value::as_array) {
            records.extend(rows.iter().cloned());
        }
    }
    let bytes = rows_to_xlsx(&columns, &records)
        .map_err(|error| Problem::new(StatusCode::INTERNAL_SERVER_ERROR, "export", error))?;
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(
            header::CONTENT_TYPE,
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        )
        .header(
            header::CONTENT_DISPOSITION,
            "attachment; filename=\"braian.xlsx\"",
        )
        .body(Body::from(bytes))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response()))
}

fn rows_to_xlsx(columns: &[String], rows: &[Value]) -> Result<Vec<u8>, String> {
    let mut workbook = rust_xlsxwriter::Workbook::new();
    let sheet = workbook.add_worksheet();
    for (col, name) in columns.iter().enumerate() {
        sheet
            .write_string(0, col as u16, name)
            .map_err(|error| error.to_string())?;
    }
    for (row_index, row) in rows.iter().enumerate() {
        for (col, name) in columns.iter().enumerate() {
            let text = cell_text(row, name);
            sheet
                .write_string((row_index as u32) + 1, col as u16, &text)
                .map_err(|error| error.to_string())?;
        }
    }
    workbook.save_to_buffer().map_err(|error| error.to_string())
}

fn cell_text(row: &Value, name: &str) -> String {
    match row.get(name) {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    }
}

async fn stats(
    State(state): State<AppState>,
    Auth(identity): Auth,
) -> Result<Json<Value>, Problem> {
    require(&identity, Role::Viewer)?;
    let heads = state
        .store
        .list_object_heads(5000)
        .await
        .map_err(store_status)?;
    let cached = state.models.get(&state.store).await.ok();
    let mut counts: BTreeMap<String, i64> = BTreeMap::new();
    for (_, _, type_id, _) in &heads {
        let key = cached
            .as_ref()
            .and_then(|cached| {
                cached
                    .defs
                    .types
                    .iter()
                    .find(|object_type| object_type.id.as_uuid() == *type_id)
                    .map(|object_type| object_type.key.clone())
            })
            .unwrap_or_else(|| type_id.to_string());
        *counts.entry(key).or_default() += 1;
    }
    let by_type = counts
        .into_iter()
        .map(|(key, count)| json!({ "type": key, "count": count }))
        .collect::<Vec<_>>();
    Ok(Json(json!({ "objects": heads.len(), "by_type": by_type })))
}

async fn list_workspaces(
    State(state): State<AppState>,
    Auth(identity): Auth,
) -> Result<Json<Value>, Problem> {
    require(&identity, Role::Viewer)?;
    let rows = match identity.user_id {
        Some(id) => state
            .store
            .list_workspaces(id)
            .await
            .map_err(store_status)?,
        None => state
            .store
            .list_all_workspaces()
            .await
            .map_err(store_status)?,
    };
    Ok(Json(json!({ "workspaces": rows })))
}

async fn get_workspace(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, Problem> {
    require(&identity, Role::Viewer)?;
    state
        .store
        .workspace(id)
        .await
        .map_err(store_status)?
        .map(Json)
        .ok_or_else(|| Problem::new(StatusCode::NOT_FOUND, "not_found", "workspace"))
}

async fn save_workspace(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Json(body): Json<WorkspaceBody>,
) -> Result<Json<Value>, Problem> {
    require(&identity, Role::Editor)?;
    let Some(owner) = identity.user_id else {
        return Err(Problem::new(
            StatusCode::BAD_REQUEST,
            "no_user",
            "sign in to save a workspace",
        ));
    };
    let id = body.id.unwrap_or_else(Uuid::now_v7);
    if state.store.workspace_locked(id).await.unwrap_or(false) && identity.role != Role::Admin {
        return Err(Problem::new(
            StatusCode::FORBIDDEN,
            "locked",
            "the workspace can be used but its layout is locked",
        ));
    }
    state
        .store
        .save_workspace(
            id,
            owner,
            &body.name,
            body.locked.unwrap_or(false),
            body.share_token.as_deref(),
            &body.layout.unwrap_or(json!({})),
        )
        .await
        .map_err(store_status)?;
    Ok(Json(json!({ "id": id })))
}

const LAYOUT_DOCUMENT_LIMIT: usize = 512 * 1024;

async fn require_workspace(state: &AppState, id: Uuid) -> Result<(), Problem> {
    if state
        .store
        .workspace(id)
        .await
        .map_err(store_status)?
        .is_none()
    {
        return Err(Problem::new(
            StatusCode::NOT_FOUND,
            "not_found",
            "workspace",
        ));
    }
    Ok(())
}

async fn get_workspace_state(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Path(id): Path<Uuid>,
    Query(query): Query<StateQuery>,
) -> Result<Json<Value>, Problem> {
    require(&identity, Role::Viewer)?;
    require_workspace(&state, id).await?;
    let environment = layout_environment(query.environment.as_deref());
    let owner = layout_owner(&identity)?;
    let stored = state
        .store
        .workspace_state(owner, &environment, id)
        .await
        .map_err(store_status)?;
    let (document, updated_at) = match stored {
        Some((document, updated_at)) => (Some(document), Some(updated_at.to_rfc3339())),
        None => (None, None),
    };
    Ok(Json(json!({
        "environment": environment,
        "document": document,
        "updated_at": updated_at,
    })))
}

async fn put_workspace_state(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Path(id): Path<Uuid>,
    Json(body): Json<StateBody>,
) -> Result<Json<Value>, Problem> {
    require(&identity, Role::Editor)?;
    require_workspace(&state, id).await?;
    if !body.document.is_object() {
        return Err(Problem::new(
            StatusCode::BAD_REQUEST,
            "document",
            "the layout document must be an object",
        ));
    }
    let size = serde_json::to_vec(&body.document)
        .map(|bytes| bytes.len())
        .unwrap_or(usize::MAX);
    if size > LAYOUT_DOCUMENT_LIMIT {
        return Err(Problem::new(
            StatusCode::BAD_REQUEST,
            "document",
            "the layout document is too large",
        ));
    }
    let environment = layout_environment(body.environment.as_deref());
    let updated = state
        .store
        .put_workspace_state(layout_owner(&identity)?, &environment, id, &body.document)
        .await
        .map_err(store_status)?;
    Ok(Json(json!({
        "environment": environment,
        "updated_at": updated.to_rfc3339(),
    })))
}

async fn delete_workspace_state(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Path(id): Path<Uuid>,
    Query(query): Query<StateQuery>,
) -> Result<StatusCode, Problem> {
    require(&identity, Role::Editor)?;
    require_workspace(&state, id).await?;
    let environment = layout_environment(query.environment.as_deref());
    state
        .store
        .delete_workspace_state(layout_owner(&identity)?, &environment, id)
        .await
        .map_err(store_status)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn assign_workspace(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Path(id): Path<Uuid>,
    Json(body): Json<AssignBody>,
) -> Result<StatusCode, Problem> {
    require(&identity, Role::Admin)?;
    let user = state
        .store
        .user_by_email(&body.email)
        .await
        .map_err(store_status)?
        .ok_or_else(|| Problem::new(StatusCode::NOT_FOUND, "not_found", "user"))?;
    state
        .store
        .assign_workspace(id, user.id)
        .await
        .map_err(store_status)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn open_share(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Path(token): Path<String>,
) -> Result<Json<Value>, Problem> {
    require(&identity, Role::Viewer)?;
    let workspace = state
        .store
        .workspace_by_share(&token)
        .await
        .map_err(store_status)?
        .ok_or_else(|| Problem::new(StatusCode::NOT_FOUND, "not_found", "share"))?;
    if let Some(user_id) = identity.user_id
        && let Some(id) = workspace
            .get("id")
            .and_then(Value::as_str)
            .and_then(|id| Uuid::parse_str(id).ok())
    {
        state
            .store
            .assign_workspace(id, user_id)
            .await
            .map_err(store_status)?;
    }
    Ok(Json(workspace))
}

async fn save_view(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Json(body): Json<ViewBody>,
) -> Result<Json<Value>, Problem> {
    require(&identity, Role::Editor)?;
    if state
        .store
        .workspace_locked(body.workspace_id)
        .await
        .map_err(store_status)?
        && identity.role != Role::Admin
    {
        return Err(Problem::new(
            StatusCode::FORBIDDEN,
            "locked",
            "layout is locked",
        ));
    }
    let id = body.id.unwrap_or_else(Uuid::now_v7);
    state
        .store
        .save_view(
            id,
            body.workspace_id,
            &body.r#type,
            &body.title,
            &body.pane.unwrap_or_else(|| "main".into()),
            body.position.unwrap_or(0),
            &body.state.unwrap_or(json!({})),
        )
        .await
        .map_err(store_status)?;
    Ok(Json(json!({ "id": id })))
}

async fn list_view_types(
    State(state): State<AppState>,
    Auth(identity): Auth,
) -> Result<Json<Value>, Problem> {
    require(&identity, Role::Viewer)?;
    Ok(Json(
        json!({ "view_types": state.store.list_view_types().await.map_err(store_status)? }),
    ))
}

async fn register_view_type(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Json(body): Json<ViewTypeBody>,
) -> Result<StatusCode, Problem> {
    require(&identity, Role::Admin)?;
    state
        .store
        .register_view_type(&body.key, &body.title, body.embed_url.as_deref())
        .await
        .map_err(store_status)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_assortments(
    State(state): State<AppState>,
    Auth(identity): Auth,
) -> Result<Json<Value>, Problem> {
    require(&identity, Role::Viewer)?;
    Ok(Json(
        json!({ "assortments": state.store.list_assortments().await.map_err(store_status)? }),
    ))
}

async fn save_assortment(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Json(body): Json<AssortmentBody>,
) -> Result<Json<Value>, Problem> {
    require(&identity, Role::Editor)?;
    let Some(owner) = identity.user_id else {
        return Err(Problem::new(
            StatusCode::BAD_REQUEST,
            "no_user",
            "sign in to save an assortment",
        ));
    };
    let id = body.id.unwrap_or_else(Uuid::now_v7);
    state
        .store
        .save_assortment(id, owner, &body.name, &body.query)
        .await
        .map_err(store_status)?;
    Ok(Json(json!({ "id": id })))
}

async fn list_job_types(
    State(state): State<AppState>,
    Auth(identity): Auth,
) -> Result<Json<Value>, Problem> {
    require(&identity, Role::Viewer)?;
    Ok(Json(
        json!({ "job_types": state.store.list_job_types().await.map_err(store_status)? }),
    ))
}

async fn register_job_type(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Json(body): Json<JobTypeBody>,
) -> Result<StatusCode, Problem> {
    require(&identity, Role::Admin)?;
    state
        .store
        .register_job_type(
            &body.key,
            &body.title,
            &body.reads_fields,
            body.assortment_id,
        )
        .await
        .map_err(store_status)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn job_runs(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Path(key): Path<String>,
) -> Result<Json<Value>, Problem> {
    require(&identity, Role::Viewer)?;
    Ok(Json(
        json!({ "runs": state.store.job_runs(&key).await.map_err(store_status)? }),
    ))
}

async fn job_run(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, Problem> {
    require(&identity, Role::Viewer)?;
    state
        .store
        .job_run(id)
        .await
        .map_err(store_status)?
        .map(Json)
        .ok_or_else(|| Problem::new(StatusCode::NOT_FOUND, "not_found", "job"))
}

async fn bulk(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Json(body): Json<BulkBody>,
) -> Result<(StatusCode, Json<Value>), Problem> {
    require(&identity, Role::Editor)?;
    if identity.kind == braian_store::PrincipalKind::Agent {
        return Err(Problem::new(
            StatusCode::FORBIDDEN,
            "needs_approval",
            "an agent files a proposal instead of a bulk write",
        ));
    }
    state
        .store
        .register_job_type("bulk.write", "Bulk write", &[], None)
        .await
        .map_err(store_status)?;
    let id = Uuid::now_v7();
    let total = i32::try_from(body.objects.len()).unwrap_or(i32::MAX);
    state
        .store
        .create_job_run(
            id,
            "bulk.write",
            None,
            identity.actor.actor,
            identity.actor.on_behalf_of,
            total,
        )
        .await
        .map_err(store_status)?;
    let objects = body.objects;
    let actor = identity.actor.clone();
    let job_state = state.clone();
    tokio::spawn(async move {
        run_bulk(job_state, id, actor, objects).await;
    });
    Ok((
        StatusCode::ACCEPTED,
        Json(json!({ "id": id, "status": "running", "total": total })),
    ))
}

async fn run_bulk(state: AppState, id: Uuid, actor: Actor, objects: Vec<CreateObject>) {
    let mut errors = Vec::new();
    let mut done = 0i32;
    let mut actor = actor;
    actor.kind = BatchKind::Bulk;
    actor.source = "bulk".into();
    let batch = match state.store.open_batch(&actor).await {
        Ok(batch) => batch,
        Err(error) => {
            let _ = state
                .store
                .finish_job_run(id, "failed", 0, &error.to_string(), &json!([]))
                .await;
            return;
        }
    };
    for chunk in objects.chunks(25) {
        let mut ops = Vec::new();
        let cached = match state.models.get(&state.store).await {
            Ok(cached) => cached,
            Err(problem) => {
                errors.push(json!({ "message": problem.detail }));
                break;
            }
        };
        for object in chunk {
            match create_op(&cached, object) {
                Ok(op) => ops.push(op),
                Err(problem) => errors.push(json!({
                    "key": object.values.get("nummer").or_else(|| object.values.get("sku")).cloned().unwrap_or(Value::Null),
                    "message": problem.detail,
                    "code": problem.code,
                })),
            }
        }
        if !ops.is_empty() {
            match state.store.write_ops(batch, &actor, ops).await {
                Ok(receipt) => {
                    state.emit(json!({
                        "seq": receipt.last_seq,
                        "op": "bulk",
                        "object_id": Value::Null,
                        "version": Value::Null,
                        "type": ""
                    }));
                }
                Err(error) => errors.push(json!({ "message": error.to_string() })),
            }
        }
        done += i32::try_from(chunk.len()).unwrap_or(0);
        let _ = state
            .store
            .update_job_progress(
                id,
                done,
                &format!("{done} rows processed"),
                &Value::Array(errors.clone()),
            )
            .await;
    }
    let status = if errors.is_empty() {
        "succeeded"
    } else {
        "failed"
    };
    let _ = state
        .store
        .finish_job_run(
            id,
            status,
            done,
            &format!("{done} rows, {} errors", errors.len()),
            &Value::Array(errors),
        )
        .await;
}

fn create_op(cached: &crate::state::CachedModel, body: &CreateObject) -> Result<ObjectOp, Problem> {
    let type_id = wire::resolve_type(&cached.model, &body.object_type)?;
    let mut groups = BTreeMap::new();
    for (structure, group) in &body.groups {
        let structure = wire::resolve_structure(&cached.model, &cached.defs, structure)?;
        groups.insert(
            structure,
            wire::resolve_group(&cached.defs, structure, group)?,
        );
    }
    let object = Object {
        id: wire::object_id(body.id)?,
        type_id,
        model_revision: cached.revision,
        groups,
        values: create_values(&cached.model, &body.values)?,
    };
    let errors = cached.model.validate(&object);
    if !errors.is_empty() {
        return Err(Problem::validation(errors, Vec::new()));
    }
    Ok(ObjectOp::Create(object))
}

fn person_id(identity: &Identity) -> Uuid {
    identity
        .actor
        .on_behalf_of
        .unwrap_or(identity.actor.actor)
        .as_uuid()
}

async fn get_frecency(
    State(state): State<AppState>,
    Auth(identity): Auth,
) -> Result<Json<Value>, Problem> {
    require(&identity, Role::Viewer)?;
    Ok(Json(json!({
        "entries": state.store.frecency(person_id(&identity)).await.map_err(store_status)?
    })))
}

#[derive(Deserialize)]
struct FrecencyBody {
    id: String,
}

async fn post_frecency(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Json(body): Json<FrecencyBody>,
) -> Result<Json<Value>, Problem> {
    require(&identity, Role::Viewer)?;
    if body.id.is_empty() || body.id.len() > 200 {
        return Err(Problem::new(
            StatusCode::BAD_REQUEST,
            "frecency",
            "command id is missing",
        ));
    }
    Ok(Json(
        state
            .store
            .touch_frecency(person_id(&identity), &body.id)
            .await
            .map_err(store_status)?,
    ))
}

async fn list_proposals(
    State(state): State<AppState>,
    Auth(identity): Auth,
) -> Result<Json<Value>, Problem> {
    require(&identity, Role::Viewer)?;
    Ok(Json(json!({
        "proposals": state.store.list_proposals().await.map_err(store_status)?
    })))
}

async fn create_proposal(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Json(body): Json<ProposalBody>,
) -> Result<Json<Value>, Problem> {
    require(&identity, Role::Editor)?;
    let id = Uuid::now_v7();
    let rows = proposal_rows(&body.rows);
    state
        .store
        .create_proposal(
            id,
            &body.title,
            &body.source.unwrap_or_else(|| "api".into()),
            identity.actor.actor,
            identity.actor.on_behalf_of,
            rows.iter().any(|row| row.conflict),
            &rows,
        )
        .await
        .map_err(store_status)?;
    Ok(Json(
        state
            .store
            .proposal(id)
            .await
            .map_err(store_status)?
            .unwrap_or(json!({ "id": id })),
    ))
}

fn proposal_rows(rows: &[ProposalRowBody]) -> Vec<ProposalRow> {
    rows.iter()
        .enumerate()
        .map(|(index, row)| ProposalRow {
            id: row.id.unwrap_or_else(Uuid::now_v7),
            object_id: row.object_id,
            object_key: row.object_key.clone(),
            type_key: row.type_key.clone(),
            base_version: row.base_version,
            patch: row.patch.clone(),
            status: "pending".into(),
            conflict: row.conflict.unwrap_or(false),
            position: i32::try_from(index).unwrap_or(0),
        })
        .collect()
}

async fn get_proposal(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, Problem> {
    require(&identity, Role::Viewer)?;
    let mut body = state
        .store
        .proposal(id)
        .await
        .map_err(store_status)?
        .ok_or_else(|| Problem::new(StatusCode::NOT_FOUND, "not_found", "proposal"))?;
    crate::model_proposal::attach(&state, &mut body).await?;
    Ok(Json(body))
}

async fn accept_proposal(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Path(id): Path<Uuid>,
    Json(body): Json<AcceptBody>,
) -> Result<Json<Value>, Problem> {
    require(&identity, Role::Editor)?;
    if identity.kind == braian_store::PrincipalKind::Agent {
        return Err(Problem::new(
            StatusCode::FORBIDDEN,
            "agent_cannot_accept",
            "an agent proposes; a person accepts",
        ));
    }
    let proposal = state
        .store
        .proposal(id)
        .await
        .map_err(store_status)?
        .ok_or_else(|| Problem::new(StatusCode::NOT_FOUND, "not_found", "proposal"))?;
    let rows = proposal
        .get("rows")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut accepted = Vec::new();
    let mut failed = Vec::new();
    for row in rows {
        let row_id = row
            .get("id")
            .and_then(Value::as_str)
            .and_then(|id| Uuid::parse_str(id).ok());
        let Some(row_id) = row_id else { continue };
        if row.get("status").and_then(Value::as_str) != Some("pending") {
            continue;
        }
        if let Some(only) = &body.rows
            && !only.is_empty()
            && !only.contains(&row_id)
        {
            continue;
        }
        match apply_row(&state, &identity, &row).await {
            Ok(seq) => {
                state
                    .store
                    .set_proposal_rows(id, &[row_id], "accepted")
                    .await
                    .map_err(store_status)?;
                accepted.push(json!({ "id": row_id, "seq": seq }));
            }
            Err(problem) => {
                failed.push(json!({ "id": row_id, "code": problem.code, "detail": problem.detail }))
            }
        }
    }
    if failed.is_empty() {
        let _ = state.store.set_proposal_status(id, "accepted").await;
    }
    Ok(Json(json!({ "accepted": accepted, "failed": failed })))
}

async fn apply_row(state: &AppState, identity: &Identity, row: &Value) -> Result<i64, Problem> {
    if crate::model_proposal::is_model_row(row) {
        return crate::model_proposal::apply_row(state, identity, row).await;
    }
    let cached = state.models.get(&state.store).await?;
    let patch_body = row.get("patch").cloned().unwrap_or(json!({}));
    let values = object_map(patch_body.get("values"));
    let groups = optional_group_map(patch_body.get("groups"));
    let type_key = row.get("type_key").and_then(Value::as_str).unwrap_or("");
    if let Some(object_id) = row.get("object_id").and_then(Value::as_str) {
        let id = ObjectId::from_uuid(Uuid::parse_str(object_id).map_err(|error| {
            Problem::new(StatusCode::BAD_REQUEST, "invalid_id", error.to_string())
        })?);
        let stored = state
            .store
            .get_object(id)
            .await
            .map_err(store_status)?
            .ok_or_else(|| store_status(braian_store::Error::NotFound(id)))?;
        let patch = patch_from_wire(&cached.model, &cached.defs, &values, &groups)?;
        let report = cached.model.validate_patch(&stored.object, &patch);
        if !report.errors.is_empty() {
            return Err(Problem::validation(report.errors, report.warnings));
        }
        let base = row
            .get("base_version")
            .and_then(Value::as_i64)
            .unwrap_or(stored.version as i64) as u64;
        let receipt = state
            .store
            .write_batch(
                &identity.actor,
                vec![ObjectOp::Update {
                    id,
                    patch,
                    base_version: base,
                    read_version: stored.version,
                }],
            )
            .await
            .map_err(store_status)?;
        state.emit(json!({
            "seq": receipt.last_seq,
            "op": "update",
            "object_id": id.as_uuid(),
            "version": receipt.versions.first().map(|(_, version)| *version).unwrap_or(0),
            "type": type_key,
        }));
        state.index_after_commit(id.as_uuid(), receipt.last_seq);
        return Ok(receipt.last_seq);
    }
    let body = CreateObject {
        id: None,
        object_type: type_key.to_owned(),
        groups: groups
            .into_iter()
            .filter_map(|(key, value)| value.map(|group| (key, group)))
            .collect(),
        values,
    };
    let op = create_op(&cached, &body)?;
    let receipt = state
        .store
        .write_batch(&identity.actor, vec![op])
        .await
        .map_err(store_status)?;
    state.emit(json!({ "seq": receipt.last_seq, "op": "create", "type": type_key }));
    if let Some((id, _)) = receipt.versions.first() {
        state.index_after_commit(id.as_uuid(), receipt.last_seq);
    }
    Ok(receipt.last_seq)
}

async fn discard_proposal(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, Problem> {
    require(&identity, Role::Editor)?;
    state
        .store
        .set_proposal_rows(id, &[], "rejected")
        .await
        .map_err(store_status)?;
    state
        .store
        .set_proposal_status(id, "discarded")
        .await
        .map_err(store_status)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn history(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Query(query): Query<HistoryParams>,
) -> Result<Json<Value>, Problem> {
    require(&identity, Role::Viewer)?;
    let rows = state
        .store
        .history(query.object, None, query.limit.unwrap_or(100))
        .await
        .map_err(store_status)?;
    Ok(Json(
        json!({ "changes": rows.into_iter().map(history_json).collect::<Vec<_>>() }),
    ))
}

fn history_json(entry: HistoryEntry) -> Value {
    json!({
        "seq": entry.seq,
        "at": entry.at,
        "batch_id": entry.batch_id,
        "object_id": entry.object_id,
        "version": entry.version,
        "op": entry.op,
        "source": entry.source,
        "kind": entry.kind,
        "actor": entry.actor_handle.unwrap_or_else(|| entry.actor.to_string()),
        "on_behalf_of": entry.behalf_handle,
        "via": entry.kind,
        "diff": entry.diff,
    })
}

async fn as_of(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Path(id): Path<Uuid>,
    Query(query): Query<AsOfParams>,
) -> Result<Json<Value>, Problem> {
    require(&identity, Role::Viewer)?;
    let cached = state.models.get(&state.store).await?;
    let entries = state
        .store
        .history(Some(id), None, 10_000)
        .await
        .map_err(store_status)?;
    let snapshot = replay(&cached.defs, &entries, query.seq);
    Ok(Json(snapshot))
}

async fn rollback(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Path(id): Path<Uuid>,
    Json(body): Json<RollbackBody>,
) -> Result<Json<Value>, Problem> {
    require(&identity, Role::Editor)?;
    let cached = state.models.get(&state.store).await?;
    let stored = state
        .store
        .get_object(ObjectId::from_uuid(id))
        .await
        .map_err(store_status)?
        .ok_or_else(|| Problem::new(StatusCode::NOT_FOUND, "not_found", "object"))?;
    let entries = state
        .store
        .history(Some(id), None, 10_000)
        .await
        .map_err(store_status)?;
    let past = replay(&cached.defs, &entries, body.seq);
    let current = values_to_wire(
        &cached.model,
        &applicable(&cached, &stored.object),
        &stored.object.values,
        true,
    );
    let mut patch_values = BTreeMap::new();
    let mut keys = BTreeSet::new();
    keys.extend(current.keys().cloned());
    if let Some(values) = past.get("values").and_then(Value::as_object) {
        keys.extend(values.keys().cloned());
        for key in &keys {
            let before = values.get(key);
            let now = current.get(key);
            if before.map(|value| value == &Value::Null).unwrap_or(false) && now.is_none() {
                continue;
            }
            if before != now {
                patch_values.insert(key.clone(), before.cloned().unwrap_or(Value::Null));
            }
        }
    }
    let touched = fields_after(&entries, body.seq);
    let conflict = patch_values.keys().any(|key| {
        cached
            .model
            .attribute_by_key(key)
            .is_some_and(|attribute| touched.contains(&attribute.id.as_uuid().to_string()))
    });
    let row = ProposalRow {
        id: Uuid::now_v7(),
        object_id: Some(id),
        object_key: Some(stored.key.clone()),
        type_key: Some(wire::type_key(&cached.defs, stored.object.type_id)),
        base_version: Some(stored.version as i64),
        patch: json!({ "values": patch_values }),
        status: "pending".into(),
        conflict,
        position: 0,
    };
    let proposal_id = Uuid::now_v7();
    state
        .store
        .create_proposal(
            proposal_id,
            "Restore",
            "rollback",
            identity.actor.actor,
            identity.actor.on_behalf_of,
            conflict,
            &[row],
        )
        .await
        .map_err(store_status)?;
    Ok(Json(
        state
            .store
            .proposal(proposal_id)
            .await
            .map_err(store_status)?
            .unwrap_or(json!({ "id": proposal_id })),
    ))
}

fn applicable(cached: &crate::state::CachedModel, object: &Object) -> Vec<AttributeId> {
    cached
        .model
        .applicable_ids(object.type_id, &object.groups)
        .unwrap_or_default()
}

fn replay(defs: &DefinitionSet, entries: &[HistoryEntry], until: i64) -> Value {
    let mut ascending = entries.to_vec();
    ascending.sort_by_key(|entry| entry.seq);
    let mut values: Map<String, Value> = Map::new();
    let mut deleted = false;
    for entry in ascending.into_iter().filter(|entry| entry.seq <= until) {
        if entry.op == "delete" {
            deleted = true;
        }
        if entry.op == "create" {
            deleted = false;
        }
        let Some(diff) = entry.diff.as_ref().and_then(Value::as_object) else {
            continue;
        };
        for (key, change) in diff {
            if key == "$groups" {
                continue;
            }
            let after = change.get("after").cloned().unwrap_or(Value::Null);
            let attribute = Uuid::parse_str(key).ok().map(AttributeId::from_uuid);
            let localized = attribute
                .and_then(|id| defs.attributes.iter().find(|attribute| attribute.id == id))
                .is_some_and(|attribute| attribute.language_dependent);
            if localized && after.is_object() {
                let slot = values.entry(key.clone()).or_insert_with(|| json!({}));
                if !slot.is_object() {
                    *slot = json!({});
                }
                if let Some(map) = slot.as_object_mut()
                    && let Some(languages) = after.as_object()
                {
                    for (language, value) in languages {
                        if value.is_null() {
                            map.remove(language);
                        } else {
                            map.insert(language.clone(), value.clone());
                        }
                    }
                }
            } else if after.is_null() {
                values.remove(key);
            } else {
                values.insert(key.clone(), after);
            }
        }
    }
    let mut wire_values = Map::new();
    for (key, value) in values {
        let name = Uuid::parse_str(&key)
            .ok()
            .and_then(|id| {
                defs.attributes
                    .iter()
                    .find(|attribute| attribute.id.as_uuid() == id)
                    .map(|attribute| attribute.key.clone())
            })
            .unwrap_or(key);
        wire_values.insert(name, value);
    }
    json!({ "seq": until, "deleted": deleted, "values": wire_values })
}

fn fields_after(entries: &[HistoryEntry], seq: i64) -> BTreeSet<String> {
    let mut fields = BTreeSet::new();
    for entry in entries.iter().filter(|entry| entry.seq > seq) {
        let Some(diff) = entry.diff.as_ref().and_then(Value::as_object) else {
            continue;
        };
        for key in diff.keys() {
            if key != "$groups" {
                fields.insert(key.clone());
            }
        }
    }
    fields
}

async fn graph(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, Problem> {
    require(&identity, Role::Viewer)?;
    let cached = state.models.get(&state.store).await?;
    let stored = state
        .store
        .get_object(ObjectId::from_uuid(id))
        .await
        .map_err(store_status)?
        .ok_or_else(|| Problem::new(StatusCode::NOT_FOUND, "not_found", "object"))?;
    let relations = state
        .store
        .relations_for(ObjectId::from_uuid(id))
        .await
        .map_err(store_status)?;
    let entries = state
        .store
        .history(Some(id), None, 2000)
        .await
        .map_err(store_status)?;
    let mut attributes = Vec::new();
    for attribute_id in applicable(&cached, &stored.object) {
        let Some(attribute) = cached.model.attribute(attribute_id) else {
            continue;
        };
        let mut sources = Vec::new();
        let mut seen_classes = BTreeSet::new();
        for structure in &cached.defs.structures {
            for class in &structure.classes {
                let Some(group_id) = stored.object.groups.get(&structure.id) else {
                    continue;
                };
                if !covers(structure, class.group, *group_id) {
                    continue;
                }
                let Some((owner_key, inherited)) = attribute_owner(structure, class, attribute_id)
                else {
                    continue;
                };
                if !seen_classes.insert(owner_key.clone()) {
                    continue;
                }
                let group_key = structure
                    .groups
                    .iter()
                    .find(|group| group.id == *group_id)
                    .map(|group| group.key.clone())
                    .unwrap_or_default();
                sources.push(json!({
                    "structure": structure.key,
                    "class": owner_key,
                    "group": group_key,
                    "inherited": inherited,
                }));
            }
        }
        if cached.defs.types.iter().any(|object_type| {
            object_type.id == stored.object.type_id
                && object_type.attributes.contains(&attribute_id)
        }) {
            sources.insert(
                0,
                json!({ "from": "type", "structure": null, "class": null, "group": null }),
            );
        }
        if sources.is_empty() {
            sources
                .push(json!({ "structure": null, "class": null, "group": null, "from": "type" }));
        }
        let last = entries.iter().find(|entry| {
            entry
                .diff
                .as_ref()
                .and_then(Value::as_object)
                .is_some_and(|diff| diff.contains_key(&attribute_id.as_uuid().to_string()))
        });
        attributes.push(json!({
            "key": attribute.key,
            "label": english_label(&attribute.labels, &attribute.key),
            "sources": sources,
            "changed_at": last.map(|entry| entry.at.to_rfc3339()),
            "changed_by": last.and_then(|entry| entry.actor_handle.clone()),
            "seq": last.map(|entry| entry.seq),
        }));
    }
    Ok(Json(json!({
        "id": id,
        "key": stored.key,
        "type": wire::type_key(&cached.defs, stored.object.type_id),
        "groups": groups_to_wire(&cached.defs, &stored.object.groups),
        "relations": relations,
        "attributes": attributes,
    })))
}

fn english_label(labels: &BTreeMap<braian_model::LanguageTag, String>, key: &str) -> String {
    labels
        .iter()
        .find(|(tag, _)| tag.as_str() == "en")
        .map(|(_, label)| label.clone())
        .or_else(|| labels.values().next().cloned())
        .unwrap_or_else(|| key.to_string())
}

fn attribute_owner(
    structure: &braian_model::Structure,
    class: &braian_model::Class,
    attribute_id: AttributeId,
) -> Option<(String, bool)> {
    let mut current = Some(class);
    let mut inherited = false;
    let mut guard = 0;
    while let Some(item) = current {
        if item.attributes.contains(&attribute_id) {
            return Some((item.key.clone(), inherited));
        }
        inherited = true;
        current = item.parent.and_then(|id| {
            structure
                .classes
                .iter()
                .find(|candidate| candidate.id == id)
        });
        guard += 1;
        if guard > 32 {
            break;
        }
    }
    None
}

fn covers(
    structure: &braian_model::Structure,
    class_group: braian_model::GroupId,
    object_group: braian_model::GroupId,
) -> bool {
    let mut current = Some(object_group);
    let mut guard = 0;
    while let Some(group_id) = current {
        if group_id == class_group {
            return true;
        }
        current = structure
            .groups
            .iter()
            .find(|group| group.id == group_id)
            .and_then(|group| group.parent);
        guard += 1;
        if guard > 64 {
            break;
        }
    }
    false
}

async fn set_relation(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Path(id): Path<Uuid>,
    Json(body): Json<RelationBody>,
) -> Result<StatusCode, Problem> {
    require(&identity, Role::Editor)?;
    if identity.kind == braian_store::PrincipalKind::Agent {
        let row = ProposalRow {
            id: Uuid::now_v7(),
            object_id: Some(id),
            object_key: None,
            type_key: None,
            base_version: None,
            patch: json!({ "relation": { "to": body.to, "kind": body.kind } }),
            status: "pending".into(),
            conflict: false,
            position: 0,
        };
        state
            .store
            .create_proposal(
                Uuid::now_v7(),
                "Relation",
                "agent",
                identity.actor.actor,
                identity.actor.on_behalf_of,
                false,
                &[row],
            )
            .await
            .map_err(store_status)?;
        return Ok(StatusCode::ACCEPTED);
    }
    let kind = match body.kind.as_str() {
        "variant_of" | "article_of" | "related" => body.kind.as_str(),
        _ => "related",
    };
    state
        .store
        .set_relation(ObjectId::from_uuid(id), ObjectId::from_uuid(body.to), kind)
        .await
        .map_err(store_status)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn impact(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Json(body): Json<ImpactBody>,
) -> Result<Json<Value>, Problem> {
    require(&identity, Role::Viewer)?;
    let cached = state.models.get(&state.store).await?;
    let mut defs = cached.defs.clone();
    let mut field_keys = Vec::new();
    let mut attribute_ids = Vec::new();
    if let Some(key) = &body.remove_attribute {
        field_keys.push(key.clone());
        if let Some(attribute) = defs
            .attributes
            .iter()
            .find(|attribute| &attribute.key == key)
        {
            attribute_ids.push(attribute.id);
        }
        defs.attributes.retain(|attribute| &attribute.key != key);
        let removed = attribute_ids.clone();
        for object_type in &mut defs.types {
            object_type.attributes.retain(|id| !removed.contains(id));
        }
        for structure in &mut defs.structures {
            for class in &mut structure.classes {
                class.attributes.retain(|id| !removed.contains(id));
            }
        }
    }
    if let Some(key) = &body.remove_class {
        for structure in &mut defs.structures {
            if let Some(class) = structure.classes.iter().find(|class| &class.key == key) {
                for attribute_id in &class.attributes {
                    attribute_ids.push(*attribute_id);
                    if let Some(attribute) = defs
                        .attributes
                        .iter()
                        .find(|attribute| attribute.id == *attribute_id)
                    {
                        field_keys.push(attribute.key.clone());
                    }
                }
            }
            structure.classes.retain(|class| &class.key != key);
        }
    }
    if let Some(key) = &body.attribute {
        field_keys.push(key.clone());
        if let Some(attribute) = defs
            .attributes
            .iter_mut()
            .find(|attribute| &attribute.key == key)
        {
            attribute_ids.push(attribute.id);
            if let Some(kind) = body.kind.clone() {
                attribute.kind = kind;
            }
        }
    }
    defs.model_revision = defs.model_revision.saturating_add(1);
    let compiled = braian_rules::CompiledModel::compile(&defs);
    let mut affected = 0i64;
    let mut invalid = 0i64;
    let mut sampled = 0i64;
    let mut samples = Vec::new();
    for attribute_id in &attribute_ids {
        affected += state
            .store
            .count_attribute_usage(attribute_id.as_uuid())
            .await
            .map_err(store_status)?;
        if let Ok(model) = &compiled {
            let ids = state
                .store
                .objects_with_attribute(attribute_id.as_uuid(), 100)
                .await
                .map_err(store_status)?;
            for id in ids {
                sampled += 1;
                let Some(stored) = state.store.get_object(id).await.map_err(store_status)? else {
                    continue;
                };
                if samples.len() < 12 {
                    samples.push(json!({
                        "id": stored.object.id.as_uuid(),
                        "key": stored.key,
                    }));
                }
                let mut object = stored.object;
                object.model_revision = defs.model_revision;
                if body.remove_attribute.is_some() {
                    object.values.remove(attribute_id);
                }
                if !model.validate(&object).is_empty() {
                    invalid += 1;
                }
            }
        }
    }
    let jobs = if field_keys.is_empty() {
        Vec::new()
    } else {
        state
            .store
            .jobs_touching(&field_keys)
            .await
            .map_err(store_status)?
    };
    let definition_errors = compiled
        .err()
        .map(|errors| {
            errors
                .into_iter()
                .map(|error| error.message)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Ok(Json(json!({
        "affected_objects": affected,
        "sampled_objects": sampled,
        "samples": samples,
        "invalid_objects": invalid,
        "jobs": jobs,
        "definition_errors": definition_errors,
    })))
}

async fn list_webhooks(
    State(state): State<AppState>,
    Auth(identity): Auth,
) -> Result<Json<Value>, Problem> {
    require(&identity, Role::Admin)?;
    Ok(Json(
        json!({ "webhooks": state.store.list_webhooks().await.map_err(store_status)? }),
    ))
}

async fn create_webhook(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Json(body): Json<WebhookBody>,
) -> Result<Json<Value>, Problem> {
    require(&identity, Role::Admin)?;
    let id = Uuid::now_v7();
    let secret = body.secret.unwrap_or_else(auth::random_token);
    state
        .store
        .create_webhook(id, &body.url, &secret, body.assortment_id)
        .await
        .map_err(store_status)?;
    Ok(Json(json!({ "id": id, "secret": secret })))
}

pub async fn deliver_webhooks(store: &Store, client: &reqwest::Client, body: &Value) {
    let Ok(hooks) = store.enabled_webhooks().await else {
        return;
    };
    let payload = body.to_string();
    for (_id, url, secret, assortment) in hooks {
        if !assortment_allows(store, assortment, body).await {
            continue;
        }
        let signature = sign_webhook(&secret, &payload);
        let _ = client
            .post(url)
            .header("content-type", "application/json")
            .header("x-braian-signature", signature)
            .body(payload.clone())
            .send()
            .await;
    }
}

async fn assortment_allows(store: &Store, assortment: Option<Uuid>, body: &Value) -> bool {
    let Some(assortment) = assortment else {
        return true;
    };
    let Ok(Some(saved)) = store.assortment(assortment).await else {
        return false;
    };
    let Some(query) = saved.get("query") else {
        return true;
    };
    if body.get("object_id").and_then(Value::as_str).is_none() {
        return false;
    }
    if let Some(want) = query.get("type").and_then(Value::as_str)
        && body.get("type").and_then(Value::as_str) != Some(want)
    {
        return false;
    }
    true
}

fn sign_webhook(secret: &str, payload: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(secret.as_bytes());
    hasher.update(b".");
    hasher.update(payload.as_bytes());
    format!("sha256={}", hex::encode(hasher.finalize()))
}

async fn wait_for_seq(store: &Store, seq: i64) {
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(3) {
        if store.indexer_seq().await.unwrap_or(0) >= seq {
            return;
        }
        tokio::time::sleep(Duration::from_millis(40)).await;
    }
}

pub async fn execute(
    state: &AppState,
    identity: &Identity,
    name: &str,
    args: &Value,
) -> Result<Value, Problem> {
    run_operation(state, identity, name, args, false).await
}

async fn run_operation(
    state: &AppState,
    identity: &Identity,
    name: &str,
    args: &Value,
    from_agent: bool,
) -> Result<Value, Problem> {
    let Some(op) = find_op(name) else {
        return Err(Problem::new(
            StatusCode::NOT_FOUND,
            "unknown_operation",
            name.to_owned(),
        ));
    };
    require(identity, op.permission)?;
    if op.name.starts_with("ui.") || is_layout_command(op.name) {
        return Ok(ui_command(op.name, args));
    }
    if identity.kind == braian_store::PrincipalKind::Agent && name == "accept_proposal" {
        return Err(Problem::new(
            StatusCode::FORBIDDEN,
            "agent_cannot_accept",
            "an agent proposes; a person accepts",
        ));
    }
    let agent_write =
        from_agent && matches!(op.name, "create_object" | "patch_object" | "set_relation");
    if op.needs_approval || agent_must_propose(identity, op) || agent_write {
        return propose_operation(state, identity, name, args).await;
    }
    match name {
        "search" => {
            let params = SearchParams {
                q: args.get("q").and_then(Value::as_str).map(str::to_owned),
                lang: args.get("lang").and_then(Value::as_str).map(str::to_owned),
                filter: args
                    .get("filter")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                sort: args.get("sort").and_then(Value::as_str).map(str::to_owned),
                group: args.get("group").and_then(Value::as_str).map(str::to_owned),
                page: args
                    .get("page")
                    .and_then(Value::as_u64)
                    .map(|page| page as u32),
                per_page: args
                    .get("per_page")
                    .and_then(Value::as_u64)
                    .map(|page| page as u32),
                wait_seq: args.get("wait_seq").and_then(Value::as_i64),
            };
            Ok(
                search(State(state.clone()), Auth(identity.clone()), Query(params))
                    .await?
                    .0,
            )
        }
        "list_objects" => Ok(list_objects(
            State(state.clone()),
            Auth(identity.clone()),
            Query(ListParams {
                q: args.get("q").and_then(Value::as_str).map(str::to_owned),
                r#type: args.get("type").and_then(Value::as_str).map(str::to_owned),
                limit: args.get("limit").and_then(Value::as_i64),
            }),
        )
        .await?
        .0),
        "get_object" => {
            let id = args.get("id").and_then(Value::as_str).unwrap_or("");
            let id = ObjectId::from_uuid(Uuid::parse_str(id).map_err(|error| {
                Problem::new(StatusCode::BAD_REQUEST, "invalid_id", error.to_string())
            })?);
            let cached = state.models.get(&state.store).await?;
            let stored = state
                .store
                .get_object(id)
                .await
                .map_err(store_status)?
                .ok_or_else(|| store_status(braian_store::Error::NotFound(id)))?;
            let applicable = applicable(&cached, &stored.object);
            Ok(json!({
                "id": stored.object.id.as_uuid(),
                "type": wire::type_key(&cached.defs, stored.object.type_id),
                "key": stored.key,
                "version": stored.version,
                "groups": groups_to_wire(&cached.defs, &stored.object.groups),
                "values": values_to_wire(&cached.model, &applicable, &stored.object.values, false),
            }))
        }
        "create_object" => {
            let body: CreateObject = serde_json::from_value(args.clone()).map_err(|error| {
                Problem::new(StatusCode::BAD_REQUEST, "invalid_body", error.to_string())
            })?;
            let cached = state.models.get(&state.store).await?;
            let op = create_op(&cached, &body)?;
            let receipt = state
                .store
                .write_batch(&identity.actor, vec![op])
                .await
                .map_err(store_status)?;
            state
                .emit(json!({ "seq": receipt.last_seq, "op": "create", "type": body.object_type }));
            Ok(json!({ "seq": receipt.last_seq, "batch_id": receipt.batch_id.as_uuid() }))
        }
        "patch_object" => {
            let id = args.get("id").and_then(Value::as_str).unwrap_or("");
            let id = ObjectId::from_uuid(Uuid::parse_str(id).map_err(|error| {
                Problem::new(StatusCode::BAD_REQUEST, "invalid_id", error.to_string())
            })?);
            let cached = state.models.get(&state.store).await?;
            let stored = state
                .store
                .get_object(id)
                .await
                .map_err(store_status)?
                .ok_or_else(|| store_status(braian_store::Error::NotFound(id)))?;
            let values = object_map(args.get("values"));
            let groups = optional_group_map(args.get("groups"));
            let patch = patch_from_wire(&cached.model, &cached.defs, &values, &groups)?;
            let report = cached.model.validate_patch(&stored.object, &patch);
            if !report.errors.is_empty() {
                return Err(Problem::validation(report.errors, report.warnings));
            }
            let base = args
                .get("base_version")
                .and_then(Value::as_u64)
                .unwrap_or(stored.version);
            let receipt = state
                .store
                .write_batch(
                    &identity.actor,
                    vec![ObjectOp::Update {
                        id,
                        patch,
                        base_version: base,
                        read_version: stored.version,
                    }],
                )
                .await
                .map_err(store_status)?;
            state.emit(
                json!({ "seq": receipt.last_seq, "op": "update", "object_id": id.as_uuid() }),
            );
            state.index_after_commit(id.as_uuid(), receipt.last_seq);
            Ok(json!({ "seq": receipt.last_seq }))
        }
        "history" => Ok(history(
            State(state.clone()),
            Auth(identity.clone()),
            Query(HistoryParams {
                object: args
                    .get("object")
                    .and_then(Value::as_str)
                    .and_then(|id| Uuid::parse_str(id).ok()),
                limit: args.get("limit").and_then(Value::as_i64),
            }),
        )
        .await?
        .0),
        "impact" => {
            let body: ImpactBody = serde_json::from_value(args.clone()).map_err(|error| {
                Problem::new(StatusCode::BAD_REQUEST, "invalid_body", error.to_string())
            })?;
            Ok(
                impact(State(state.clone()), Auth(identity.clone()), Json(body))
                    .await?
                    .0,
            )
        }
        "export" => Ok(json!({ "hint": "POST /v1/export returns the workbook" })),
        "get_layout" => {
            let id = args
                .get("workspace")
                .and_then(Value::as_str)
                .and_then(|id| Uuid::parse_str(id).ok())
                .ok_or_else(|| {
                    Problem::new(
                        StatusCode::BAD_REQUEST,
                        "workspace",
                        "workspace id is required",
                    )
                })?;
            let environment = layout_environment(args.get("environment").and_then(Value::as_str));
            let owner = layout_owner(identity)?;
            if state
                .store
                .workspace(id)
                .await
                .map_err(store_status)?
                .is_none()
            {
                return Err(Problem::new(
                    StatusCode::NOT_FOUND,
                    "not_found",
                    "workspace",
                ));
            }
            if let Some((document, _)) = state
                .store
                .workspace_state(owner, &environment, id)
                .await
                .map_err(store_status)?
            {
                return Ok(layout_snapshot(id, &document));
            }
            let workspace = state
                .store
                .workspace(id)
                .await
                .map_err(store_status)?
                .ok_or_else(|| Problem::new(StatusCode::NOT_FOUND, "not_found", "workspace"))?;
            Ok(layout_snapshot(
                id,
                workspace.get("layout").unwrap_or(&Value::Null),
            ))
        }
        "save_workspace" => {
            let body: WorkspaceBody = serde_json::from_value(args.clone()).map_err(|error| {
                Problem::new(StatusCode::BAD_REQUEST, "invalid_body", error.to_string())
            })?;
            Ok(
                save_workspace(State(state.clone()), Auth(identity.clone()), Json(body))
                    .await?
                    .0,
            )
        }
        "set_relation" => {
            let id = args
                .get("id")
                .and_then(Value::as_str)
                .and_then(|id| Uuid::parse_str(id).ok())
                .unwrap_or_else(Uuid::nil);
            let body = RelationBody {
                to: args
                    .get("to")
                    .and_then(Value::as_str)
                    .and_then(|id| Uuid::parse_str(id).ok())
                    .unwrap_or_else(Uuid::nil),
                kind: args
                    .get("kind")
                    .and_then(Value::as_str)
                    .unwrap_or("related")
                    .to_owned(),
            };
            set_relation(
                State(state.clone()),
                Auth(identity.clone()),
                Path(id),
                Json(body),
            )
            .await?;
            Ok(json!({ "ok": true }))
        }
        "accept_proposal" => {
            let id = args
                .get("id")
                .and_then(Value::as_str)
                .and_then(|id| Uuid::parse_str(id).ok())
                .unwrap_or_else(Uuid::nil);
            let rows = args
                .get("rows")
                .and_then(Value::as_array)
                .map(|rows| {
                    rows.iter()
                        .filter_map(|row| row.as_str().and_then(|id| Uuid::parse_str(id).ok()))
                        .collect()
                })
                .unwrap_or_default();
            Ok(accept_proposal(
                State(state.clone()),
                Auth(identity.clone()),
                Path(id),
                Json(AcceptBody { rows: Some(rows) }),
            )
            .await?
            .0)
        }
        "discard_proposal" => {
            let id = args
                .get("id")
                .and_then(Value::as_str)
                .and_then(|id| Uuid::parse_str(id).ok())
                .unwrap_or_else(Uuid::nil);
            discard_proposal(State(state.clone()), Auth(identity.clone()), Path(id)).await?;
            Ok(json!({ "ok": true }))
        }
        other => Err(Problem::new(
            StatusCode::NOT_FOUND,
            "unknown_operation",
            other.to_owned(),
        )),
    }
}

async fn propose_operation(
    state: &AppState,
    identity: &Identity,
    name: &str,
    args: &Value,
) -> Result<Value, Problem> {
    match name {
        "mass_edit" => {
            let body: MassBody = serde_json::from_value(args.clone()).map_err(|error| {
                Problem::new(StatusCode::BAD_REQUEST, "invalid_body", error.to_string())
            })?;
            Ok(mass_edit(state, identity, body).await?)
        }
        "rollback" => {
            let id = args
                .get("id")
                .and_then(Value::as_str)
                .and_then(|id| Uuid::parse_str(id).ok())
                .unwrap_or_else(Uuid::nil);
            let seq = args.get("seq").and_then(Value::as_i64).unwrap_or(0);
            Ok(rollback(
                State(state.clone()),
                Auth(identity.clone()),
                Path(id),
                Json(RollbackBody { seq }),
            )
            .await?
            .0)
        }
        name if name.starts_with("model.") => {
            crate::model_proposal::propose(state, identity, name, args).await
        }
        "create_object" | "patch_object" | "set_relation" => {
            let id = Uuid::now_v7();
            let object_id = args
                .get("id")
                .and_then(Value::as_str)
                .and_then(|text| Uuid::parse_str(text).ok());
            let row = ProposalRow {
                id: Uuid::now_v7(),
                object_id,
                object_key: args
                    .get("values")
                    .and_then(|values| values.get("nummer"))
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                type_key: args.get("type").and_then(Value::as_str).map(str::to_owned),
                base_version: args.get("base_version").and_then(Value::as_i64),
                patch: args.clone(),
                status: "pending".into(),
                conflict: false,
                position: 0,
            };
            state
                .store
                .create_proposal(
                    id,
                    "Agent proposal",
                    "agent",
                    identity.actor.actor,
                    identity.actor.on_behalf_of,
                    false,
                    &[row],
                )
                .await
                .map_err(store_status)?;
            Ok(json!({ "proposal_id": id, "needs_approval": true }))
        }
        other => Err(Problem::new(
            StatusCode::FORBIDDEN,
            "needs_approval",
            other.to_owned(),
        )),
    }
}

async fn mass_edit(
    state: &AppState,
    identity: &Identity,
    body: MassBody,
) -> Result<Value, Problem> {
    let cached = state.models.get(&state.store).await?;
    let heads = state
        .store
        .list_object_heads(body.limit.unwrap_or(1000))
        .await
        .map_err(store_status)?;
    let mut rows = Vec::new();
    for (index, (id, key, type_id, version)) in heads.into_iter().enumerate() {
        if let Some(ids) = &body.ids
            && !ids.contains(&id)
        {
            continue;
        }
        let type_key = cached
            .defs
            .types
            .iter()
            .find(|object_type| object_type.id.as_uuid() == type_id)
            .map(|object_type| object_type.key.clone())
            .unwrap_or_default();
        if let Some(want) = &body.object_type
            && want != &type_key
        {
            continue;
        }
        rows.push(ProposalRow {
            id: Uuid::now_v7(),
            object_id: Some(id),
            object_key: Some(key),
            type_key: Some(type_key),
            base_version: Some(version),
            patch: json!({ "values": body.values }),
            status: "pending".into(),
            conflict: false,
            position: i32::try_from(index).unwrap_or(0),
        });
    }
    let id = Uuid::now_v7();
    state
        .store
        .create_proposal(
            id,
            body.title.as_deref().unwrap_or("Mass edit"),
            "mass_edit",
            identity.actor.actor,
            identity.actor.on_behalf_of,
            false,
            &rows,
        )
        .await
        .map_err(store_status)?;
    Ok(json!({ "id": id, "rows": rows.len() }))
}

fn object_map(value: Option<&Value>) -> BTreeMap<String, Value> {
    value
        .and_then(Value::as_object)
        .map(|map| {
            map.iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect()
        })
        .unwrap_or_default()
}

fn optional_group_map(value: Option<&Value>) -> BTreeMap<String, Option<String>> {
    value
        .and_then(Value::as_object)
        .map(|map| {
            map.iter()
                .map(|(key, value)| (key.clone(), value.as_str().map(str::to_owned)))
                .collect()
        })
        .unwrap_or_default()
}

fn cookie_from(headers: &HeaderMap, name: &str) -> Option<String> {
    let header = headers.get(header::COOKIE)?.to_str().ok()?;
    for part in header.split(';') {
        let (key, value) = part.trim().split_once('=')?;
        if key == name {
            return Some(value.to_owned());
        }
    }
    None
}

#[derive(Deserialize)]
struct LoginBody {
    email: String,
    password: String,
}

#[derive(Deserialize)]
struct TokenBody {
    name: String,
    #[serde(default)]
    kind: String,
}

#[derive(Deserialize)]
struct AgentBody {
    messages: Vec<Value>,
    #[serde(default)]
    stream: bool,
}

#[derive(Deserialize)]
struct SearchParams {
    q: Option<String>,
    lang: Option<String>,
    filter: Option<String>,
    sort: Option<String>,
    group: Option<String>,
    page: Option<u32>,
    per_page: Option<u32>,
    wait_seq: Option<i64>,
}

#[derive(Deserialize)]
struct ListParams {
    q: Option<String>,
    #[serde(rename = "type")]
    r#type: Option<String>,
    limit: Option<i64>,
}

#[derive(Deserialize)]
struct ExportBody {
    #[serde(default)]
    columns: Vec<String>,
    q: Option<String>,
    lang: Option<String>,
    filter: Option<String>,
    #[serde(rename = "type")]
    r#type: Option<String>,
}

#[derive(Deserialize)]
struct WorkspaceBody {
    id: Option<Uuid>,
    name: String,
    locked: Option<bool>,
    share_token: Option<String>,
    layout: Option<Value>,
}

#[derive(Deserialize)]
struct StateQuery {
    environment: Option<String>,
}

#[derive(Deserialize)]
struct StateBody {
    environment: Option<String>,
    document: Value,
}

#[derive(Deserialize)]
struct AssignBody {
    email: String,
}

#[derive(Deserialize)]
struct ViewBody {
    id: Option<Uuid>,
    workspace_id: Uuid,
    #[serde(rename = "type")]
    r#type: String,
    title: String,
    pane: Option<String>,
    position: Option<i32>,
    state: Option<Value>,
}

#[derive(Deserialize)]
struct ViewTypeBody {
    key: String,
    title: String,
    embed_url: Option<String>,
}

#[derive(Deserialize)]
struct AssortmentBody {
    id: Option<Uuid>,
    name: String,
    query: Value,
}

#[derive(Deserialize)]
struct JobTypeBody {
    key: String,
    title: String,
    #[serde(default)]
    reads_fields: Vec<String>,
    assortment_id: Option<Uuid>,
}

#[derive(Deserialize)]
struct BulkBody {
    objects: Vec<CreateObject>,
}

#[derive(Deserialize)]
struct ProposalBody {
    title: String,
    source: Option<String>,
    #[serde(default)]
    rows: Vec<ProposalRowBody>,
}

#[derive(Deserialize)]
struct ProposalRowBody {
    id: Option<Uuid>,
    object_id: Option<Uuid>,
    object_key: Option<String>,
    type_key: Option<String>,
    base_version: Option<i64>,
    patch: Value,
    conflict: Option<bool>,
}

#[derive(Deserialize)]
struct AcceptBody {
    rows: Option<Vec<Uuid>>,
}

#[derive(Deserialize)]
struct HistoryParams {
    object: Option<Uuid>,
    limit: Option<i64>,
}

#[derive(Deserialize)]
struct AsOfParams {
    seq: i64,
}

#[derive(Deserialize)]
struct RollbackBody {
    seq: i64,
}

#[derive(Deserialize)]
struct RelationBody {
    to: Uuid,
    kind: String,
}

#[derive(Deserialize)]
struct ImpactBody {
    remove_attribute: Option<String>,
    remove_class: Option<String>,
    attribute: Option<String>,
    kind: Option<AttributeKind>,
}

#[derive(Deserialize)]
struct WebhookBody {
    url: String,
    secret: Option<String>,
    assortment_id: Option<Uuid>,
}

#[derive(Deserialize)]
struct MassBody {
    title: Option<String>,
    #[serde(rename = "type")]
    object_type: Option<String>,
    ids: Option<Vec<Uuid>>,
    #[serde(default)]
    values: BTreeMap<String, Value>,
    limit: Option<i64>,
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use axum::extract::Json;
    use axum::routing::post;
    use tokio::net::TcpListener;

    use super::*;

    #[test]
    fn dotted_names_become_underscores_and_map_back() {
        assert_eq!(tool_name("create_object"), "create_object");
        assert_eq!(tool_name("a.b.c"), "a_b_c");
        assert!(!tool_name("a.b").contains('.'));
        for op in registry() {
            let wire = tool_name(op.name);
            assert!(!wire.contains('.'), "{wire}");
            assert_eq!(operation_for_tool(&wire), Some(op.name));
        }
    }

    #[tokio::test]
    async fn mocked_openai_receives_responses_tools_without_dots() {
        let seen = Arc::new(Mutex::new(None));
        let captured = Arc::clone(&seen);
        let app = axum::Router::new().route(
            "/v1/responses",
            post(move |headers: HeaderMap, Json(body): Json<Value>| {
                let captured = Arc::clone(&captured);
                async move {
                    let authorized = headers
                        .get(header::AUTHORIZATION)
                        .and_then(|value| value.to_str().ok())
                        == Some("Bearer test-key");
                    *captured.lock().expect("lock") = Some((authorized, body));
                    Json(json!({
                        "output_text": "Hej",
                        "output": [{
                            "type": "message",
                            "content": [{"type": "output_text", "text": "Hej"}]
                        }]
                    }))
                }
            }),
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let address = listener.local_addr().expect("addr");
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve");
        });
        let body = responses_body("gpt-6-luna", &[json!({ "role": "user", "content": "hej" })]);
        let url = format!("http://{address}/v1/responses");
        let payload = post_openai_when_up(&url, "test-key", &body)
            .await
            .expect("openai");
        assert_eq!(reply_text(&payload), "Hej");
        let (authorized, sent) = seen.lock().expect("lock").clone().expect("body");
        assert!(authorized);
        assert_eq!(sent["model"], "gpt-6-luna");
        assert_eq!(sent["reasoning"]["effort"], "none");
        let tools = sent["tools"].as_array().expect("tools");
        assert!(!tools.is_empty());
        for tool in tools {
            assert_eq!(tool["type"], "function");
            let name = tool["name"].as_str().expect("name");
            assert!(
                !name.contains('.'),
                "tool name {name} is not a valid OpenAI function name"
            );
            assert!(
                name.chars()
                    .all(|character| character.is_ascii_alphanumeric()
                        || character == '_'
                        || character == '-')
            );
            assert_eq!(tool["parameters"]["type"], "object");
            assert!(operation_for_tool(name).is_some(), "{name}");
        }
        assert!(tools.iter().any(|tool| tool["name"] == "create_object"));
        assert!(tools.iter().any(|tool| tool["name"] == "ui_filter"));
        assert!(tools.iter().any(|tool| tool["name"] == "model_draft"));
        assert!(tools.iter().any(|tool| tool["name"] == "model_command"));
    }

    #[test]
    fn ui_filter_tool_name_maps_back_and_echoes_args() {
        assert_eq!(tool_name("ui.filter"), "ui_filter");
        assert_eq!(operation_for_tool("ui_filter"), Some("ui.filter"));
        let value = ui_command("ui.filter", &json!({ "q": "tält" }));
        assert_eq!(value["ui"], true);
        assert_eq!(value["name"], "ui.filter");
        assert_eq!(value["args"]["q"], "tält");
        assert_eq!(value["applied"], false);
    }

    #[test]
    fn function_call_named_ui_filter_maps_to_the_operation() {
        let payload = json!({
            "output": [{
                "type": "function_call",
                "name": "ui_filter",
                "call_id": "call_1",
                "arguments": "{\"q\":\"tält\"}"
            }]
        });
        let calls = function_calls(&payload);
        assert_eq!(calls.len(), 1);
        assert_eq!(operation_for_tool(&calls[0].name), Some("ui.filter"));
    }

    #[tokio::test]
    async fn mocked_openai_error_is_a_swedish_message() {
        let app = axum::Router::new().route(
            "/v1/responses",
            post(|| async {
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": { "message": "reasoning_effort must be none" } })),
                )
            }),
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let address = listener.local_addr().expect("addr");
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve");
        });
        let error = post_openai_when_up(
            &format!("http://{address}/v1/responses"),
            "test-key",
            &json!({ "model": "gpt-6-luna" }),
        )
        .await
        .expect_err("status");
        assert!(error.detail.contains("The model did not reply"));
        assert!(error.detail.contains("reasoning_effort must be none"));
    }

    async fn post_openai_when_up(url: &str, key: &str, body: &Value) -> Result<Value, Problem> {
        let client = reqwest::Client::new();
        let mut last = None;
        for _ in 0..40 {
            match post_openai(&client, url, key, body).await {
                Err(problem) if problem.detail.contains("could not be reached") => {
                    last = Some(problem);
                    tokio::time::sleep(Duration::from_millis(25)).await;
                }
                other => return other,
            }
        }
        Err(last.expect("attempt"))
    }
}
