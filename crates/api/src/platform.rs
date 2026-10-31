//! Product surface on top of the object log: people, operations, workspaces,
//! search, jobs, proposals, history and the live stream.
//!
//! Buttons, the command palette, the in-app agent and the MCP endpoint call
//! [`execute`]. An agent or import never writes directly; the call becomes a
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
use braian_model::{AttributeId, AttributeKind, DefinitionSet, Object, ObjectId, TypeId};
use braian_search::SearchQuery;
use braian_store::{
    Actor, BatchKind, HistoryEntry, ObjectOp, ObjectProjection, ProposalLock, ProposalRow,
    ProposalWrite, Store, WriteReceipt,
};
use futures::stream;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use utoipa::{IntoParams, ToSchema};
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
        .route("/v1/objects/names", post(object_names))
        .route("/v1/export", post(crate::export::export_xlsx))
        .route("/v1/exports/{id}", get(crate::export::download_export))
        .route("/v1/stats", get(stats))
        .route("/v1/workspaces", get(list_workspaces).post(save_workspace))
        .route(
            "/v1/workspaces/{id}",
            get(get_workspace).delete(delete_workspace),
        )
        .route(
            "/v1/workspaces/{id}/state",
            get(get_workspace_state)
                .put(put_workspace_state)
                .delete(delete_workspace_state),
        )
        .route(
            "/v1/views/{view_id}/column-prefs",
            get(crate::column_prefs::get_column_prefs)
                .put(crate::column_prefs::put_column_prefs)
                .delete(crate::column_prefs::delete_column_prefs),
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
        .route("/v1/assortments/{id}", get(get_assortment))
        .route("/v1/job-types", get(list_job_types).post(register_job_type))
        .route("/v1/job-types/{key}/runs", get(job_runs))
        .route("/v1/jobs/{id}", get(job_run))
        .route("/v1/bulk", post(bulk))
        .route("/v1/proposals", get(list_proposals).post(create_proposal))
        .route("/v1/proposals/{id}", get(get_proposal))
        .route("/v1/proposals/{id}/rows", get(proposal_rows_page))
        .route("/v1/proposals/{id}/accept", post(accept_proposal))
        .route("/v1/proposals/{id}/discard", post(discard_proposal))
        .route("/v1/frecency", get(get_frecency).post(post_frecency))
        .route("/v1/history", get(history))
        .route("/v1/objects/{id}/as-of", get(as_of))
        .route("/v1/objects/{id}/rollback", post(rollback))
        .route("/v1/objects/{id}/graph", get(graph))
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
            name: "save_workspace",
            title: "Save workspace",
            permission: Role::Editor,
            needs_approval: false,
        },
        Operation {
            name: "export",
            title: "Export",
            permission: Role::Viewer,
            needs_approval: false,
        },
        Operation {
            name: "import.start",
            title: "Start import draft",
            permission: Role::Editor,
            needs_approval: false,
        },
        Operation {
            name: "import.status",
            title: "Import status",
            permission: Role::Viewer,
            needs_approval: false,
        },
        Operation {
            name: "import.map",
            title: "Set import mapping",
            permission: Role::Editor,
            needs_approval: false,
        },
        Operation {
            name: "import.preview",
            title: "Preview import draft",
            permission: Role::Editor,
            needs_approval: false,
        },
        Operation {
            name: "import.discard",
            title: "Discard import draft",
            permission: Role::Editor,
            needs_approval: false,
        },
        Operation {
            name: "import.mappings.list",
            title: "List saved import mappings",
            permission: Role::Viewer,
            needs_approval: false,
        },
        Operation {
            name: "import.mappings.rename",
            title: "Rename saved import mapping",
            permission: Role::Editor,
            needs_approval: false,
        },
        Operation {
            name: "import.mappings.delete",
            title: "Delete saved import mapping",
            permission: Role::Editor,
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
            name: "ui.open_changes",
            title: "Open changes",
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
            title: "Export",
            permission: Role::Viewer,
            needs_approval: false,
        },
        Operation {
            name: "ui.get_context",
            title: "Read workspace, panes, focus, selection, filters and language",
            permission: Role::Viewer,
            needs_approval: false,
        },
        Operation {
            name: "ui.get_selection",
            title: "Read the selected objects",
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
            title: "Move group",
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
            name: "model.attach_attribute",
            title: "Add attribute",
            permission: Role::Editor,
            needs_approval: true,
        },
        Operation {
            name: "model.detach_attribute",
            title: "Remove attribute",
            permission: Role::Editor,
            needs_approval: true,
        },
        Operation {
            name: "model.update_attribute",
            title: "Update attribute",
            permission: Role::Editor,
            needs_approval: true,
        },
        Operation {
            name: "model.update_type",
            title: "Update type names",
            permission: Role::Editor,
            needs_approval: true,
        },
        Operation {
            name: "model.create_type",
            title: "New type",
            permission: Role::Editor,
            needs_approval: true,
        },
        Operation {
            name: "model.create_value_list",
            title: "New value list",
            permission: Role::Editor,
            needs_approval: true,
        },
        Operation {
            name: "model.update_value_list",
            title: "Update value list",
            permission: Role::Editor,
            needs_approval: true,
        },
        Operation {
            name: "model.create_unit_family",
            title: "New unit family",
            permission: Role::Editor,
            needs_approval: true,
        },
        Operation {
            name: "model.update_unit_family",
            title: "Update unit family",
            permission: Role::Editor,
            needs_approval: true,
        },
        Operation {
            name: "model.update_structure",
            title: "Rename structure",
            permission: Role::Editor,
            needs_approval: true,
        },
        Operation {
            name: "model.reorder_type_attributes",
            title: "Reorder attributes",
            permission: Role::Editor,
            needs_approval: true,
        },
        Operation {
            name: "model.undo",
            title: "Undo model change",
            permission: Role::Editor,
            needs_approval: false,
        },
        Operation {
            name: "model.create_group",
            title: "Add group",
            permission: Role::Editor,
            needs_approval: true,
        },
        Operation {
            name: "model.rename_group",
            title: "Rename group",
            permission: Role::Editor,
            needs_approval: true,
        },
        Operation {
            name: "model.move_group",
            title: "Move group",
            permission: Role::Editor,
            needs_approval: true,
        },
        Operation {
            name: "model.duplicate_group",
            title: "Duplicate group",
            permission: Role::Editor,
            needs_approval: true,
        },
        Operation {
            name: "model.delete_group",
            title: "Delete group",
            permission: Role::Editor,
            needs_approval: true,
        },
        Operation {
            name: "model.set_structures",
            title: "Allowed trees…",
            permission: Role::Editor,
            needs_approval: true,
        },
        Operation {
            name: "move_to_group",
            title: "Move to group…",
            permission: Role::Editor,
            needs_approval: false,
        },
        Operation {
            name: "model.rename_type_key",
            title: "Rename key…",
            permission: Role::Editor,
            needs_approval: true,
        },
        Operation {
            name: "model.rename_attribute_key",
            title: "Rename key…",
            permission: Role::Editor,
            needs_approval: true,
        },
        Operation {
            name: "model.rename_structure_key",
            title: "Rename key…",
            permission: Role::Editor,
            needs_approval: true,
        },
        Operation {
            name: "model.rename_group_key",
            title: "Rename key…",
            permission: Role::Editor,
            needs_approval: true,
        },
        Operation {
            name: "model.rename_value_list_key",
            title: "Rename key…",
            permission: Role::Editor,
            needs_approval: true,
        },
        Operation {
            name: "model.rename_unit_family_key",
            title: "Rename key…",
            permission: Role::Editor,
            needs_approval: true,
        },
        Operation {
            name: "model.create_class",
            title: "Create group",
            permission: Role::Editor,
            needs_approval: true,
        },
        Operation {
            name: "model.list_attribute_definitions",
            title: "List attribute definitions",
            permission: Role::Viewer,
            needs_approval: false,
        },
        Operation {
            name: "list_attribute_paths",
            title: "List attribute paths",
            permission: Role::Viewer,
            needs_approval: false,
        },
        Operation {
            name: "get_view_column_prefs",
            title: "Get view column preferences",
            permission: Role::Viewer,
            needs_approval: false,
        },
        Operation {
            name: "put_view_column_prefs",
            title: "Save view column preferences",
            permission: Role::Viewer,
            needs_approval: false,
        },
        Operation {
            name: "model.attribute_usage",
            title: "Attribute usage",
            permission: Role::Viewer,
            needs_approval: false,
        },
        Operation {
            name: "list_assortments",
            title: "List selections",
            permission: Role::Viewer,
            needs_approval: false,
        },
        Operation {
            name: "assortment.save",
            title: "Save selection",
            permission: Role::Editor,
            needs_approval: true,
        },
        Operation {
            name: "model.set_parent",
            title: "Set group parent",
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
        Operation {
            name: "model.create_structure",
            title: "Create structure",
            permission: Role::Editor,
            needs_approval: true,
        },
        Operation {
            name: "model.rename_structure",
            title: "Rename structure",
            permission: Role::Editor,
            needs_approval: true,
        },
        Operation {
            name: "model.delete_structure",
            title: "Delete structure",
            permission: Role::Editor,
            needs_approval: true,
        },
        Operation {
            name: "model.delete_type",
            title: "Delete type",
            permission: Role::Editor,
            needs_approval: true,
        },
        Operation {
            name: "model.delete_attribute",
            title: "Delete attribute",
            permission: Role::Editor,
            needs_approval: true,
        },
        Operation {
            name: "model.delete_value_list",
            title: "Delete value list",
            permission: Role::Editor,
            needs_approval: true,
        },
        Operation {
            name: "model.delete_value_option",
            title: "Delete value list option",
            permission: Role::Editor,
            needs_approval: true,
        },
        Operation {
            name: "model.delete_unit_family",
            title: "Delete unit family",
            permission: Role::Editor,
            needs_approval: true,
        },
        Operation {
            name: "model.delete_unit",
            title: "Delete unit",
            permission: Role::Editor,
            needs_approval: true,
        },
        Operation {
            name: "delete_object",
            title: "Delete object",
            permission: Role::Editor,
            needs_approval: false,
        },
        Operation {
            name: "list_links",
            title: "List links",
            permission: Role::Viewer,
            needs_approval: false,
        },
        Operation {
            name: "create_link",
            title: "Create link",
            permission: Role::Editor,
            needs_approval: false,
        },
        Operation {
            name: "update_link",
            title: "Edit link",
            permission: Role::Editor,
            needs_approval: false,
        },
        Operation {
            name: "remove_link",
            title: "Remove link",
            permission: Role::Editor,
            needs_approval: false,
        },
        Operation {
            name: "model.create_link_type",
            title: "New link type",
            permission: Role::Editor,
            needs_approval: true,
        },
        Operation {
            name: "model.update_link_type",
            title: "Update link type",
            permission: Role::Editor,
            needs_approval: true,
        },
        Operation {
            name: "model.delete_link_type",
            title: "Delete link type",
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

async fn translate_workspace_views(state: &AppState, workspace: &mut Value) {
    let Ok(cached) = state.models.get(&state.store).await else {
        return;
    };
    let Some(views) = workspace.get_mut("views").and_then(Value::as_array_mut) else {
        return;
    };
    for view in views {
        if let Some(view_state) = view.get_mut("state") {
            braian_model::translate_view_state(view_state, &cached.defs);
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

/// UI operations withheld from the agent and from MCP.
/// Empty on purpose: every registered UI operation is exposed. Add a name
/// here only when the browser must keep the operation and the agent must not
/// be offered it, and keep that name out of both tool lists.
const AGENT_UI_EXCLUDED: &[&str] = &[];

/// Operations an agent must not be offered and must not run.
///
/// `accept_proposal` is a person's save. Every other name here deletes data or
/// a definition that data hangs on. `propose` rejects those steps too.
const AGENT_COMMIT_EXCLUDED: &[&str] = &[
    "accept_proposal",
    "discard_proposal",
    "delete_object",
    "import.discard",
    "import.mappings.delete",
    "model.delete_attribute",
    "model.delete_group",
    "model.delete_structure",
    "model.delete_type",
    "model.delete_unit",
    "model.delete_unit_family",
    "model.delete_value_list",
    "model.delete_value_option",
    "model.delete_link_type",
    "model.detach_attribute",
    "model.remove_attribute",
    "model.undo",
    "remove_link",
];

fn exposed_to_agent(op: &Operation) -> bool {
    !AGENT_UI_EXCLUDED.contains(&op.name) && !AGENT_COMMIT_EXCLUDED.contains(&op.name)
}

/// True when the operation removes data or a definition that data hangs on.
pub(crate) fn operation_deletes(name: &str) -> bool {
    name.split(['.', '_'])
        .any(|part| matches!(part, "delete" | "remove" | "discard" | "detach"))
}

#[cfg(test)]
fn is_ui_operation(name: &str) -> bool {
    name.starts_with("ui.") || is_layout_command(name) || name == "get_layout"
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

pub(crate) fn require(identity: &Identity, role: Role) -> Result<(), Problem> {
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
    !identity.may_commit()
        && matches!(
            op.name,
            "create_object" | "patch_object" | "move_to_group" | "create_link" | "update_link"
        )
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
    // Playwright reuses one demo user across specs; leftover hop columns from an earlier
    // test break later count assertions. Only e2e.mjs sets BRAIAN_E2E=1.
    if std::env::var("BRAIAN_E2E").ok().as_deref() == Some("1") {
        state
            .store
            .delete_all_view_column_prefs_for_user(user.id)
            .await
            .map_err(store_status)?;
    }
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

async fn configured_types(state: &AppState) -> Vec<braian_model::ObjectType> {
    match state.models.get(&state.store).await {
        Ok(cached) => cached.defs.types.clone(),
        Err(_) => Vec::new(),
    }
}

async fn list_operations(
    State(state): State<AppState>,
    Auth(identity): Auth,
) -> Result<Json<Value>, Problem> {
    require(&identity, Role::Viewer)?;
    let types = configured_types(&state).await;
    let ops = registry()
        .iter()
        .filter(|op| identity.may_commit() || exposed_to_agent(op))
        .map(|op| {
            json!({
                "name": op.name,
                "title": braian_model::operation_title(op.title, &types, "en"),
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
    let types = configured_types(&state).await;
    let result = match method {
        "initialize" => json!({
            "protocolVersion": "2025-06-18",
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "braian", "version": "0.0.0" }
        }),
        "tools/list" => json!({
            "tools": registry().iter().filter(|op| exposed_to_agent(op)).map(|op| json!({
                "name": tool_name(op.name),
                "description": braian_model::operation_title(op.title, &types, "en"),
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
        let context = body.context;
        tokio::spawn(async move {
            drive_agent(&state, &identity, messages, context, &tx).await;
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
    drive_agent(&state, &identity, body.messages, body.context, &tx).await;
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
    context: Value,
    tx: &tokio::sync::mpsc::UnboundedSender<Value>,
) {
    // A question about the marked row is answered from the snapshot. That
    // skips the model, so it cannot open a list or claim the screen is invisible.
    if let Some(text) = selection_reply(&incoming, &context) {
        let _ = tx.send(json!({ "type": "answer", "text": text, "available": true }));
        return;
    }
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
    let latest = latest_user_text(&incoming);
    let mut messages = vec![json!({
        "role": "system",
        "content": agent_system_prompt(&context, &latest)
    })];
    messages.extend(incoming);
    let client = reqwest::Client::new();
    let url = openai_responses_url();
    let types = configured_types(state).await;
    let payload = match post_openai(
        &client,
        &url,
        &key,
        &responses_body(&model, &messages, &types),
    )
    .await
    {
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
        let call_args = selection_args(operation, &args, &context);
        let result = if let Some(value) = context_tool_result(operation, &call_args, Some(&context))
        {
            value
        } else {
            run_operation(state, identity, operation, &call_args, true)
                .await
                .unwrap_or_else(|problem| json!({ "error": problem.detail, "code": problem.code }))
        };
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

fn responses_body(model: &str, messages: &[Value], types: &[braian_model::ObjectType]) -> Value {
    let tools = registry()
        .iter()
        .filter(|op| exposed_to_agent(op))
        .map(|op| {
            json!({
                "type": "function",
                "name": tool_name(op.name),
                "description": braian_model::operation_title(op.title, types, "en"),
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
    join_reply_parts(&parts)
}

fn join_reply_parts(parts: &[&str]) -> String {
    let mut out = String::new();
    for part in parts {
        if part.is_empty() {
            continue;
        }
        if !out.is_empty() {
            let needs_gap = !out
                .chars()
                .next_back()
                .map(char::is_whitespace)
                .unwrap_or(true)
                && !part.chars().next().map(char::is_whitespace).unwrap_or(true);
            if needs_gap {
                out.push(' ');
            }
        }
        out.push_str(part);
    }
    out
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

fn empty_ui_context() -> Value {
    json!({
        "workspace": null,
        "panes": [],
        "focused": null,
        "selection": [],
        "filter": "",
        "sort": "",
        "columns": [],
        "language": ""
    })
}

fn field_text(value: &Value, key: &str, max: usize) -> String {
    clip(
        value.get(key).and_then(Value::as_str).unwrap_or("").trim(),
        max,
    )
}

fn compact_pane(value: &Value) -> Value {
    json!({
        "id": field_text(value, "id", 80),
        "type": field_text(value, "type", 80),
        "title": field_text(value, "title", 120),
        "focused": value.get("focused").and_then(Value::as_bool).unwrap_or(false),
    })
}

/// A table column: `ui.hide_column` and `ui.show_column` take its `id` as `column`.
fn compact_column(value: &Value) -> Option<Value> {
    let id = field_text(value, "id", 80);
    if id.is_empty() {
        return None;
    }
    Some(json!({
        "id": id,
        "header": field_text(value, "header", 120),
        "hidden": value.get("hidden").and_then(Value::as_bool).unwrap_or(false),
    }))
}

fn compact_selected(value: &Value) -> Option<Value> {
    let id = field_text(value, "id", 80);
    let key = field_text(value, "key", 80);
    let name = field_text(value, "name", 200);
    let kind = field_text(value, "type", 80);
    if id.is_empty() && key.is_empty() && name.is_empty() {
        return None;
    }
    let mut selected = json!({ "id": id, "key": key, "name": name, "type": kind });
    if let Some(version) = value.get("version").and_then(|item| {
        item.as_i64()
            .or_else(|| item.as_u64().map(|n| n as i64))
            .or_else(|| item.as_str().and_then(|text| text.parse().ok()))
    }) {
        selected["version"] = json!(version);
    }
    Some(selected)
}

/// The screen the browser sent, reduced to the fields the agent is allowed to trust.
fn compact_ui_context(context: &Value) -> Value {
    if !context.is_object() {
        return empty_ui_context();
    }
    let workspace = match context.get("workspace") {
        Some(value) if value.is_object() => {
            let mut workspace = json!({
                "id": field_text(value, "id", 80),
                "name": field_text(value, "name", 120),
            });
            let kind = field_text(value, "type", 80);
            if !kind.is_empty() {
                workspace["type"] = json!(kind);
            }
            workspace
        }
        _ => Value::Null,
    };
    let panes = context
        .get("panes")
        .and_then(Value::as_array)
        .map(|panes| panes.iter().take(24).map(compact_pane).collect::<Vec<_>>())
        .unwrap_or_default();
    let focused = context
        .get("focused")
        .and_then(Value::as_str)
        .map(|value| clip(value.trim(), 80));
    let selection = context
        .get("selection")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .take(24)
                .filter_map(compact_selected)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let columns = context
        .get("columns")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .take(64)
                .filter_map(compact_column)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    json!({
        "workspace": workspace,
        "panes": panes,
        "focused": focused,
        "selection": selection,
        "filter": field_text(context, "filter", 200),
        "sort": field_text(context, "sort", 120),
        "columns": columns,
        "language": field_text(context, "language", 16),
    })
}

fn latest_user_text(messages: &[Value]) -> String {
    messages
        .iter()
        .rev()
        .find_map(|message| {
            if message.get("role").and_then(Value::as_str) != Some("user") {
                return None;
            }
            message
                .get("content")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .unwrap_or_default()
}

fn agent_system_prompt(context: &Value, _latest: &str) -> String {
    let snapshot =
        serde_json::to_string(&compact_ui_context(context)).unwrap_or_else(|_| "{}".to_owned());
    format!(
        "You are the Braian agent. You always act for the signed-in person. Write changes must become proposals, not direct writes. \
Answer in English. \
The UI snapshot is the current screen: workspace, open panes with their types, focused pane, selected objects (id, key, name, type, version), active filter and sort, the open table's columns (id, header, hidden), and the data language. \
To hide or show a column, call ui_hide_column or ui_show_column with {{\"column\": id}}. \
Answer questions about the selection, the open views, or the workspace from that snapshot. Do not call search or list_objects for those questions, and do not ask for a screenshot. \
Call get_object when a field value is missing from the snapshot. Call ui_get_context or ui_get_selection to read the snapshot again. \
UI snapshot: {snapshot}"
    )
}

// english-check: allow
fn asks_which_is_selected(text: &str) -> bool {
    let lower = text.to_lowercase();
    const PHRASES: &[&str] = &[
        "vilken produkt",
        "vilket objekt",
        "vilken artikel",
        "vilka produkter",
        "vilka objekt",
        "vilken rad",
        "vilken är markerad",
        "vad är markerat",
        "vad är valt",
        "which product",
        "which object",
        "which article",
        "which item",
        "which row",
        "what is selected",
        "what's selected",
        "what is marked",
        "what's marked",
        "welche produkt",
        "welches objekt",
        "welches produkt",
        "was ist markiert",
        "was ist ausgewählt",
        "was ist ausgewahlt",
    ];
    if PHRASES.iter().any(|phrase| lower.contains(phrase)) {
        return !asks_for_field(&lower);
    }
    let about = lower.contains("markerad")
        || lower.contains("markerat")
        || lower.contains("is selected")
        || lower.contains("are selected")
        || lower.contains("is marked")
        || lower.contains("ist markiert");
    let identity = lower.contains("vilken")
        || lower.contains("vilket")
        || lower.contains("vilka")
        || lower.contains("which")
        || lower.contains("vad är")
        || lower.contains("what is")
        || lower.contains("what's")
        || lower.contains("welche")
        || lower.contains("was ist");
    about && identity && !asks_for_field(&lower)
}

fn asks_for_field(text: &str) -> bool {
    [
        "status",
        "vikt",
        "weight",
        "pris",
        "price",
        "beskrivning",
        "description",
        "fält",
        "field",
        "värde",
        "value",
    ]
    .iter()
    .any(|needle| text.contains(needle))
}
// english-check: end

fn selection_items(context: &Value) -> Vec<Value> {
    context
        .get("selection")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn object_label(item: &Value) -> String {
    let key = field_text(item, "key", 80);
    let name = field_text(item, "name", 200);
    let kind = field_text(item, "type", 80);
    let id = field_text(item, "id", 80);
    let head = match (key.is_empty(), name.is_empty()) {
        (false, false) => format!("{key} {name}"),
        (false, true) => key,
        (true, false) => name,
        (true, true) => id,
    };
    if kind.is_empty() {
        head
    } else {
        format!("{head} ({kind})")
    }
}

fn format_selection(items: &[Value]) -> String {
    if items.is_empty() {
        return "Nothing is selected.".to_owned();
    }
    let labels = items
        .iter()
        .map(object_label)
        .filter(|label| !label.is_empty())
        .collect::<Vec<_>>();
    let joined = labels.join(", ");
    if items.len() == 1 {
        return format!("{joined} is selected.");
    }
    format!("Selected: {joined}.")
}

/// Direct answer when the person asks which object is marked and the snapshot says.
fn selection_reply(messages: &[Value], context: &Value) -> Option<String> {
    if !context.is_object() {
        return None;
    }
    let question = latest_user_text(messages);
    if !asks_which_is_selected(&question) {
        return None;
    }
    let snapshot = compact_ui_context(context);
    Some(format_selection(&selection_items(&snapshot)))
}

fn context_tool_result(name: &str, args: &Value, injected: Option<&Value>) -> Option<Value> {
    let op = find_op(name)?;
    if op.name != "ui.get_context" && op.name != "ui.get_selection" {
        return None;
    }
    let snapshot = match injected {
        Some(value) if value.is_object() => compact_ui_context(value),
        _ if args.get("workspace").is_some()
            || args.get("selection").is_some()
            || args.get("panes").is_some() =>
        {
            compact_ui_context(args)
        }
        _ => compact_ui_context(args.get("context").unwrap_or(&Value::Null)),
    };
    Some(if op.name == "ui.get_selection" {
        json!({ "selection": snapshot.get("selection").cloned().unwrap_or_else(|| json!([])) })
    } else {
        snapshot
    })
}

async fn stream(State(state): State<AppState>, Auth(identity): Auth) -> Result<Response, Problem> {
    require(&identity, Role::Viewer)?;
    // The body stream is only polled when a frame is ready. A broadcast
    // receiver parked inside that stream did not get woken, so the browser
    // stayed on the greeting comment. A task owns the receiver and pushes
    // frames into a channel the body forwards.
    let (frames, rx) = tokio::sync::mpsc::channel(64);
    let mut live = state.live.subscribe();
    let owner = identity.actor.actor.as_uuid().to_string();
    tokio::spawn(async move {
        if frames.send(Bytes::from(": connected\n\n")).await.is_err() {
            return;
        }
        let mut tick = tokio::time::interval(Duration::from_secs(15));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                biased;
                value = live.recv() => match value {
                    Ok(value) => {
                        if value.get("op").and_then(|item| item.as_str()) == Some("import")
                            && value.get("owner").and_then(|item| item.as_str()) != Some(owner.as_str())
                        {
                            continue;
                        }
                        let frame = Bytes::from(format!("data: {value}\n\n"));
                        if frames.send(frame).await.is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                        let frame = Bytes::from(format!(
                            "data: {{\"op\":\"lagged\",\"skipped\":{skipped}}}\n\n"
                        ));
                        if frames.send(frame).await.is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                },
                _ = tick.tick() => {
                    if frames.send(Bytes::from(": ping\n\n")).await.is_err() {
                        break;
                    }
                }
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
    let (filter_by, sort_by, group_by) = search_fields(
        &state,
        query.filter.unwrap_or_default(),
        query.sort.unwrap_or_default(),
        query.group.unwrap_or_default(),
    )
    .await?;
    let Some(search) = &state.search else {
        return Err(Problem::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "search_unavailable",
            "TYPESENSE_URL is not configured",
        ));
    };
    let mut page = search
        .search(&SearchQuery {
            text: query.q.unwrap_or_default(),
            language: query.lang.unwrap_or_else(|| "en".into()),
            filter_by,
            sort_by,
            group_by,
            page: query.page.unwrap_or(1),
            per_page: query.per_page.unwrap_or(50),
        })
        .await
        .map_err(|error| Problem::new(StatusCode::BAD_GATEWAY, "search", error.to_string()))?;
    if let Ok(cached) = state.models.get(&state.store).await {
        page = hide_link_hits(page, &cached.defs);
    }
    Ok(Json(json!({
        "found": page.found,
        "hits": page.hits,
        "groups": page.groups,
        "indexed_seq": state.store.indexer_seq().await.map_err(store_status)?,
    })))
}

async fn search_fields(
    state: &AppState,
    filter: String,
    sort: String,
    group: String,
) -> Result<(String, String, String), Problem> {
    let Ok(cached) = state.models.get(&state.store).await else {
        return Ok((filter, sort, group));
    };
    let invalid =
        |message: String| Problem::new(StatusCode::UNPROCESSABLE_ENTITY, "invalid_path", message);
    let filter =
        braian_model::translate_filter_expression(&filter, &cached.defs).map_err(invalid)?;
    let sort = braian_model::translate_sort_expression(&sort, &cached.defs).map_err(invalid)?;
    let group = if group.trim().is_empty() {
        group
    } else {
        braian_model::translate_sort_expression(&group, &cached.defs).map_err(invalid)?
    };
    Ok((filter, sort, group))
}

async fn list_objects(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Query(query): Query<ListParams>,
) -> Result<Json<Value>, Problem> {
    require(&identity, Role::Viewer)?;
    let store = state.store.on_interactive();
    let limit = query.limit.unwrap_or(200).clamp(1, 2000);
    let after_id = match query.cursor.as_deref().filter(|cursor| !cursor.is_empty()) {
        Some(cursor) => Some(decode_cursor(cursor).ok_or_else(|| {
            Problem::new(
                StatusCode::BAD_REQUEST,
                "invalid_cursor",
                "cursor is not a list cursor",
            )
        })?),
        None => None,
    };
    let cached = state.models.get(&store).await.ok();
    let type_id = match query.r#type.as_deref().filter(|value| !value.is_empty()) {
        Some(want) => {
            let Some(cached) = cached.as_ref() else {
                return Ok(empty_object_list());
            };
            match cached.model.type_id(want) {
                Some(id) => Some(id.as_uuid()),
                None => return Ok(empty_object_list()),
            }
        }
        None => None,
    };
    let key_contains = query.q.as_deref().filter(|value| !value.is_empty());
    let page_limit = usize::try_from(limit).unwrap_or(2000);
    let mut heads = store
        .list_objects_page(type_id, key_contains, after_id, limit.saturating_add(1))
        .await
        .map_err(store_status)?;
    let next_cursor = if heads.len() > page_limit {
        heads.truncate(page_limit);
        heads.last().map(|(id, _, _, _)| encode_cursor(*id))
    } else {
        None
    };
    let type_keys: BTreeMap<Uuid, String> = cached
        .as_ref()
        .map(|cached| {
            cached
                .defs
                .types
                .iter()
                .map(|object_type| (object_type.id.as_uuid(), object_type.key.clone()))
                .collect()
        })
        .unwrap_or_default();
    let objects = heads
        .into_iter()
        .map(|(id, key, type_id, version)| {
            let type_key = if cached.is_some() {
                Value::String(type_keys.get(&type_id).cloned().unwrap_or_default())
            } else {
                Value::Null
            };
            json!({
                "id": id,
                "key": key,
                "type": type_key,
                "version": version,
            })
        })
        .collect::<Vec<_>>();
    Ok(Json(json!({
        "objects": objects,
        "next_cursor": next_cursor,
    })))
}

/// A link document is not a search hit, even when an older index still holds it.
fn hide_link_hits(
    mut page: braian_search::SearchPage,
    defs: &DefinitionSet,
) -> braian_search::SearchPage {
    let links: BTreeSet<&str> = defs
        .types
        .iter()
        .filter(|object_type| object_type.is_link())
        .map(|object_type| object_type.key.as_str())
        .collect();
    if links.is_empty() {
        return page;
    }
    let link_hit = |hit: &braian_search::SearchHit| {
        hit.document
            .get("type_key")
            .and_then(Value::as_str)
            .is_some_and(|key| links.contains(key))
    };
    if page.groups.is_empty() {
        let before = page.hits.len() as u32;
        page.hits.retain(|hit| !link_hit(hit));
        let removed = before.saturating_sub(page.hits.len() as u32);
        page.found = page.found.saturating_sub(removed);
        return page;
    }
    let before_groups = page.groups.len() as u32;
    for group in &mut page.groups {
        let before = group.hits.len() as u32;
        group.hits.retain(|hit| !link_hit(hit));
        let removed = before.saturating_sub(group.hits.len() as u32);
        group.found = group.found.saturating_sub(removed);
    }
    page.groups.retain(|group| !group.hits.is_empty());
    let removed_groups = before_groups.saturating_sub(page.groups.len() as u32);
    page.found = page.found.saturating_sub(removed_groups);
    page
}

fn empty_object_list() -> Json<Value> {
    Json(json!({ "objects": [], "next_cursor": null }))
}

fn encode_cursor(id: Uuid) -> String {
    format!("v1.{}", hex::encode(id.as_bytes()))
}

fn decode_cursor(cursor: &str) -> Option<Uuid> {
    let hex_id = cursor.strip_prefix("v1.")?;
    let bytes = hex::decode(hex_id).ok()?;
    let bytes: [u8; 16] = bytes.try_into().ok()?;
    Some(Uuid::from_bytes(bytes))
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
    let user_id = membership_user_id(&identity)?;
    require_workspace_member(&state, id, user_id).await?;
    let mut workspace = state
        .store
        .workspace(id)
        .await
        .map_err(store_status)?
        .ok_or_else(|| Problem::new(StatusCode::NOT_FOUND, "not_found", "workspace"))?;
    translate_workspace_views(&state, &mut workspace).await;
    Ok(Json(workspace))
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
    let existing = state.store.workspace(id).await.map_err(store_status)?;
    if existing.is_some() {
        // Do not let an Editor overwrite another user's workspace UUID (incl. lock).
        require_workspace_member(&state, id, owner).await?;
    }
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

/// Workspace must exist and the signed-in user must be a member.
/// Returns the same 404 as a missing workspace so callers cannot probe ids.
pub(crate) async fn require_workspace_member(
    state: &AppState,
    id: Uuid,
    user_id: Uuid,
) -> Result<(), Problem> {
    let exists = state
        .store
        .workspace(id)
        .await
        .map_err(store_status)?
        .is_some();
    let member = state
        .store
        .workspace_is_member(id, user_id)
        .await
        .map_err(store_status)?;
    if !exists || !member {
        return Err(Problem::new(
            StatusCode::NOT_FOUND,
            "not_found",
            "workspace",
        ));
    }
    Ok(())
}

/// View must exist and the caller must be a member of its workspace.
/// Always `404 not_found` / `view` — do not reveal existence to non-members.
pub(crate) async fn require_view_member(
    state: &AppState,
    view_id: Uuid,
    user_id: Uuid,
) -> Result<(), Problem> {
    let not_found = || Problem::new(StatusCode::NOT_FOUND, "not_found", "view");
    let Some(workspace_id) = state
        .store
        .view_workspace_id(view_id)
        .await
        .map_err(store_status)?
    else {
        return Err(not_found());
    };
    let exists = state
        .store
        .workspace(workspace_id)
        .await
        .map_err(store_status)?
        .is_some();
    let member = state
        .store
        .workspace_is_member(workspace_id, user_id)
        .await
        .map_err(store_status)?;
    if !exists || !member {
        return Err(not_found());
    }
    Ok(())
}

fn membership_user_id(identity: &crate::auth::Identity) -> Result<Uuid, Problem> {
    identity
        .user_id
        .ok_or_else(|| Problem::new(StatusCode::NOT_FOUND, "not_found", "workspace"))
}

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
    let owner = layout_owner(&identity)?;
    require_workspace_member(&state, id, owner).await?;
    let environment = layout_environment(query.environment.as_deref());
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
    let owner = layout_owner(&identity)?;
    require_workspace_member(&state, id, owner).await?;
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
        .put_workspace_state(owner, &environment, id, &body.document)
        .await
        .map_err(store_status)?;
    Ok(Json(json!({
        "environment": environment,
        "updated_at": updated.to_rfc3339(),
    })))
}

async fn delete_workspace(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, Problem> {
    require(&identity, Role::Editor)?;
    if !identity.may_commit() {
        return Err(crate::auth::delete_forbidden());
    }
    let Some(user_id) = identity.user_id else {
        return Err(Problem::new(
            StatusCode::BAD_REQUEST,
            "no_user",
            "sign in to delete a workspace",
        ));
    };
    require_workspace_member(&state, id, user_id).await?;
    if state
        .store
        .workspace_locked(id)
        .await
        .map_err(store_status)?
    {
        return Err(Problem::new(
            StatusCode::FORBIDDEN,
            "locked",
            "unlock the workspace before deleting it",
        ));
    }
    let remaining = state
        .store
        .workspace_count_for_user(user_id)
        .await
        .map_err(store_status)?;
    if remaining <= 1 {
        return Err(Problem::new(
            StatusCode::BAD_REQUEST,
            "last_workspace",
            "the last workspace cannot be deleted",
        ));
    }
    state
        .store
        .delete_workspace(id)
        .await
        .map_err(store_status)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn delete_workspace_state(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Path(id): Path<Uuid>,
    Query(query): Query<StateQuery>,
) -> Result<StatusCode, Problem> {
    require(&identity, Role::Editor)?;
    if !identity.may_commit() {
        return Err(crate::auth::delete_forbidden());
    }
    let owner = layout_owner(&identity)?;
    require_workspace_member(&state, id, owner).await?;
    let environment = layout_environment(query.environment.as_deref());
    state
        .store
        .delete_workspace_state(owner, &environment, id)
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
    // Admin grants membership; caller need not be a member. Still 404 if missing.
    require_workspace(&state, id).await?;
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
    let mut workspace = state
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
    translate_workspace_views(&state, &mut workspace).await;
    Ok(Json(workspace))
}

async fn save_view(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Json(body): Json<ViewBody>,
) -> Result<Json<Value>, Problem> {
    require(&identity, Role::Editor)?;
    let user_id = membership_user_id(&identity)?;
    require_workspace_member(&state, body.workspace_id, user_id).await?;
    let id = body.id.unwrap_or_else(Uuid::now_v7);
    if let Some(existing_ws) = state
        .store
        .view_workspace_id(id)
        .await
        .map_err(store_status)?
    {
        // Updating an existing view: must already be a member of its workspace.
        require_workspace_member(&state, existing_ws, user_id).await?;
    }
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
    let mut view_state = body.state.unwrap_or(json!({}));
    if let Ok(cached) = state.models.get(&state.store).await {
        braian_model::translate_view_state(&mut view_state, &cached.defs);
    }
    state
        .store
        .save_view(
            id,
            body.workspace_id,
            &body.r#type,
            &body.title,
            &body.pane.unwrap_or_else(|| "main".into()),
            body.position.unwrap_or(0),
            &view_state,
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

/// Copies the workspace type into selection calls. The agent loop is the only
/// caller that has the screen; a human request is left unchanged.
fn selection_args(operation: &str, args: &Value, context: &Value) -> Value {
    if operation != "list_assortments" && operation != "assortment.save" {
        return args.clone();
    }
    let Some(token) = context
        .get("workspace")
        .and_then(|workspace| workspace.get("type"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|token| !token.is_empty())
    else {
        return args.clone();
    };
    let mut call = args.clone();
    if let Some(object) = call.as_object_mut() {
        object.insert("workspace_type".into(), json!(token));
    }
    call
}

fn wrong_type() -> Problem {
    Problem::new(
        StatusCode::UNPROCESSABLE_ENTITY,
        "wrong_type",
        "A selection belongs to one type and cannot be used with another.",
    )
}

fn missing_selection_type() -> Problem {
    Problem::new(
        StatusCode::BAD_REQUEST,
        "assortment",
        "a selection needs a type",
    )
}

async fn resolve_selection_type(state: &AppState, token: &str) -> Result<Uuid, Problem> {
    let cached = state.models.get(&state.store).await?;
    Ok(wire::resolve_type(&cached.model, token)?.as_uuid())
}

fn query_type_token(query: &Value) -> Option<&str> {
    query
        .get("type")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|token| !token.is_empty())
}

fn strip_query_type(query: &mut Value) {
    if let Some(object) = query.as_object_mut() {
        object.remove("type");
    }
}

async fn with_type_key(state: &AppState, mut row: Value) -> Value {
    let Some(type_id) = row
        .get("type_id")
        .and_then(Value::as_str)
        .and_then(|value| Uuid::parse_str(value).ok())
    else {
        return row;
    };
    let key = match state.models.get(&state.store).await {
        Ok(cached) => wire::type_key(&cached.defs, TypeId::from_uuid(type_id)),
        Err(_) => type_id.to_string(),
    };
    if let Some(object) = row.as_object_mut() {
        object.insert("type".into(), json!(key));
    }
    row
}

async fn list_assortments(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Query(query): Query<AssortmentTypeQuery>,
) -> Result<Json<Value>, Problem> {
    require(&identity, Role::Viewer)?;
    let type_id = match query
        .r#type
        .as_deref()
        .map(str::trim)
        .filter(|token| !token.is_empty())
    {
        Some(token) => Some(resolve_selection_type(&state, token).await?),
        None => None,
    };
    let rows = state
        .store
        .list_assortments(type_id)
        .await
        .map_err(store_status)?;
    let cached = state.models.get(&state.store).await.ok();
    let mut assortments = Vec::with_capacity(rows.len());
    for row in rows {
        let mut row = with_type_key(&state, row).await;
        if let Some(cached) = &cached
            && let Some(query) = row.get_mut("query")
        {
            braian_model::translate_assortment_query(query, &cached.defs);
            strip_query_type(query);
        }
        assortments.push(row);
    }
    Ok(Json(json!({ "assortments": assortments })))
}

async fn get_assortment(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Path(id): Path<Uuid>,
    Query(query): Query<AssortmentTypeQuery>,
) -> Result<Json<Value>, Problem> {
    require(&identity, Role::Viewer)?;
    let Some(row) = state.store.assortment(id).await.map_err(store_status)? else {
        return Err(Problem::new(
            StatusCode::NOT_FOUND,
            "not_found",
            "selection not found",
        ));
    };
    if let Some(token) = query
        .r#type
        .as_deref()
        .map(str::trim)
        .filter(|token| !token.is_empty())
    {
        let want = resolve_selection_type(&state, token).await?;
        let have = row
            .get("type_id")
            .and_then(Value::as_str)
            .and_then(|value| Uuid::parse_str(value).ok());
        if have != Some(want) {
            return Err(wrong_type());
        }
    }
    let mut row = with_type_key(&state, row).await;
    if let Ok(cached) = state.models.get(&state.store).await
        && let Some(query) = row.get_mut("query")
    {
        braian_model::translate_assortment_query(query, &cached.defs);
        strip_query_type(query);
    }
    Ok(Json(row))
}

async fn prepared_selection(
    state: &AppState,
    id: Option<Uuid>,
    type_token: Option<&str>,
    mut query: Value,
) -> Result<(Uuid, Value), Problem> {
    let Some(type_token) = type_token.map(str::trim).filter(|token| !token.is_empty()) else {
        return Err(missing_selection_type());
    };
    let cached = state.models.get(&state.store).await?;
    let type_id = wire::resolve_type(&cached.model, type_token)?.as_uuid();
    if let Some(query_type) = query_type_token(&query) {
        let query_id = wire::resolve_type(&cached.model, query_type)?.as_uuid();
        if query_id != type_id {
            return Err(wrong_type());
        }
    }
    strip_query_type(&mut query);
    braian_model::translate_assortment_query(&mut query, &cached.defs);
    strip_query_type(&mut query);
    if let Some(id) = id
        && let Some(existing) = state.store.assortment(id).await.map_err(store_status)?
    {
        let have = existing
            .get("type_id")
            .and_then(Value::as_str)
            .and_then(|value| Uuid::parse_str(value).ok());
        if have != Some(type_id) {
            return Err(wrong_type());
        }
    }
    Ok((type_id, query))
}

async fn save_assortment(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Json(body): Json<AssortmentBody>,
) -> Result<Json<Value>, Problem> {
    require(&identity, Role::Editor)?;
    crate::auth::require_commit(&identity)?;
    let Some(owner) = identity.user_id else {
        return Err(Problem::new(
            StatusCode::BAD_REQUEST,
            "no_user",
            "sign in to save a selection",
        ));
    };
    let id = body.id.unwrap_or_else(Uuid::now_v7);
    let (type_id, query) =
        prepared_selection(&state, Some(id), body.r#type.as_deref(), body.query).await?;
    state
        .store
        .save_assortment(id, owner, type_id, &body.name, &query)
        .await
        .map_err(store_status)?;
    let row = state
        .store
        .assortment(id)
        .await
        .map_err(store_status)?
        .unwrap_or_else(
            || json!({ "id": id, "name": body.name, "type_id": type_id, "query": query }),
        );
    Ok(Json(with_type_key(&state, row).await))
}

async fn propose_assortment(
    state: &AppState,
    identity: &Identity,
    args: &Value,
) -> Result<Value, Problem> {
    let name = args
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| !name.is_empty())
        .ok_or_else(|| Problem::new(StatusCode::BAD_REQUEST, "assortment", "missing name"))?;
    let type_token = args
        .get("type")
        .and_then(Value::as_str)
        .filter(|token| !token.trim().is_empty())
        .or_else(|| {
            args.get("workspace_type")
                .and_then(Value::as_str)
                .filter(|token| !token.trim().is_empty())
        });
    let id = args
        .get("id")
        .and_then(Value::as_str)
        .and_then(|value| Uuid::parse_str(value).ok());
    let query = args.get("query").cloned().unwrap_or(json!({}));
    let (type_id, query) = prepared_selection(state, id, type_token, query).await?;
    let proposal_id = Uuid::now_v7();
    let title = format!("Save selection {name}");
    let row = ProposalRow {
        id: Uuid::now_v7(),
        object_id: None,
        object_key: Some(name.to_owned()),
        type_key: Some("assortment".into()),
        base_version: None,
        patch: json!({
            "name": name,
            "query": query,
            "id": id,
            "type": type_id,
        }),
        status: "pending".into(),
        conflict: false,
        position: 0,
    };
    state
        .store
        .create_proposal(
            proposal_id,
            &title,
            "user",
            identity.actor.actor,
            identity.actor.on_behalf_of,
            false,
            &[row],
        )
        .await
        .map_err(store_status)?;
    Ok(
        json!({ "proposal_id": proposal_id, "id": proposal_id, "needs_approval": true, "title": title }),
    )
}

async fn apply_assortment_row(
    state: &AppState,
    identity: &Identity,
    proposal_id: Uuid,
    row_id: Uuid,
    row: &Value,
    claim: Uuid,
) -> Result<Option<i64>, Problem> {
    let patch = row.get("patch").cloned().unwrap_or(json!({}));
    let name = patch
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("Selection");
    let query = patch.get("query").cloned().unwrap_or(json!({}));
    let Some(owner) = identity.user_id else {
        return Err(Problem::new(
            StatusCode::BAD_REQUEST,
            "no_user",
            "sign in to save a selection",
        ));
    };
    let id = patch
        .get("id")
        .and_then(Value::as_str)
        .and_then(|value| Uuid::parse_str(value).ok())
        .unwrap_or_else(Uuid::now_v7);
    let type_token = patch
        .get("type")
        .and_then(Value::as_str)
        .filter(|token| !token.trim().is_empty())
        .map(str::to_owned)
        .or_else(|| query_type_token(&query).map(str::to_owned));
    let (type_id, query) =
        prepared_selection(state, Some(id), type_token.as_deref(), query).await?;
    let receipt = finish_row(
        state,
        identity,
        proposal_id,
        row_id,
        "accepted",
        ProposalWrite::Assortment {
            id,
            owner,
            type_id,
            name: name.to_owned(),
            query,
        },
        claim,
    )
    .await?;
    Ok(receipt.map(|receipt| receipt.last_seq))
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
    crate::auth::require_commit(&identity)?;
    let Some(reservation) = state.bulk.try_reserve() else {
        return Err(Problem::new(
            StatusCode::TOO_MANY_REQUESTS,
            "bulk_busy",
            "too many bulk jobs are already queued",
        ));
    };
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
    let chunk = state.bulk.chunk();
    tokio::spawn(async move {
        reservation
            .finish(run_bulk(job_state, id, actor, objects, chunk))
            .await;
    });
    Ok((
        StatusCode::ACCEPTED,
        Json(json!({ "id": id, "status": "running", "total": total })),
    ))
}

pub(crate) fn emit_job(state: &AppState, id: Uuid, status: &str, done: i32, total: i32) {
    state.emit(json!({
        "op": "job",
        "id": id,
        "status": status,
        "done": done,
        "total": total,
    }));
}

async fn run_bulk(
    state: AppState,
    id: Uuid,
    actor: Actor,
    objects: Vec<CreateObject>,
    chunk_size: usize,
) {
    let store = state.store.on_bulk();
    let total = i32::try_from(objects.len()).unwrap_or(i32::MAX);
    emit_job(&state, id, "running", 0, total);
    let mut errors = Vec::new();
    let mut done = 0i32;
    let mut actor = actor;
    actor.kind = BatchKind::Bulk;
    actor.source = "bulk".into();
    let batch = match store.open_batch(&actor).await {
        Ok(batch) => batch,
        Err(error) => {
            let _ = store
                .finish_job_run(id, "failed", 0, &error.to_string(), &json!([]))
                .await;
            emit_job(&state, id, "failed", 0, total);
            return;
        }
    };
    for chunk in objects.chunks(chunk_size.max(1)) {
        let mut ops = Vec::new();
        let cached = match state.models.get(&store).await {
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
                    "key": labeled_key_from(&cached.model, Some(&object.object_type), |key| {
                        object.values.get(key)
                    })
                    .cloned()
                    .unwrap_or(Value::Null),
                    "message": problem.detail,
                    "code": problem.code,
                })),
            }
        }
        if !ops.is_empty() {
            match store.write_ops(batch, &actor, ops).await {
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
        } else {
            store.yield_between_bulk_batches().await;
        }
        done += i32::try_from(chunk.len()).unwrap_or(0);
        let _ = store
            .update_job_progress(
                id,
                done,
                &format!("{done} rows processed"),
                &Value::Array(errors.clone()),
            )
            .await;
        emit_job(&state, id, "running", done, total);
    }
    let status = if errors.is_empty() {
        "succeeded"
    } else {
        "failed"
    };
    let _ = store
        .finish_job_run(
            id,
            status,
            done,
            &format!("{done} rows, {} errors", errors.len()),
            &Value::Array(errors),
        )
        .await;
    emit_job(&state, id, status, done, total);
}

fn key_attribute_name(
    model: &braian_rules::CompiledModel,
    type_key: Option<&str>,
) -> Option<String> {
    let type_key = type_key?;
    let type_id = model.type_id(type_key)?;
    let attribute_id = model.key_attribute(type_id)?;
    model
        .attribute(attribute_id)
        .map(|attribute| attribute.key.clone())
}

/// Business key for a bulk error or an agent proposal.
///
/// The name comes from the type's key attribute. `number` is only used when
/// that type is not in the model.
fn labeled_key_from<'a>(
    model: &braian_rules::CompiledModel,
    type_key: Option<&str>,
    get: impl Fn(&str) -> Option<&'a Value>,
) -> Option<&'a Value> {
    match key_attribute_name(model, type_key) {
        Some(name) => get(&name),
        None => get("number"),
    }
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
    let mut values = create_values(&cached.model, &body.values)?;
    braian_model::apply_defaults(&cached.defs, type_id, &mut values);
    let object = Object {
        id: wire::object_id(body.id)?,
        type_id,
        model_revision: cached.revision,
        groups,
        values,
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

#[derive(Deserialize, ToSchema)]
pub(crate) struct FrecencyBody {
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

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct ProposalListParams {
    offset: Option<i64>,
    limit: Option<i64>,
    /// `open`, `accepted` or `discarded`.
    status: Option<String>,
    /// `user`, `agent` or `import`.
    source: Option<String>,
}

async fn list_proposals(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Query(query): Query<ProposalListParams>,
) -> Result<Json<Value>, Problem> {
    require(&identity, Role::Viewer)?;
    let status = query.status.as_deref().filter(|item| *item != "all");
    let source = query.source.as_deref().filter(|item| *item != "all");
    let (total, proposals) = state
        .store
        .list_proposals_page(
            status,
            source,
            query.offset.unwrap_or(0).max(0),
            query.limit.unwrap_or(50).clamp(1, 200),
        )
        .await
        .map_err(store_status)?;
    Ok(Json(json!({
        "proposals": proposals,
        "total": total,
        "pending": state.store.pending_summary().await.map_err(store_status)?,
    })))
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct ProposalRowsParams {
    offset: Option<i64>,
    limit: Option<i64>,
    /// Row status filter. Import drafts: `new`, `changed`, `failed`, `stale`, `excluded`.
    /// Other proposals: `pending`, `accepted`, `rejected`.
    status: Option<String>,
}

/// One page of a draft's rows. Import drafts and ordinary proposals share the
/// row shape, so one table can show both.
async fn proposal_rows_page(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Path(id): Path<Uuid>,
    Query(query): Query<ProposalRowsParams>,
) -> Result<Json<Value>, Problem> {
    require(&identity, Role::Viewer)?;
    let offset = query.offset.unwrap_or(0).max(0);
    let limit = query.limit.unwrap_or(100).clamp(1, 500);
    let status = query.status.as_deref().filter(|item| *item != "all");
    if let Some(session_id) = state
        .store
        .proposal_session(id)
        .await
        .map_err(store_status)?
    {
        return Ok(Json(
            crate::import::draft_rows(&state, session_id, status, offset, limit).await?,
        ));
    }
    if state
        .store
        .proposal(id)
        .await
        .map_err(store_status)?
        .is_none()
    {
        return Err(Problem::new(StatusCode::NOT_FOUND, "not_found", "proposal"));
    }
    let (total, rows) = state
        .store
        .proposal_rows_page(id, status, offset, limit)
        .await
        .map_err(store_status)?;
    Ok(Json(
        json!({ "total": total, "rows": rows, "kind": "rows" }),
    ))
}

async fn create_proposal(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Json(body): Json<ProposalBody>,
) -> Result<Json<Value>, Problem> {
    require(&identity, Role::Editor)?;
    let id = Uuid::now_v7();
    // The caller does not choose `source`. A spoofed body field is ignored.
    let source = proposal_source(identity.kind);
    let rows = proposal_rows(&body.rows)?;
    state
        .store
        .create_proposal(
            id,
            &body.title,
            source,
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

fn proposal_source(kind: braian_store::PrincipalKind) -> &'static str {
    match kind {
        braian_store::PrincipalKind::Human => "user",
        braian_store::PrincipalKind::Agent => "agent",
        braian_store::PrincipalKind::Integration => "import",
    }
}

fn proposal_rows(rows: &[ProposalRowBody]) -> Result<Vec<ProposalRow>, Problem> {
    let mut stored = Vec::with_capacity(rows.len());
    for (index, row) in rows.iter().enumerate() {
        if updates_existing_object(row) && row.base_version.is_none() {
            return Err(base_version_required());
        }
        stored.push(ProposalRow {
            id: row.id.unwrap_or_else(Uuid::now_v7),
            object_id: row.object_id,
            object_key: row.object_key.clone(),
            type_key: row.type_key.clone(),
            base_version: row.base_version,
            patch: row.patch.clone(),
            status: "pending".into(),
            conflict: row.conflict.unwrap_or(false),
            position: i32::try_from(index).unwrap_or(0),
        });
    }
    Ok(stored)
}

fn base_version_required() -> Problem {
    Problem::new(
        StatusCode::BAD_REQUEST,
        "base_version_required",
        "a proposal that changes an existing object needs base_version",
    )
}

/// An update of an object the catalog already stores. Creates, relations,
/// model edits and assortments are not versioned object patches.
fn updates_existing_object(row: &ProposalRowBody) -> bool {
    if row.object_id.is_none() {
        return false;
    }
    if matches!(row.type_key.as_deref(), Some("model" | "assortment")) {
        return false;
    }
    !is_relation_patch(&row.patch)
}

fn is_relation_patch(patch: &Value) -> bool {
    relation_proposal(&json!({ "patch": patch })).is_some()
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
) -> Result<AcceptReply, Problem> {
    require(&identity, Role::Editor)?;
    if !identity.may_commit() {
        return Err(Problem::new(
            StatusCode::FORBIDDEN,
            "agent_cannot_accept",
            "an agent or import proposes; a person accepts",
        ));
    }
    // An import draft keeps its rows in the import session, not in proposal_rows.
    if let Some(session_id) = state
        .store
        .proposal_session(id)
        .await
        .map_err(store_status)?
    {
        let open = state
            .store
            .proposal(id)
            .await
            .map_err(store_status)?
            .and_then(|proposal| {
                proposal
                    .get("status")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            });
        if open.as_deref() != Some("open") {
            return Err(Problem::new(
                StatusCode::CONFLICT,
                "conflict",
                "this proposal is no longer pending",
            ));
        }
        let value = crate::import::accept_import(&state, &identity, session_id, &body).await?;
        return Ok(AcceptReply::Job(value));
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
    let selected = selected_rows(&rows, &body.rows);
    validate_accept_patches(&state, &selected, &body.patches).await?;
    if selected.is_empty() {
        if proposal.get("status").and_then(Value::as_str) == Some("open") {
            close_open_proposal_without_pending(&state, id).await?;
        }
        return Err(Problem::new(
            StatusCode::CONFLICT,
            "conflict",
            "this proposal is no longer pending",
        ));
    }
    let patches = body.patches;
    // A short lease, not a pooled connection. The background job keeps the
    // claim and renews it on each row. A second accept loses the claim.
    let lock = state.store.claim_proposal(id).await.map_err(store_status)?;
    let proposal = match state.store.proposal(id).await {
        Ok(Some(proposal)) => proposal,
        Ok(None) => {
            lock.release().await.map_err(store_status)?;
            return Err(Problem::new(StatusCode::NOT_FOUND, "not_found", "proposal"));
        }
        Err(error) => {
            lock.release().await.map_err(store_status)?;
            return Err(store_status(error));
        }
    };
    let rows = proposal
        .get("rows")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let selected = selected_rows(&rows, &body.rows);
    if selected.is_empty() {
        if proposal.get("status").and_then(Value::as_str) == Some("open") {
            let claim = lock.token();
            let closed = close_claimed_proposal(&state, id, &proposal, claim).await;
            let released = lock.release().await;
            closed?;
            released.map_err(store_status)?;
        } else {
            lock.release().await.map_err(store_status)?;
        }
        return Err(Problem::new(
            StatusCode::CONFLICT,
            "conflict",
            "this proposal is no longer pending",
        ));
    }
    if selected.len() >= LARGE_ACCEPT {
        return accept_later(state, identity, id, selected, patches, lock).await;
    }
    let value = apply_selected(&state, &identity, id, selected, None, &patches, lock).await?;
    if let Some(problem) = refusal_from_accept(&value) {
        return Err(problem);
    }
    Ok(AcceptReply::Sync(value))
}

const LARGE_ACCEPT: usize = 8;

enum AcceptReply {
    Sync(Value),
    Job(Value),
}

impl IntoResponse for AcceptReply {
    fn into_response(self) -> Response {
        match self {
            Self::Sync(value) => Json(value).into_response(),
            Self::Job(value) => (StatusCode::ACCEPTED, Json(value)).into_response(),
        }
    }
}

/// A crash can leave every row finished and the proposal still `open`.
/// The next accept closes it, then reports that nothing was pending.
async fn close_open_proposal_without_pending(state: &AppState, id: Uuid) -> Result<(), Problem> {
    let lock = match state.store.claim_proposal(id).await {
        Ok(lock) => lock,
        Err(braian_store::Error::ProposalBusy(_)) => return Ok(()),
        Err(braian_store::Error::ProposalTransition { .. }) => return Ok(()),
        Err(error) => return Err(store_status(error)),
    };
    let proposal = match state.store.proposal(id).await {
        Ok(Some(proposal)) => proposal,
        Ok(None) => {
            lock.release().await.map_err(store_status)?;
            return Err(Problem::new(StatusCode::NOT_FOUND, "not_found", "proposal"));
        }
        Err(error) => {
            lock.release().await.map_err(store_status)?;
            return Err(store_status(error));
        }
    };
    let claim = lock.token();
    let closed = close_claimed_proposal(state, id, &proposal, claim).await;
    let released = lock.release().await;
    closed?;
    released.map_err(store_status)?;
    Ok(())
}

async fn close_claimed_proposal(
    state: &AppState,
    id: Uuid,
    proposal: &Value,
    claim: Uuid,
) -> Result<(), Problem> {
    if proposal.get("status").and_then(Value::as_str) != Some("open") {
        return Ok(());
    }
    let rows = proposal.get("rows").and_then(Value::as_array);
    let Some(rows) = rows else {
        return Ok(());
    };
    if rows.is_empty()
        || rows
            .iter()
            .any(|row| row.get("status").and_then(Value::as_str) == Some("pending"))
    {
        return Ok(());
    }
    let accepted = rows
        .iter()
        .any(|row| row.get("status").and_then(Value::as_str) == Some("accepted"));
    let status = if accepted { "accepted" } else { "discarded" };
    state
        .store
        .set_proposal_status(id, status, claim, false)
        .await
        .map_err(store_status)?;
    Ok(())
}

fn selected_rows(rows: &[Value], only: &Option<Vec<Uuid>>) -> Vec<Value> {
    rows.iter()
        .filter(|row| {
            let Some(row_id) = row
                .get("id")
                .and_then(Value::as_str)
                .and_then(|id| Uuid::parse_str(id).ok())
            else {
                return false;
            };
            if row.get("status").and_then(Value::as_str) != Some("pending") {
                return false;
            }
            match only {
                Some(ids) if !ids.is_empty() => ids.contains(&row_id),
                _ => true,
            }
        })
        .cloned()
        .collect()
}

async fn accept_later(
    state: AppState,
    identity: Identity,
    proposal_id: Uuid,
    selected: Vec<Value>,
    patches: BTreeMap<Uuid, Value>,
    lock: ProposalLock,
) -> Result<AcceptReply, Problem> {
    state
        .store
        .register_job_type("proposal.accept", "Accept proposal", &[], None)
        .await
        .map_err(store_status)?;
    let job_id = Uuid::now_v7();
    let total = i32::try_from(selected.len()).unwrap_or(i32::MAX);
    if let Err(error) = state
        .store
        .create_job_run(
            job_id,
            "proposal.accept",
            None,
            identity.actor.actor,
            identity.actor.on_behalf_of,
            total,
        )
        .await
    {
        lock.release().await.map_err(store_status)?;
        return Err(store_status(error));
    }
    emit_job(&state, job_id, "running", 0, total);
    let job_state = state.clone();
    tokio::spawn(async move {
        match apply_selected(
            &job_state,
            &identity,
            proposal_id,
            selected,
            Some(job_id),
            &patches,
            lock,
        )
        .await
        {
            Ok(_) => {
                let _ = job_state
                    .store
                    .finish_job_run(job_id, "succeeded", total, "accepted", &json!([]))
                    .await;
                emit_job(&job_state, job_id, "succeeded", total, total);
            }
            Err(problem) => {
                let _ = job_state
                    .store
                    .finish_job_run(
                        job_id,
                        "failed",
                        0,
                        &problem.detail,
                        &json!([{ "detail": problem.detail }]),
                    )
                    .await;
                emit_job(&job_state, job_id, "failed", 0, total);
            }
        }
    });
    Ok(AcceptReply::Job(
        json!({ "id": job_id, "status": "running", "total": total }),
    ))
}

async fn apply_selected(
    state: &AppState,
    identity: &Identity,
    proposal_id: Uuid,
    rows: Vec<Value>,
    job_id: Option<Uuid>,
    patches: &BTreeMap<Uuid, Value>,
    lock: ProposalLock,
) -> Result<Value, Problem> {
    let claim = lock.token();
    let outcome =
        apply_selected_locked(state, identity, proposal_id, rows, job_id, patches, claim).await;
    let status_result = match outcome.as_ref() {
        Ok(tally)
            if tally.failed.is_empty() && !(tally.accepted.is_empty() && tally.rejected == 0) =>
        {
            let status = if tally.accepted.is_empty() && tally.rejected > 0 {
                "discarded"
            } else {
                "accepted"
            };
            state
                .store
                .set_proposal_status(proposal_id, status, claim, false)
                .await
        }
        _ => Ok(()),
    };
    let release_result = lock.release().await;
    let tally = outcome?;
    if tally.accepted.is_empty() && tally.failed.is_empty() && tally.rejected == 0 {
        return Err(Problem::new(
            StatusCode::CONFLICT,
            "conflict",
            "this proposal is no longer pending",
        ));
    }
    status_result.map_err(store_status)?;
    release_result.map_err(store_status)?;
    Ok(json!({ "accepted": tally.accepted, "failed": tally.failed }))
}

struct AcceptTally {
    accepted: Vec<Value>,
    failed: Vec<Value>,
    rejected: i32,
}

async fn apply_selected_locked(
    state: &AppState,
    identity: &Identity,
    proposal_id: Uuid,
    rows: Vec<Value>,
    job_id: Option<Uuid>,
    patches: &BTreeMap<Uuid, Value>,
    claim: Uuid,
) -> Result<AcceptTally, Problem> {
    let total = i32::try_from(rows.len()).unwrap_or(i32::MAX);
    let mut done = 0i32;
    let mut accepted = Vec::new();
    let mut failed = Vec::new();
    let mut rejected = 0i32;
    for row in rows {
        let Some(row_id) = row_uuid(&row) else {
            continue;
        };
        let mut row = row;
        if let Some(replacement) = patches.get(&row_id) {
            if patch_is_empty(replacement) {
                if finish_row(
                    state,
                    identity,
                    proposal_id,
                    row_id,
                    "rejected",
                    ProposalWrite::None,
                    claim,
                )
                .await?
                .is_some()
                {
                    rejected += 1;
                }
                done += 1;
                if let Some(job_id) = job_id {
                    let _ = state
                        .store
                        .update_job_progress(
                            job_id,
                            done,
                            &format!("{done} of {total}"),
                            &json!([]),
                        )
                        .await;
                    emit_job(state, job_id, "running", done, total);
                }
                continue;
            }
            if let Some(object) = row.as_object_mut() {
                object.insert("patch".to_owned(), replacement.clone());
            }
        }
        match apply_row(state, identity, proposal_id, row_id, &row, claim).await {
            Ok(Some(seq)) => accepted.push(json!({ "id": row_id, "seq": seq })),
            Ok(None) => {}
            Err(problem) if problem.code == "claim_lost" => return Err(problem),
            Err(problem) => {
                let detail = problem
                    .errors
                    .first()
                    .map(|error| error.message.clone())
                    .unwrap_or_else(|| problem.detail.clone());
                let mut failure = Map::new();
                failure.insert("id".to_owned(), json!(row_id));
                failure.insert("code".to_owned(), json!(problem.code));
                failure.insert("detail".to_owned(), json!(detail));
                if let Some(version) = problem.current_version {
                    failure.insert("current_version".to_owned(), json!(version));
                }
                failed.push(Value::Object(failure));
            }
        }
        done += 1;
        if let Some(job_id) = job_id {
            let _ = state
                .store
                .update_job_progress(job_id, done, &format!("{done} of {total}"), &json!([]))
                .await;
            emit_job(state, job_id, "running", done, total);
        }
    }
    Ok(AcceptTally {
        accepted,
        failed,
        rejected,
    })
}

fn row_uuid(row: &Value) -> Option<Uuid> {
    row.get("id")
        .and_then(Value::as_str)
        .and_then(|id| Uuid::parse_str(id).ok())
}

/// A single-row conflict is the accept response itself. A mixed batch still
/// returns the rows that did apply.
fn refusal_from_accept(value: &Value) -> Option<Problem> {
    let accepted = value.get("accepted")?.as_array()?;
    if !accepted.is_empty() {
        return None;
    }
    let failed = value.get("failed")?.as_array()?;
    let row = failed.iter().find(|item| {
        matches!(
            item.get("code").and_then(Value::as_str),
            Some("conflict" | "base_ahead" | "base_version_required")
        )
    })?;
    let code = row.get("code").and_then(Value::as_str)?;
    let detail = row
        .get("detail")
        .and_then(Value::as_str)
        .unwrap_or("conflict");
    let status = if code == "base_version_required" {
        StatusCode::BAD_REQUEST
    } else {
        StatusCode::CONFLICT
    };
    let mut problem = Problem::new(status, code, detail);
    problem.current_version = row.get("current_version").and_then(Value::as_u64);
    Some(problem)
}

async fn validate_accept_patches(
    state: &AppState,
    rows: &[Value],
    patches: &BTreeMap<Uuid, Value>,
) -> Result<(), Problem> {
    if patches.is_empty() {
        return Ok(());
    }
    for row in rows {
        let Some(row_id) = row_uuid(row) else {
            continue;
        };
        let Some(replacement) = patches.get(&row_id) else {
            continue;
        };
        if patch_is_empty(replacement) {
            continue;
        }
        let original = row.get("patch").unwrap_or(&Value::Null);
        let current = current_wire_values(state, row).await?;
        validate_replacement(original, replacement, current.as_ref())?;
    }
    Ok(())
}

async fn current_wire_values(
    state: &AppState,
    row: &Value,
) -> Result<Option<Map<String, Value>>, Problem> {
    let Some(text) = row.get("object_id").and_then(Value::as_str) else {
        return Ok(None);
    };
    let id =
        ObjectId::from_uuid(Uuid::parse_str(text).map_err(|error| {
            Problem::new(StatusCode::BAD_REQUEST, "invalid_id", error.to_string())
        })?);
    let Some(stored) = state.store.get_object(id).await.map_err(store_status)? else {
        return Ok(None);
    };
    let cached = state.models.get(&state.store).await?;
    let applicable_ids = applicable(&cached, &stored.object);
    let wire = values_to_wire(&cached.model, &applicable_ids, &stored.object.values, false);
    Ok(Some(wire.into_iter().collect()))
}

fn patch_not_proposed(detail: impl Into<String>) -> Problem {
    Problem::new(
        StatusCode::UNPROCESSABLE_ENTITY,
        "patch_not_proposed",
        detail,
    )
}

/// A replacement may keep or drop proposed fields and values. Anything that
/// was not in the proposal is rejected. An empty `values` object is handled
/// earlier and means the whole row is rejected.
fn validate_replacement(
    original: &Value,
    replacement: &Value,
    current_values: Option<&Map<String, Value>>,
) -> Result<(), Problem> {
    let Some(object) = replacement.as_object() else {
        return Err(patch_not_proposed("an accept patch must be an object"));
    };
    for key in object.keys() {
        if key != "values" && key != "groups" {
            return Err(patch_not_proposed(format!("field {key} was not proposed")));
        }
    }
    let proposed_values = original.get("values").and_then(Value::as_object);
    if let Some(values) = object.get("values") {
        let Some(values) = values.as_object() else {
            return Err(patch_not_proposed("values must be an object"));
        };
        for (key, value) in values {
            let Some(proposed) = proposed_values.and_then(|map| map.get(key)) else {
                return Err(patch_not_proposed(format!("field {key} was not proposed")));
            };
            let current = current_values.and_then(|map| map.get(key));
            ensure_proposed_value(key, proposed, value, current)?;
        }
    }
    let proposed_groups = original.get("groups").and_then(Value::as_object);
    if let Some(groups) = object.get("groups") {
        let Some(groups) = groups.as_object() else {
            return Err(patch_not_proposed("groups must be an object"));
        };
        for (key, value) in groups {
            match proposed_groups.and_then(|map| map.get(key)) {
                Some(proposed) if proposed == value => {}
                Some(_) => {
                    return Err(patch_not_proposed(format!(
                        "group {key} does not match the proposal"
                    )));
                }
                None => {
                    return Err(patch_not_proposed(format!("group {key} was not proposed")));
                }
            }
        }
    }
    Ok(())
}

fn ensure_proposed_value(
    field: &str,
    proposed: &Value,
    replacement: &Value,
    current: Option<&Value>,
) -> Result<(), Problem> {
    if let (Value::Object(proposed_map), Value::Object(replacement_map)) = (proposed, replacement) {
        for (key, value) in replacement_map {
            match proposed_map.get(key) {
                Some(proposed_value) if proposed_value == value => {}
                Some(_) => {
                    return Err(patch_not_proposed(format!(
                        "value for {field}.{key} was not proposed"
                    )));
                }
                None => {
                    return Err(patch_not_proposed(format!(
                        "{field}.{key} was not proposed"
                    )));
                }
            }
        }
        return Ok(());
    }
    if let (Value::Array(proposed_items), Value::Array(replacement_items)) = (proposed, replacement)
    {
        let current_items = current.and_then(Value::as_array);
        if list_keeps_or_rejects(
            current_items.map(Vec::as_slice).unwrap_or(&[]),
            proposed_items,
            replacement_items,
        ) {
            return Ok(());
        }
        return Err(patch_not_proposed(format!(
            "a value in {field} was not proposed"
        )));
    }
    if proposed == replacement {
        return Ok(());
    }
    Err(patch_not_proposed(format!(
        "value for {field} was not proposed"
    )))
}

/// Membership edits may only keep or drop items the proposal added or removed.
/// Items in both the current value and the proposal stay. A new code is rejected.
fn list_keeps_or_rejects(current: &[Value], proposed: &[Value], replacement: &[Value]) -> bool {
    let current = value_counts(current);
    let proposed = value_counts(proposed);
    let replacement = value_counts(replacement);
    let mut keys = BTreeSet::new();
    keys.extend(current.keys().cloned());
    keys.extend(proposed.keys().cloned());
    keys.extend(replacement.keys().cloned());
    keys.iter().all(|key| {
        let had = current.get(key).copied().unwrap_or(0);
        let wanted = proposed.get(key).copied().unwrap_or(0);
        let kept = replacement.get(key).copied().unwrap_or(0);
        kept >= had.min(wanted) && kept <= had.max(wanted)
    })
}

fn value_counts(items: &[Value]) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for item in items {
        let key = serde_json::to_string(item).unwrap_or_default();
        *counts.entry(key).or_default() += 1;
    }
    counts
}

fn patch_is_empty(patch: &Value) -> bool {
    let values_empty = match patch.get("values") {
        Some(Value::Object(map)) => map.is_empty(),
        Some(_) => false,
        None => true,
    };
    let groups_empty = match patch.get("groups") {
        Some(Value::Object(map)) => map.is_empty(),
        Some(_) => false,
        None => true,
    };
    values_empty && groups_empty
}

/// A relation suggestion stored before link types. Accepting it writes a link
/// when a link type with that key exists. Value patches are not relations.
fn relation_proposal(row: &Value) -> Option<(Uuid, String)> {
    let patch = row.get("patch")?;
    let nested = patch.get("relation");
    if nested.is_none() && (patch.get("values").is_some() || patch.get("to").is_none()) {
        return None;
    }
    let source = nested.unwrap_or(patch);
    let to = source
        .get("to")
        .and_then(Value::as_str)
        .and_then(|id| Uuid::parse_str(id).ok())?;
    let kind = match source
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or("related")
    {
        kind @ ("variant_of" | "article_of" | "related") => kind,
        _ => "related",
    };
    Some((to, kind.to_owned()))
}

async fn apply_row(
    state: &AppState,
    identity: &Identity,
    proposal_id: Uuid,
    row_id: Uuid,
    row: &Value,
    claim: Uuid,
) -> Result<Option<i64>, Problem> {
    if crate::model_proposal::is_model_row(row) {
        return crate::model_proposal::apply_row(state, identity, proposal_id, row_id, row, claim)
            .await;
    }
    if row
        .get("patch")
        .and_then(|patch| patch.get("link"))
        .is_some()
    {
        return apply_link_row(state, identity, proposal_id, row_id, row, claim).await;
    }
    if row.get("type_key").and_then(Value::as_str) == Some("assortment") {
        return apply_assortment_row(state, identity, proposal_id, row_id, row, claim).await;
    }
    if let Some((to, kind)) = relation_proposal(row) {
        return accept_legacy_relation(state, identity, proposal_id, row_id, row, to, &kind, claim)
            .await;
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
        let Some(base) = row
            .get("base_version")
            .and_then(Value::as_i64)
            .filter(|base| *base >= 0)
        else {
            return Err(base_version_required());
        };
        let base = base as u64;
        let Some(receipt) = finish_row(
            state,
            identity,
            proposal_id,
            row_id,
            "accepted",
            ProposalWrite::Objects {
                ops: vec![ObjectOp::Update {
                    id,
                    patch,
                    base_version: base,
                    read_version: base,
                }],
                expect_version: Some((id, base)),
            },
            claim,
        )
        .await?
        else {
            return Ok(None);
        };
        state.emit(json!({
            "seq": receipt.last_seq,
            "op": "update",
            "object_id": id.as_uuid(),
            "version": receipt.versions.first().map(|(_, version)| *version).unwrap_or(0),
            "type": type_key,
        }));
        state.index_after_commit(id.as_uuid(), receipt.last_seq);
        return Ok(Some(receipt.last_seq));
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
    let Some(receipt) = finish_row(
        state,
        identity,
        proposal_id,
        row_id,
        "accepted",
        ProposalWrite::Objects {
            ops: vec![op],
            expect_version: None,
        },
        claim,
    )
    .await?
    else {
        return Ok(None);
    };
    state.emit(json!({ "seq": receipt.last_seq, "op": "create", "type": type_key }));
    if let Some((id, _)) = receipt.versions.first() {
        state.index_after_commit(id.as_uuid(), receipt.last_seq);
    }
    Ok(Some(receipt.last_seq))
}

/// `Ok(None)` means another accept already took the row. The status change and the
/// write commit together, so a failure leaves the row `pending`.
async fn finish_row(
    state: &AppState,
    identity: &Identity,
    proposal_id: Uuid,
    row_id: Uuid,
    status: &str,
    write: ProposalWrite,
    claim: Uuid,
) -> Result<Option<WriteReceipt>, Problem> {
    state
        .store
        .commit_proposal_row(&identity.actor, proposal_id, row_id, status, write, claim)
        .await
        .map_err(proposal_write_error)
}

async fn apply_link_row(
    state: &AppState,
    identity: &Identity,
    proposal_id: Uuid,
    row_id: Uuid,
    row: &Value,
    claim: Uuid,
) -> Result<Option<i64>, Problem> {
    let cached = state.models.get(&state.store).await?;
    let link = row
        .get("patch")
        .and_then(|patch| patch.get("link"))
        .cloned()
        .unwrap_or_else(|| json!({}));
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
        let Some(base) = row
            .get("base_version")
            .and_then(Value::as_i64)
            .filter(|base| *base >= 0)
        else {
            return Err(base_version_required());
        };
        let body = crate::links::patch_body(&link)?;
        let op = crate::links::link_update(&cached, &stored, &body, base as u64)?;
        let Some(receipt) = finish_row(
            state,
            identity,
            proposal_id,
            row_id,
            "accepted",
            ProposalWrite::Objects {
                ops: vec![op],
                expect_version: Some((id, base as u64)),
            },
            claim,
        )
        .await?
        else {
            return Ok(None);
        };
        state.emit(json!({ "seq": receipt.last_seq, "op": "update", "object_id": id.as_uuid() }));
        state.index_after_commit(id.as_uuid(), receipt.last_seq);
        return Ok(Some(receipt.last_seq));
    }
    let body = crate::links::create_body(&link)?;
    let op = crate::links::link_create(&cached, &body)?;
    let Some(receipt) = finish_row(
        state,
        identity,
        proposal_id,
        row_id,
        "accepted",
        ProposalWrite::Objects {
            ops: vec![op],
            expect_version: None,
        },
        claim,
    )
    .await?
    else {
        return Ok(None);
    };
    state.emit(json!({ "seq": receipt.last_seq, "op": "create" }));
    if let Some((id, _)) = receipt.versions.first() {
        state.index_after_commit(id.as_uuid(), receipt.last_seq);
    }
    Ok(Some(receipt.last_seq))
}

fn agent_cannot_discard() -> Problem {
    Problem::new(
        StatusCode::FORBIDDEN,
        "agent_cannot_discard",
        "an agent or import proposes; a person discards",
    )
}

fn proposal_write_error(error: braian_store::Error) -> Problem {
    match error {
        braian_store::Error::Conflict {
            current_version, ..
        } => {
            let mut problem = Problem::new(
                StatusCode::CONFLICT,
                "conflict",
                format!(
                    "proposal base_version is older than the object's version {current_version}; the object was not changed"
                ),
            );
            problem.current_version = Some(current_version);
            problem
        }
        braian_store::Error::BaseAhead { current_version } => {
            let mut problem = Problem::new(
                StatusCode::CONFLICT,
                "base_ahead",
                format!(
                    "proposal base_version is ahead of the object's version {current_version}; the object was not changed"
                ),
            );
            problem.current_version = Some(current_version);
            problem
        }
        other => store_status(other),
    }
}

async fn discard_proposal(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, Problem> {
    require(&identity, Role::Editor)?;
    if !identity.may_commit() {
        return Err(agent_cannot_discard());
    }
    if let Some(session_id) = state
        .store
        .proposal_session(id)
        .await
        .map_err(store_status)?
    {
        crate::import::discard_import(&state, session_id).await?;
        return Ok(StatusCode::NO_CONTENT);
    }
    let lock = state.store.claim_proposal(id).await.map_err(store_status)?;
    let closed = state
        .store
        .set_proposal_status(id, "discarded", lock.token(), true)
        .await;
    let released = lock.release().await;
    closed.map_err(store_status)?;
    released.map_err(store_status)?;
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

#[derive(Deserialize, ToSchema)]
pub(crate) struct NamesBody {
    #[serde(default)]
    ids: Vec<String>,
}

const NAME_BATCH_LIMIT: usize = 400;

async fn object_names(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Json(body): Json<NamesBody>,
) -> Result<Json<Value>, Problem> {
    require(&identity, Role::Viewer)?;
    if body.ids.len() > NAME_BATCH_LIMIT {
        return Err(Problem::new(
            StatusCode::BAD_REQUEST,
            "too_many_ids",
            "at most 400 ids",
        ));
    }
    let mut ids = Vec::with_capacity(body.ids.len());
    let mut seen = BTreeSet::new();
    for raw in &body.ids {
        let uuid = Uuid::parse_str(raw).map_err(|error| {
            Problem::new(StatusCode::BAD_REQUEST, "invalid_id", error.to_string())
        })?;
        if seen.insert(uuid) {
            ids.push(ObjectId::from_uuid(uuid));
        }
    }
    let cached = state.models.get(&state.store).await?;
    let name_ids: Vec<_> = [braian_model::name_id()]
        .into_iter()
        .filter(|id| cached.model.attribute(*id).is_some())
        .chain(
            cached
                .model
                .attribute_by_key("name")
                .map(|attribute| attribute.id),
        )
        .collect();
    // The graph asks for names in batches. Load the display-name parts; the
    // object key and type come from the object row.
    let projection = if name_ids.is_empty() {
        ObjectProjection {
            groups: false,
            values: false,
            parts: None,
        }
    } else {
        let parts = state
            .store
            .parts_for_attributes(&name_ids, None)
            .await
            .map_err(store_status)?;
        ObjectProjection {
            groups: false,
            values: true,
            parts: Some(parts),
        }
    };
    let stored = state
        .store
        .get_objects_projected(&ids, &projection)
        .await
        .map_err(store_status)?;
    let mut objects = Vec::with_capacity(stored.len());
    for item in stored {
        if item.deleted_at.is_some() {
            continue;
        }
        let name = name_ids
            .iter()
            .find_map(|id| {
                let text = item
                    .object
                    .values
                    .get(id)
                    .map(display_name_text)
                    .unwrap_or_default();
                if text.trim().is_empty() {
                    None
                } else {
                    Some(text)
                }
            })
            .unwrap_or_default();
        objects.push(json!({
            "id": item.object.id.as_uuid(),
            "key": item.key,
            "type": wire::type_key(&cached.defs, item.object.type_id),
            "name": name,
        }));
    }
    Ok(Json(json!({ "objects": objects })))
}

/// English name when the value has one, otherwise another stored language.
/// A measurement or an empty value yields an empty string, never the object key.
fn display_name_text(value: &Value) -> String {
    if let Some(text) = value.as_str() {
        return text.trim().to_string();
    }
    let Some(map) = value.as_object() else {
        return String::new();
    };
    for tag in ["en", "sv", "de"] {
        if let Some(text) = map.get(tag).and_then(Value::as_str) {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                return trimmed.to_string();
            }
        }
    }
    for (key, inner) in map {
        if matches!(key.as_str(), "value" | "unit" | "min" | "max") {
            continue;
        }
        if let Some(text) = inner.as_str() {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                return trimmed.to_string();
            }
        }
    }
    String::new()
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
    let link_views = state
        .store
        .links_for(ObjectId::from_uuid(id))
        .await
        .map_err(store_status)?;
    let mut links = Vec::with_capacity(link_views.len());
    for link in &link_views {
        let link_type = cached
            .defs
            .types
            .iter()
            .find(|item| item.id == link.link_type_id);
        let spec = link_type.and_then(|item| item.link.as_ref());
        let label = spec
            .map(|spec| {
                if link.outgoing {
                    spec.to_label.text("en")
                } else {
                    spec.from_label.text("en")
                }
            })
            .unwrap_or_else(|| "Link".to_owned());
        links.push(json!({
            "id": link.id,
            "link_type": link_type.map(|item| item.key.as_str()).unwrap_or(""),
            "label": label,
            "object_id": link.peer_id,
            "key": link.peer_key,
            "type_id": link.peer_type_id,
            "direction": if link.outgoing { "outgoing" } else { "incoming" },
        }));
    }
    let entries = state
        .store
        .history(Some(id), None, 2000)
        .await
        .map_err(store_status)?;
    let effective = cached
        .model
        .effective_attributes(stored.object.type_id, &stored.object.groups)
        .unwrap_or_default();
    let mut attributes = Vec::with_capacity(effective.len());
    for item in effective {
        let Some(attribute) = cached.model.attribute(item.id) else {
            continue;
        };
        let sources = attribute_sources(&cached.defs, &stored.object, item.id, &item.sources);
        let last = entries.iter().find(|entry| {
            entry
                .diff
                .as_ref()
                .and_then(Value::as_object)
                .is_some_and(|diff| diff.contains_key(&item.id.as_uuid().to_string()))
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
        "links": links,
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

/// Sources in the same shape the graph has always returned, with `inherited`
/// taken from [`braian_rules::CompiledModel::effective_attributes`].
///
/// A type source is the object's own type. A group source names the node that
/// defines the attribute. `inherited` is true when that node is an ancestor of
/// the group the object is placed in.
fn attribute_sources(
    defs: &DefinitionSet,
    object: &Object,
    attribute_id: AttributeId,
    sources: &[braian_rules::AttributeSource],
) -> Vec<Value> {
    let mut rows = Vec::with_capacity(sources.len());
    for source in sources {
        match source {
            braian_rules::AttributeSource::Type { key } => {
                rows.push(json!({
                    "from": "type",
                    "type": key,
                    "structure": Value::Null,
                    "class": Value::Null,
                    "group": Value::Null,
                    "inherited": false,
                }));
            }
            braian_rules::AttributeSource::Group { structure, group } => {
                push_group_source(&mut rows, defs, object, attribute_id, structure, group);
            }
        }
    }
    rows
}

fn push_group_source(
    rows: &mut Vec<Value>,
    defs: &DefinitionSet,
    object: &Object,
    attribute_id: AttributeId,
    structure_key: &str,
    group_key: &str,
) {
    let structure = defs
        .structures
        .iter()
        .find(|item| item.key == structure_key);
    let defined_on = structure.and_then(|item| {
        item.groups
            .iter()
            .find(|group| group.key == group_key)
            .map(|group| group.id)
    });
    let placed = structure.and_then(|item| object.groups.get(&item.id).copied());
    let inherited = match (structure, defined_on, placed) {
        (Some(structure), Some(defined_on), Some(placed)) => {
            defined_on != placed && covers(structure, defined_on, placed)
        }
        _ => false,
    };
    let classes: Vec<&str> = structure
        .into_iter()
        .flat_map(|item| item.classes.iter())
        .filter(|class| defined_on.is_some_and(|id| class.group == id))
        .filter(|class| class.attributes.contains(&attribute_id))
        .map(|class| class.key.as_str())
        .collect();
    if classes.is_empty() {
        rows.push(json!({
            "from": "group",
            "structure": structure_key,
            "class": Value::Null,
            "group": group_key,
            "inherited": inherited,
        }));
        return;
    }
    for class_key in classes {
        rows.push(json!({
            "from": "group",
            "structure": structure_key,
            "class": class_key,
            "group": group_key,
            "inherited": inherited,
        }));
    }
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

// `to` and `kind` are already parsed from the row; the claim still has to be explicit.
#[expect(clippy::too_many_arguments)]
async fn accept_legacy_relation(
    state: &AppState,
    identity: &Identity,
    proposal_id: Uuid,
    row_id: Uuid,
    row: &Value,
    to: Uuid,
    kind: &str,
    claim: Uuid,
) -> Result<Option<i64>, Problem> {
    let from = row
        .get("object_id")
        .and_then(Value::as_str)
        .and_then(|id| Uuid::parse_str(id).ok())
        .ok_or_else(|| Problem::new(StatusCode::BAD_REQUEST, "invalid_id", "relation"))?;
    let cached = state.models.get(&state.store).await?;
    let link_type = cached
        .defs
        .types
        .iter()
        .find(|item| item.is_link() && item.key == kind)
        .ok_or_else(|| {
            Problem::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                "unknown_link_type",
                format!("no link type {kind}"),
            )
        })?;
    let body = crate::links::CreateLink {
        id: None,
        link_type: link_type.key.clone(),
        from_id: from,
        to_id: to,
        values: BTreeMap::new(),
    };
    let op = crate::links::link_create(&cached, &body)?;
    let receipt = finish_row(
        state,
        identity,
        proposal_id,
        row_id,
        "accepted",
        ProposalWrite::Objects {
            ops: vec![op],
            expect_version: None,
        },
        claim,
    )
    .await?;
    Ok(receipt.map(|receipt| receipt.last_seq))
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
    if body.get("object_id").and_then(Value::as_str).is_none() {
        return false;
    }
    let Some(type_id) = saved.get("type_id").and_then(Value::as_str) else {
        return false;
    };
    let Some(event_type) = body.get("type").and_then(Value::as_str) else {
        return false;
    };
    if event_type == type_id {
        return true;
    }
    let Ok(parsed) = Uuid::parse_str(type_id) else {
        return false;
    };
    let Ok(defs) = store.definition_set().await else {
        return false;
    };
    wire::type_key(&defs, TypeId::from_uuid(parsed)) == event_type
}

fn sign_webhook(secret: &str, payload: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(secret.as_bytes());
    hasher.update(b".");
    hasher.update(payload.as_bytes());
    format!("sha256={}", hex::encode(hasher.finalize()))
}

async fn wait_for_seq(store: &Store, seq: i64) {
    // Back off. A tight poll while the indexer is importing shares the
    // database and, once the seq is reached, the following search shares
    // Typesense with that import.
    let start = std::time::Instant::now();
    let mut pause = Duration::from_millis(50);
    while start.elapsed() < Duration::from_secs(3) {
        if store.indexer_seq().await.unwrap_or(0) >= seq {
            return;
        }
        tokio::time::sleep(pause).await;
        pause = (pause * 2).min(Duration::from_millis(400));
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

/// The service principal must not accept, discard or delete.
///
/// The row is [`braian_store::SYSTEM_PRINCIPAL`]. A person who has the handle
/// `system` is a different row and is not what this checks. The kind is read
/// from `principals`. A human row would pass `may_commit`.
#[doc(hidden)]
pub async fn stored_principal_cannot_approve_discard_or_delete(
    store: &Store,
) -> Result<(), String> {
    let record = store
        .principal(braian_store::SYSTEM_PRINCIPAL)
        .await
        .map_err(|error| error.to_string())?
        .ok_or("system principal is missing")?;
    if record.kind != braian_store::PrincipalKind::Integration {
        return Err(format!("system principal kind is {}", record.kind.as_str()));
    }
    if record.name != "System" {
        return Err(format!("system principal name is {}", record.name));
    }
    let identity = Identity::from_principal(&record);
    let state = AppState::new(store.clone(), Vec::new(), None);
    let object_id = Uuid::nil();
    for (name, code) in [
        ("accept_proposal", "agent_cannot_accept"),
        ("discard_proposal", "agent_cannot_discard"),
        ("delete_object", "agent_cannot_delete"),
    ] {
        match run_operation(&state, &identity, name, &json!({ "id": object_id }), false).await {
            Ok(value) => return Err(format!("{name} was allowed: {value}")),
            Err(problem) => {
                if problem.status != StatusCode::FORBIDDEN.as_u16() || problem.code != code {
                    return Err(format!(
                        "{name} returned {} {}",
                        problem.status, problem.code
                    ));
                }
            }
        }
    }
    Ok(())
}

fn uuid_arg(args: &Value, key: &str) -> Result<Uuid, Problem> {
    args.get(key)
        .and_then(Value::as_str)
        .and_then(|value| Uuid::parse_str(value).ok())
        .ok_or_else(|| {
            Problem::new(
                StatusCode::BAD_REQUEST,
                "missing_id",
                format!("name the {key}"),
            )
        })
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
    if let Some(value) = context_tool_result(op.name, args, None) {
        return Ok(value);
    }
    if op.name.starts_with("ui.") || is_layout_command(op.name) {
        return Ok(ui_command(op.name, args));
    }
    // The in-app agent calls this with the person's identity (`from_agent`).
    // Hiding the tool is not enough: a call still has to stay a proposal.
    if op.name == "accept_proposal" && (from_agent || !identity.may_commit()) {
        return Err(Problem::new(
            StatusCode::FORBIDDEN,
            "agent_cannot_accept",
            "an agent or import proposes; a person accepts",
        ));
    }
    if op.name == "discard_proposal" && (from_agent || !identity.may_commit()) {
        return Err(agent_cannot_discard());
    }
    if op.name == "model.undo" && (from_agent || !identity.may_commit()) {
        return Err(crate::auth::delete_forbidden());
    }
    if operation_deletes(op.name) && (from_agent || !identity.may_commit()) {
        return Err(crate::auth::delete_forbidden());
    }
    let agent_write = from_agent
        && matches!(
            op.name,
            "create_object" | "patch_object" | "move_to_group" | "create_link" | "update_link"
        );
    if op.needs_approval || agent_must_propose(identity, op) || agent_write {
        // Ask AI calls with from_agent=true and often omits base_version. Fill it
        // from the object head so the proposal still carries optimistic concurrency.
        let owned = if from_agent && matches!(op.name, "patch_object" | "move_to_group") {
            ensure_agent_base_version(state, args).await?
        } else {
            args.clone()
        };
        return propose_operation(state, identity, name, &owned).await;
    }
    match name {
        "model.list_attribute_definitions" => crate::attribute_catalog::list(state).await,
        "list_attribute_paths" => {
            let token = args
                .get("type")
                .or_else(|| args.get("id"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let cached = state.models.get(&state.store).await?;
            let type_id = wire::resolve_type(&cached.model, token)?;
            Ok(crate::definitions::attribute_path_list(
                &cached.defs,
                type_id,
            ))
        }
        "get_view_column_prefs" => {
            crate::column_prefs::get_view_column_prefs_op(state, identity, args).await
        }
        "put_view_column_prefs" => {
            crate::column_prefs::put_view_column_prefs_op(state, identity, args).await
        }
        "model.attribute_usage" => crate::attribute_catalog::usage(state, args).await,
        "model.undo" => crate::model_draft::undo_model(state, identity).await,
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
        "list_assortments" => {
            let workspace = args
                .get("workspace_type")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|token| !token.is_empty());
            let requested = args
                .get("type")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|token| !token.is_empty());
            let token = match (workspace, requested) {
                (Some(workspace), Some(requested)) => {
                    let cached = state.models.get(&state.store).await?;
                    let workspace_id = wire::resolve_type(&cached.model, workspace)?;
                    let requested_id = wire::resolve_type(&cached.model, requested)?;
                    if workspace_id != requested_id {
                        return Err(wrong_type());
                    }
                    Some(workspace.to_owned())
                }
                (Some(workspace), None) => Some(workspace.to_owned()),
                (None, Some(requested)) => Some(requested.to_owned()),
                (None, None) => None,
            };
            Ok(list_assortments(
                State(state.clone()),
                Auth(identity.clone()),
                Query(AssortmentTypeQuery { r#type: token }),
            )
            .await?
            .0)
        }
        "list_objects" => Ok(list_objects(
            State(state.clone()),
            Auth(identity.clone()),
            Query(ListParams {
                q: args.get("q").and_then(Value::as_str).map(str::to_owned),
                r#type: args.get("type").and_then(Value::as_str).map(str::to_owned),
                limit: args.get("limit").and_then(Value::as_i64),
                cursor: args
                    .get("cursor")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
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
            let audit = state
                .store
                .object_audits(&[id])
                .await
                .map_err(store_status)?
                .into_iter()
                .next();
            Ok(json!({
                "id": stored.object.id.as_uuid(),
                "type": wire::type_key(&cached.defs, stored.object.type_id),
                "key": stored.key,
                "version": stored.version,
                "groups": groups_to_wire(&cached.defs, &stored.object.groups),
                "values": values_to_wire(&cached.model, &applicable, &stored.object.values, false),
                "created_at": audit.as_ref().map(|audit| audit.created_at.to_rfc3339()),
                "updated_at": audit.as_ref().map(|audit| audit.updated_at.to_rfc3339()),
                "updated_by": audit.as_ref().and_then(|audit| {
                    Some(json!({
                        "id": audit.updated_by?.as_uuid(),
                        "name": audit.updated_by_name.clone()?,
                        "kind": audit.updated_by_kind?.as_str(),
                    }))
                }),
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
        "move_to_group" => {
            let structure = args.get("structure").and_then(Value::as_str).unwrap_or("");
            let group = args.get("group").cloned().unwrap_or(Value::Null);
            let mut groups = serde_json::Map::new();
            groups.insert(structure.to_owned(), group);
            let mut patched = args.clone();
            patched["groups"] = Value::Object(groups);
            // Same write as an object patch, including the proposal an agent files.
            Box::pin(run_operation(
                state,
                identity,
                "patch_object",
                &patched,
                from_agent,
            ))
            .await
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
        "export" => Ok(
            json!({ "hint": "POST /v1/export returns the xlsx file, or a job when the view is large" }),
        ),
        name if name.starts_with("import.") => {
            crate::import::run_agent_op(state, identity, name, args).await
        }
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
            Ok(
                match accept_proposal(
                    State(state.clone()),
                    Auth(identity.clone()),
                    Path(id),
                    Json(AcceptBody {
                        rows: Some(rows),
                        patches: BTreeMap::new(),
                        skip_failed: false,
                        only_rows: None,
                        exclude_rows: None,
                    }),
                )
                .await?
                {
                    AcceptReply::Sync(value) | AcceptReply::Job(value) => value,
                },
            )
        }
        "delete_object" => {
            let id = args
                .get("id")
                .and_then(Value::as_str)
                .and_then(|id| Uuid::parse_str(id).ok())
                .ok_or_else(|| {
                    Problem::new(
                        StatusCode::BAD_REQUEST,
                        "missing_id",
                        "name the object to delete",
                    )
                })?;
            state
                .store
                .write_batch(
                    &identity.actor,
                    vec![ObjectOp::Delete {
                        id: ObjectId::from_uuid(id),
                        base_version: None,
                    }],
                )
                .await
                .map_err(store_status)?;
            Ok(json!({ "ok": true }))
        }
        "list_links" => {
            let object = args
                .get("object")
                .and_then(Value::as_str)
                .and_then(|id| Uuid::parse_str(id).ok());
            let listed = crate::links::list_links(
                State(state.clone()),
                Auth(identity.clone()),
                Query(crate::links::LinksQuery { object }),
            )
            .await?;
            Ok(serde_json::to_value(&listed.0).unwrap_or_else(|_| json!([])))
        }
        "create_link" => {
            let body = crate::links::create_body(args)?;
            let response =
                crate::links::post_link(State(state.clone()), Auth(identity.clone()), Json(body))
                    .await?;
            Ok(json!({ "status": response.status().as_u16() }))
        }
        "update_link" => {
            let id = uuid_arg(args, "id")?;
            let body = crate::links::patch_body(args)?;
            let mut headers = HeaderMap::new();
            if let Some(version) = args.get("base_version").and_then(Value::as_u64) {
                headers.insert(
                    header::IF_MATCH,
                    header::HeaderValue::from_str(&version.to_string()).map_err(|error| {
                        Problem::new(StatusCode::BAD_REQUEST, "invalid_body", error.to_string())
                    })?,
                );
            }
            let response = crate::links::patch_link(
                State(state.clone()),
                Auth(identity.clone()),
                Path(id),
                headers,
                Json(body),
            )
            .await?;
            Ok(json!({ "status": response.status().as_u16() }))
        }
        "remove_link" => {
            let id = uuid_arg(args, "id")?;
            state
                .store
                .write_batch(
                    &identity.actor,
                    vec![ObjectOp::Delete {
                        id: ObjectId::from_uuid(id),
                        base_version: None,
                    }],
                )
                .await
                .map_err(store_status)?;
            Ok(json!({ "ok": true }))
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

/// When the in-app agent patches an existing object without `base_version`, use
/// the object's current version at proposal time. Accept still rejects a stale base.
///
/// A numeric string (`"3"`) is kept as that version. An invalid type or string is
/// rejected instead of being overwritten with the head.
async fn ensure_agent_base_version(state: &AppState, args: &Value) -> Result<Value, Problem> {
    match parse_provided_base_version(args)? {
        Some(version) => Ok(with_base_version(args, version)),
        None => {
            let Some(id) = args
                .get("id")
                .and_then(Value::as_str)
                .and_then(|text| Uuid::parse_str(text).ok())
                .map(ObjectId::from_uuid)
            else {
                return Ok(args.clone());
            };
            let Some(stored) = state.store.get_object(id).await.map_err(store_status)? else {
                return Ok(args.clone());
            };
            Ok(with_base_version(
                args,
                i64::try_from(stored.version).unwrap_or(i64::MAX),
            ))
        }
    }
}

fn with_base_version(args: &Value, version: i64) -> Value {
    let mut filled = args.clone();
    if let Some(object) = filled.as_object_mut() {
        object.insert("base_version".into(), json!(version));
    }
    filled
}

/// `Ok(None)` when the field is missing or null (fill from the object head).
/// `Ok(Some(v))` when the caller already named a version (number or numeric string).
/// `Err` when the value is present but not an integer.
fn parse_provided_base_version(args: &Value) -> Result<Option<i64>, Problem> {
    match args.get("base_version") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(number)) => {
            if let Some(version) = number.as_i64() {
                return Ok(Some(version));
            }
            if let Some(version) = number.as_u64() {
                return i64::try_from(version)
                    .map(Some)
                    .map_err(|_| invalid_base_version());
            }
            Err(invalid_base_version())
        }
        Some(Value::String(text)) => {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                return Err(invalid_base_version());
            }
            trimmed
                .parse::<i64>()
                .map(Some)
                .map_err(|_| invalid_base_version())
        }
        Some(_) => Err(invalid_base_version()),
    }
}

fn invalid_base_version() -> Problem {
    Problem::new(
        StatusCode::BAD_REQUEST,
        "invalid_base_version",
        "base_version must be an integer or a numeric string",
    )
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
        "assortment.save" => propose_assortment(state, identity, args).await,
        "move_to_group" => {
            let structure = args.get("structure").and_then(Value::as_str).unwrap_or("");
            let group = args.get("group").cloned().unwrap_or(Value::Null);
            let mut groups = serde_json::Map::new();
            groups.insert(structure.to_owned(), group);
            let mut patched = args.clone();
            patched["groups"] = Value::Object(groups);
            Box::pin(propose_operation(state, identity, "patch_object", &patched)).await
        }
        "create_link" | "update_link" => {
            let id = Uuid::now_v7();
            let object_id = args
                .get("id")
                .and_then(Value::as_str)
                .and_then(|text| Uuid::parse_str(text).ok());
            let row = ProposalRow {
                id: Uuid::now_v7(),
                object_id,
                object_key: None,
                type_key: args.get("type").and_then(Value::as_str).map(str::to_owned),
                base_version: args.get("base_version").and_then(Value::as_i64),
                patch: json!({ "link": args }),
                status: "pending".into(),
                conflict: false,
                position: 0,
            };
            state
                .store
                .create_proposal(
                    id,
                    "Link proposal",
                    crate::auth::suggestion_source(identity.kind),
                    identity.actor.actor,
                    identity.actor.on_behalf_of,
                    false,
                    &[row],
                )
                .await
                .map_err(store_status)?;
            Ok(
                json!({ "proposal_id": id, "needs_approval": true, "source": crate::auth::suggestion_source(identity.kind) }),
            )
        }
        "create_object" | "patch_object" => {
            if name == "patch_object" && args.get("base_version").and_then(Value::as_i64).is_none()
            {
                return Err(base_version_required());
            }
            let id = Uuid::now_v7();
            let object_id = args
                .get("id")
                .and_then(Value::as_str)
                .and_then(|text| Uuid::parse_str(text).ok());
            let cached = state.models.get(&state.store).await.ok();
            let object_key = match &cached {
                Some(cached) => labeled_key_from(
                    &cached.model,
                    args.get("type").and_then(Value::as_str),
                    |key| args.get("values").and_then(|values| values.get(key)),
                )
                .and_then(Value::as_str)
                .map(str::to_owned),
                None => args
                    .get("values")
                    .and_then(|values| values.get("number"))
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            };
            let row = ProposalRow {
                id: Uuid::now_v7(),
                object_id,
                object_key,
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
                    crate::auth::suggestion_source(identity.kind),
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

#[derive(Deserialize, ToSchema)]
pub(crate) struct LoginBody {
    email: String,
    password: String,
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct TokenBody {
    name: String,
    #[serde(default)]
    kind: String,
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct AgentBody {
    #[schema(value_type = Vec<crate::http_schema::FreeObject>)]
    messages: Vec<Value>,
    #[serde(default)]
    stream: bool,
    /// Compact screen snapshot from the browser. Included in the system
    /// prompt on every turn and returned by `ui.get_context`.
    #[serde(default)]
    #[schema(value_type = crate::http_schema::FreeObject)]
    context: Value,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct SearchParams {
    q: Option<String>,
    lang: Option<String>,
    filter: Option<String>,
    sort: Option<String>,
    group: Option<String>,
    page: Option<u32>,
    per_page: Option<u32>,
    wait_seq: Option<i64>,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct ListParams {
    q: Option<String>,
    #[serde(rename = "type")]
    r#type: Option<String>,
    limit: Option<i64>,
    /// Opaque `next_cursor` from the previous page.
    cursor: Option<String>,
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct WorkspaceBody {
    id: Option<Uuid>,
    name: String,
    locked: Option<bool>,
    share_token: Option<String>,
    #[schema(value_type = Option<crate::http_schema::FreeObject>)]
    layout: Option<Value>,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct StateQuery {
    environment: Option<String>,
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct StateBody {
    environment: Option<String>,
    #[schema(value_type = crate::http_schema::FreeObject)]
    document: Value,
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct AssignBody {
    email: String,
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct ViewBody {
    id: Option<Uuid>,
    workspace_id: Uuid,
    #[serde(rename = "type")]
    r#type: String,
    title: String,
    pane: Option<String>,
    position: Option<i32>,
    #[schema(value_type = Option<crate::http_schema::FreeObject>)]
    state: Option<Value>,
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct ViewTypeBody {
    key: String,
    title: String,
    embed_url: Option<String>,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct AssortmentTypeQuery {
    #[serde(rename = "type")]
    r#type: Option<String>,
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct AssortmentBody {
    id: Option<Uuid>,
    name: String,
    /// Type id or key. Absent until validation so an agent still gets 403 first.
    #[serde(default)]
    r#type: Option<String>,
    #[schema(value_type = crate::http_schema::FreeObject)]
    query: Value,
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct JobTypeBody {
    key: String,
    title: String,
    #[serde(default)]
    reads_fields: Vec<String>,
    assortment_id: Option<Uuid>,
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct BulkBody {
    objects: Vec<CreateObject>,
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct ProposalBody {
    title: String,
    /// Ignored. `source` is taken from the authenticated principal.
    /// The field stays so a client can send it and the value is discarded.
    #[serde(default, rename = "source")]
    _source: Option<String>,
    #[serde(default)]
    rows: Vec<ProposalRowBody>,
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct ProposalRowBody {
    id: Option<Uuid>,
    object_id: Option<Uuid>,
    object_key: Option<String>,
    type_key: Option<String>,
    base_version: Option<i64>,
    #[schema(value_type = crate::http_schema::FreeObject)]
    patch: Value,
    conflict: Option<bool>,
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct AcceptBody {
    rows: Option<Vec<Uuid>>,
    /// Import drafts only. Write the rows that cannot be imported as skipped
    /// instead of refusing the approval.
    #[serde(default)]
    pub(crate) skip_failed: bool,
    /// Import drafts only. Write just these preview row numbers.
    #[serde(default)]
    pub(crate) only_rows: Option<Vec<i32>>,
    /// Import drafts only. Leave these preview row numbers out.
    #[serde(default)]
    pub(crate) exclude_rows: Option<Vec<i32>>,
    /// Replacement patches keyed by proposal row. An empty `values` object rejects that row.
    #[serde(default)]
    #[schema(value_type = crate::http_schema::FreeObject)]
    patches: BTreeMap<Uuid, Value>,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct HistoryParams {
    object: Option<Uuid>,
    limit: Option<i64>,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct AsOfParams {
    seq: i64,
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct RollbackBody {
    seq: i64,
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct ImpactBody {
    remove_attribute: Option<String>,
    #[serde(alias = "remove_group")]
    remove_class: Option<String>,
    attribute: Option<String>,
    kind: Option<AttributeKind>,
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct WebhookBody {
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
    fn search_hides_a_link_type_even_when_the_index_returns_it() {
        let defs: braian_model::DefinitionSet = serde_json::from_value(json!({
            "model_revision": 1,
            "languages": ["en"],
            "unit_families": [],
            "attributes": [],
            "types": [
                {
                    "id": "018f0000-0000-7000-8000-000000000200",
                    "key": "item",
                    "labels": {},
                    "key_attribute": "018f0000-0000-7000-8000-000000000100",
                    "attributes": []
                },
                {
                    "id": "018f0000-0000-7000-8000-000000000201",
                    "key": "pairs_with",
                    "labels": {},
                    "key_attribute": "018f0000-0000-7000-8000-000000000100",
                    "attributes": [],
                    "kind": "link"
                }
            ],
            "structures": []
        }))
        .expect("defs");
        let hit = |id: &str, type_key: &str| braian_search::SearchHit {
            id: id.into(),
            document: json!({ "id": id, "key": id, "type_key": type_key }),
        };
        let page = hide_link_hits(
            braian_search::SearchPage {
                found: 3,
                hits: vec![
                    hit("a", "item"),
                    hit("link", "pairs_with"),
                    hit("b", "item"),
                ],
                groups: vec![],
            },
            &defs,
        );
        assert_eq!(page.found, 2);
        assert_eq!(
            page.hits
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            vec!["a", "b"]
        );
    }

    #[test]
    fn an_accept_patch_can_only_keep_proposed_values() {
        let proposed = json!({
            "values": {
                "active": false,
                "name": { "sv": "Hej", "en-GB": "Hello" },
                "tags": ["aa", "bb"]
            }
        });
        let current = json!({ "tags": ["aa"] });
        let current = current.as_object().expect("current");
        validate_replacement(
            &proposed,
            &json!({ "values": { "active": false, "name": { "sv": "Hej" }, "tags": ["aa"] } }),
            Some(current),
        )
        .expect("subset");
        let extra_field = validate_replacement(
            &proposed,
            &json!({ "values": { "color": "red" } }),
            Some(current),
        )
        .expect_err("field");
        assert_eq!(extra_field.code, "patch_not_proposed");
        let extra_language = validate_replacement(
            &proposed,
            &json!({ "values": { "name": { "sv": "Hej", "de": "Hi" } } }),
            Some(current),
        )
        .expect_err("language");
        assert!(extra_language.detail.contains("de"), "{extra_language:?}");
        let changed = validate_replacement(
            &proposed,
            &json!({ "values": { "active": true } }),
            Some(current),
        )
        .expect_err("value");
        assert!(changed.detail.contains("active"), "{changed:?}");
        let extra_tag = validate_replacement(
            &proposed,
            &json!({ "values": { "tags": ["aa", "zz"] } }),
            Some(current),
        )
        .expect_err("tag");
        assert!(extra_tag.detail.contains("tags"), "{extra_tag:?}");
    }

    #[tokio::test]
    async fn from_agent_cannot_accept_a_proposal() {
        if std::env::var_os("BRAIAN_TEST_DATABASE_URL").is_none()
            && let Ok(text) = std::fs::read_to_string(".env")
        {
            for line in text.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                let Some((key, value)) = line.split_once('=') else {
                    continue;
                };
                if std::env::var_os(key).is_none() {
                    unsafe { std::env::set_var(key, value.trim_matches('"')) };
                }
            }
        }
        let db = braian_store::testing::TestDb::create().await;
        let identities =
            crate::identities(db.store(), "ada:human:token-ada;bot:agent@ada:token-bot")
                .await
                .expect("identities");
        let human = identities
            .iter()
            .find(|identity| identity.handle == "ada")
            .expect("human")
            .clone();
        let state = AppState::new(db.store().clone(), identities, None);
        let proposal_id = Uuid::now_v7();
        let row_id = Uuid::now_v7();
        state
            .store
            .create_proposal(
                proposal_id,
                "From the agent",
                "agent",
                human.actor.actor,
                human.actor.on_behalf_of,
                false,
                &[ProposalRow {
                    id: row_id,
                    object_id: None,
                    object_key: Some("SKU-FROM-AGENT".into()),
                    type_key: Some("item".into()),
                    base_version: None,
                    patch: json!({ "values": { "sku": "SKU-FROM-AGENT", "active": true } }),
                    status: "pending".into(),
                    conflict: false,
                    position: 0,
                }],
            )
            .await
            .expect("proposal");
        let refused = run_operation(
            &state,
            &human,
            "accept_proposal",
            &json!({ "id": proposal_id }),
            true,
        )
        .await
        .expect_err("agent accept");
        assert_eq!(refused.status, StatusCode::FORBIDDEN.as_u16());
        assert_eq!(refused.code, "agent_cannot_accept");
        let discarded = run_operation(
            &state,
            &human,
            "discard_proposal",
            &json!({ "id": proposal_id }),
            true,
        )
        .await
        .expect_err("agent discard");
        assert_eq!(discarded.status, StatusCode::FORBIDDEN.as_u16());
        assert_eq!(discarded.code, "agent_cannot_discard");
        let stored = state
            .store
            .proposal(proposal_id)
            .await
            .expect("load")
            .expect("row");
        assert_eq!(stored["status"], "open");
        assert_eq!(stored["rows"][0]["status"], "pending");
        assert_eq!(stored["rows"][0]["id"], json!(row_id));
    }

    #[tokio::test]
    async fn from_agent_patch_fills_base_version_on_an_existing_object() {
        if std::env::var_os("BRAIAN_TEST_DATABASE_URL").is_none()
            && let Ok(text) = std::fs::read_to_string(".env")
        {
            for line in text.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                let Some((key, value)) = line.split_once('=') else {
                    continue;
                };
                if std::env::var_os(key).is_none() {
                    unsafe { std::env::set_var(key, value.trim_matches('"')) };
                }
            }
        }
        let db = braian_store::testing::TestDb::create().await;
        let identities =
            crate::identities(db.store(), "ada:human:token-ada;bot:agent@ada:token-bot")
                .await
                .expect("identities");
        let human = identities
            .iter()
            .find(|identity| identity.handle == "ada")
            .expect("human")
            .clone();
        let state = AppState::new(db.store().clone(), identities, None);
        let defs: braian_model::DefinitionSet =
            serde_json::from_str(include_str!("../../../testdata/rules/definitions.json"))
                .expect("defs");
        state
            .store
            .put_definitions(&human.actor, &defs)
            .await
            .expect("definitions");
        state.models.invalidate().await;
        run_operation(
            &state,
            &human,
            "create_object",
            &json!({
                "type": "item",
                "values": { "sku": "SKU-AGENT-VER", "active": true }
            }),
            false,
        )
        .await
        .expect("create");
        let object_id = state
            .store
            .list_object_heads(10)
            .await
            .expect("heads")
            .into_iter()
            .find(|(_, key, _, _)| key == "SKU-AGENT-VER")
            .map(|(id, _, _, _)| id.to_string())
            .expect("created object");
        let stored = state
            .store
            .get_object(ObjectId::from_uuid(
                Uuid::parse_str(&object_id).expect("uuid"),
            ))
            .await
            .expect("load")
            .expect("object");
        let version = stored.version;
        let proposed = run_operation(
            &state,
            &human,
            "patch_object",
            &json!({
                "id": object_id,
                "type": "item",
                "values": { "active": false }
            }),
            true,
        )
        .await
        .expect("agent patch");
        assert_eq!(proposed["needs_approval"], true, "{proposed}");
        let proposal_id = proposed["proposal_id"].as_str().expect("proposal_id");
        let proposal = state
            .store
            .proposal(Uuid::parse_str(proposal_id).expect("proposal uuid"))
            .await
            .expect("load proposal")
            .expect("proposal row");
        assert_eq!(
            proposal["rows"][0]["base_version"].as_i64(),
            Some(version as i64),
            "{proposal}"
        );
    }

    #[tokio::test]
    async fn from_agent_keeps_a_string_base_version_and_rejects_a_bad_one() {
        if std::env::var_os("BRAIAN_TEST_DATABASE_URL").is_none()
            && let Ok(text) = std::fs::read_to_string(".env")
        {
            for line in text.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                let Some((key, value)) = line.split_once('=') else {
                    continue;
                };
                if std::env::var_os(key).is_none() {
                    unsafe { std::env::set_var(key, value.trim_matches('"')) };
                }
            }
        }
        let db = braian_store::testing::TestDb::create().await;
        let identities =
            crate::identities(db.store(), "ada:human:token-ada;bot:agent@ada:token-bot")
                .await
                .expect("identities");
        let human = identities
            .iter()
            .find(|identity| identity.handle == "ada")
            .expect("human")
            .clone();
        let state = AppState::new(db.store().clone(), identities, None);
        let defs: braian_model::DefinitionSet =
            serde_json::from_str(include_str!("../../../testdata/rules/definitions.json"))
                .expect("defs");
        state
            .store
            .put_definitions(&human.actor, &defs)
            .await
            .expect("definitions");
        state.models.invalidate().await;
        run_operation(
            &state,
            &human,
            "create_object",
            &json!({
                "type": "item",
                "values": { "sku": "SKU-AGENT-STR", "active": true }
            }),
            false,
        )
        .await
        .expect("create");
        let object_id = state
            .store
            .list_object_heads(10)
            .await
            .expect("heads")
            .into_iter()
            .find(|(_, key, _, _)| key == "SKU-AGENT-STR")
            .map(|(id, _, _, _)| id.to_string())
            .expect("created object");
        let stored = state
            .store
            .get_object(ObjectId::from_uuid(
                Uuid::parse_str(&object_id).expect("uuid"),
            ))
            .await
            .expect("load")
            .expect("object");
        let version = stored.version;
        // A second write bumps the head so a stale string base_version is not the head.
        run_operation(
            &state,
            &human,
            "patch_object",
            &json!({
                "id": object_id,
                "type": "item",
                "base_version": version,
                "values": { "active": false }
            }),
            false,
        )
        .await
        .expect("human patch");
        let stale = version.to_string();
        let proposed = run_operation(
            &state,
            &human,
            "patch_object",
            &json!({
                "id": object_id,
                "type": "item",
                "base_version": stale,
                "values": { "active": true }
            }),
            true,
        )
        .await
        .expect("agent patch with string base_version");
        let proposal_id = proposed["proposal_id"].as_str().expect("proposal_id");
        let proposal = state
            .store
            .proposal(Uuid::parse_str(proposal_id).expect("proposal uuid"))
            .await
            .expect("load proposal")
            .expect("proposal row");
        assert_eq!(
            proposal["rows"][0]["base_version"].as_i64(),
            Some(version as i64),
            "string base_version must be kept, not replaced with the head: {proposal}"
        );
        let refused = run_operation(
            &state,
            &human,
            "patch_object",
            &json!({
                "id": object_id,
                "type": "item",
                "base_version": "not-a-version",
                "values": { "active": true }
            }),
            true,
        )
        .await
        .expect_err("bad string");
        assert_eq!(refused.code, "invalid_base_version");
        assert!(
            refused.detail.contains("integer") || refused.detail.contains("numeric"),
            "{refused:?}"
        );
    }

    #[test]
    fn cursor_round_trips_an_object_id() {
        let id = Uuid::now_v7();
        assert_eq!(decode_cursor(&encode_cursor(id)), Some(id));
        assert!(decode_cursor("018f1000-0000-7000-8000-000000000011").is_none());
        assert!(decode_cursor("v1.zz").is_none());
    }

    #[test]
    fn business_key_follows_the_type_key_attribute() {
        let defs: DefinitionSet = serde_json::from_str(ANCESTOR_DEFS).expect("defs");
        let model = braian_rules::CompiledModel::compile(&defs).expect("compile");
        let values = json!({"number": "ART-1", "sku": "OLD"});
        let found = labeled_key_from(&model, Some("article"), |key| values.get(key));
        assert_eq!(found.and_then(Value::as_str), Some("ART-1"));
        let only_old = json!({"sku": "OLD"});
        assert!(labeled_key_from(&model, Some("article"), |key| only_old.get(key)).is_none());
        assert!(labeled_key_from(&model, Some("missing"), |key| only_old.get(key)).is_none());
        let numbered = json!({"number": "ART-9"});
        assert_eq!(
            labeled_key_from(&model, Some("missing"), |key| numbered.get(key))
                .and_then(Value::as_str),
            Some("ART-9")
        );
    }

    #[test]
    fn graph_marks_an_ancestor_group_attribute_as_inherited() {
        let defs: DefinitionSet = serde_json::from_str(ANCESTOR_DEFS).expect("defs");
        let model = braian_rules::CompiledModel::compile(&defs).expect("compile");
        let food = braian_model::StructureId::from_uuid(
            Uuid::parse_str("018f0000-0000-7000-8000-000000000300").expect("structure"),
        );
        let dairy = braian_model::GroupId::from_uuid(
            Uuid::parse_str("018f0000-0000-7000-8000-000000000310").expect("dairy"),
        );
        let cheese = braian_model::GroupId::from_uuid(
            Uuid::parse_str("018f0000-0000-7000-8000-000000000311").expect("cheese"),
        );
        let type_id = braian_model::TypeId::from_uuid(
            Uuid::parse_str("018f0000-0000-7000-8000-000000000201").expect("type"),
        );
        let in_cheese = Object {
            id: ObjectId::from_uuid(
                Uuid::parse_str("018f1000-0000-7000-8000-000000000011").expect("object"),
            ),
            type_id,
            model_revision: 1,
            groups: BTreeMap::from([(food, cheese)]),
            values: BTreeMap::new(),
        };
        let in_dairy = Object {
            groups: BTreeMap::from([(food, dairy)]),
            ..in_cheese.clone()
        };
        let rows = |object: &Object, key: &str| {
            let attribute = model.attribute_by_key(key).expect(key);
            let item = model
                .effective_attributes(object.type_id, &object.groups)
                .expect("effective")
                .into_iter()
                .find(|item| item.id == attribute.id);
            match item {
                Some(item) => attribute_sources(&defs, object, attribute.id, &item.sources),
                None => Vec::new(),
            }
        };

        let fat = rows(&in_cheese, "fat_content");
        assert_eq!(fat.len(), 1, "{fat:?}");
        assert_eq!(fat[0]["from"], "group");
        assert_eq!(fat[0]["structure"], "food");
        assert_eq!(fat[0]["group"], "dairy");
        assert!(fat[0]["class"].is_null());
        assert_eq!(fat[0]["inherited"], true);

        let ripeness = rows(&in_cheese, "ripeness");
        assert_eq!(ripeness.len(), 1, "{ripeness:?}");
        assert_eq!(ripeness[0]["group"], "cheese");
        assert!(ripeness[0]["class"].is_null());
        assert_eq!(ripeness[0]["inherited"], false);

        let number = rows(&in_cheese, "number");
        assert_eq!(number[0]["from"], "type");
        assert_eq!(number[0]["inherited"], false);

        assert_eq!(rows(&in_dairy, "fat_content")[0]["inherited"], false);
        assert_eq!(rows(&in_dairy, "fat_content")[0]["group"], "dairy");
        assert!(rows(&in_dairy, "ripeness").is_empty());
    }

    #[test]
    fn display_name_prefers_english_and_skips_measurements() {
        assert_eq!(
            display_name_text(&json!({"sv": "Canvas 2 green", "en": "Tent 2 green"})),
            "Tent 2 green"
        );
        assert_eq!(
            display_name_text(&json!({"sv": "Canvas 2 green", "en": "  "})),
            "Canvas 2 green"
        );
        assert_eq!(display_name_text(&json!("Plain")), "Plain");
        assert_eq!(
            display_name_text(&json!({"value": "2.4", "unit": "kg"})),
            ""
        );
    }

    #[test]
    fn parse_provided_base_version_keeps_numbers_and_numeric_strings() {
        assert_eq!(
            parse_provided_base_version(&json!({})).expect("missing"),
            None
        );
        assert_eq!(
            parse_provided_base_version(&json!({ "base_version": null })).expect("null"),
            None
        );
        assert_eq!(
            parse_provided_base_version(&json!({ "base_version": 3 })).expect("i64"),
            Some(3)
        );
        assert_eq!(
            parse_provided_base_version(&json!({ "base_version": "7" })).expect("string"),
            Some(7)
        );
        assert_eq!(
            parse_provided_base_version(&json!({ "base_version": " 12 " })).expect("trim"),
            Some(12)
        );
        let bad_string = parse_provided_base_version(&json!({ "base_version": "latest" }))
            .expect_err("non-numeric string");
        assert_eq!(bad_string.code, "invalid_base_version");
        let empty = parse_provided_base_version(&json!({ "base_version": "" })).expect_err("empty");
        assert_eq!(empty.code, "invalid_base_version");
        let bad_type =
            parse_provided_base_version(&json!({ "base_version": true })).expect_err("bool");
        assert_eq!(bad_type.code, "invalid_base_version");
        let array =
            parse_provided_base_version(&json!({ "base_version": [1] })).expect_err("array");
        assert_eq!(array.code, "invalid_base_version");
    }

    #[test]
    fn with_base_version_normalizes_a_string_field_to_a_number() {
        let args = json!({ "id": "x", "base_version": "4", "values": { "active": false } });
        let filled = with_base_version(&args, 4);
        assert_eq!(filled["base_version"], json!(4));
        assert_eq!(filled["values"]["active"], false);
    }

    #[test]
    fn reply_text_keeps_a_space_between_output_blocks() {
        let payload = json!({
            "output": [{
                "type": "message",
                "content": [
                    {"type": "output_text", "text": "I need the product's current version to propose this change."},
                    {"type": "output_text", "text": "I couldn't create the proposal because it requires the product's current version."}
                ]
            }]
        });
        let text = reply_text(&payload);
        assert!(
            text.contains("change. I couldn't") || text.contains("change.\nI couldn't"),
            "{text}"
        );
        assert!(!text.contains("change.I couldn't"), "{text}");
    }

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
        let body = responses_body(
            "gpt-6-luna",
            &[json!({ "role": "user", "content": "hej" })],
            &[],
        );
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
        let value = ui_command("ui.filter", &json!({ "q": "tent" }));
        assert_eq!(value["ui"], true);
        assert_eq!(value["name"], "ui.filter");
        assert_eq!(value["args"]["q"], "tent");
        assert_eq!(value["applied"], false);
    }

    /// Names the browser lists in `apps/web/src/ui-bus.ts` (`uiOperations`).
    /// A name added on only one side fails this test or the web registry test.
    const CLIENT_UI_OPERATIONS: &[&str] = &[
        "ui.open_view",
        "ui.open_template",
        "ui.open_workspace",
        "ui.open_changes",
        "ui.place",
        "ui.link",
        "ui.filter",
        "ui.group",
        "ui.sort",
        "ui.select",
        "ui.select_rows",
        "ui.show_column",
        "ui.hide_column",
        "ui.move_column",
        "ui.set_theme",
        "ui.export",
        "ui.get_context",
        "ui.get_selection",
        "open_view",
        "move_view",
        "split",
        "resize",
        "swap",
        "focus",
        "move_view_left",
        "move_view_right",
        "move_view_up",
        "move_view_down",
        "zoom",
        "close_view",
        "get_layout",
        "reset_layout",
    ];

    fn marked_product() -> Value {
        json!({
            "workspace": { "id": "018f2000-0000-7000-8000-000000000003", "name": "Enrich objects" },
            "panes": [
                { "id": "table", "type": "object_table", "title": "Objects", "focused": true },
                { "id": "detail", "type": "object_detail", "title": "Detail", "focused": false },
                { "id": "graph", "type": "graph", "title": "Graph", "focused": false }
            ],
            "focused": "table",
            "selection": [{
                "id": "018f1000-0000-7000-8000-000000000011",
                "key": "SYN-000001",
                "name": "Insulation 2",
                "type": "article",
                "version": 3
            }],
            "filter": "",
            "sort": "key:asc",
            "columns": [
                { "id": "name", "header": "Name", "hidden": false },
                { "id": "status", "header": "Status", "hidden": true }
            ],
            "language": "sv"
        })
    }

    #[test]
    fn selection_question_resolves_to_the_marked_object() {
        let context = marked_product();
        // english-check: allow
        let messages = vec![json!({ "role": "user", "content": "Vilken produkt är markerad?" })];
        let reply = selection_reply(&messages, &context).expect("answer");
        assert!(reply.contains("SYN-000001"), "{reply}");
        assert!(reply.contains("Insulation 2"), "{reply}");
        assert!(reply.contains("is selected"), "{reply}");
        assert!(selection_reply(&messages, &Value::Null).is_none());

        let prompt = agent_system_prompt(&context, "Vilken produkt är markerad?");
        // english-check: end
        assert!(prompt.contains("SYN-000001"), "{prompt}");
        assert!(prompt.contains("Insulation 2"), "{prompt}");
        assert!(prompt.contains("Answer in English"), "{prompt}");
        assert!(prompt.contains("object_detail"), "{prompt}");
        assert!(prompt.contains("get_object"), "{prompt}");

        let english = selection_reply(
            &[json!({ "role": "user", "content": "Which product is selected?" })],
            &context,
        )
        .expect("english");
        assert!(english.contains("SYN-000001"), "{english}");
        assert!(english.contains("Insulation 2"), "{english}");
        assert!(english.contains("selected"), "{english}");

        let read =
            context_tool_result("ui_get_selection", &json!({}), Some(&context)).expect("tool");
        assert_eq!(read["selection"][0]["key"], "SYN-000001");
        assert_eq!(read["selection"][0]["name"], "Insulation 2");
        assert_eq!(read["selection"][0]["version"], 3);

        // The agent sees which columns are hidden, so it can show them again.
        assert!(prompt.contains("ui_show_column"), "{prompt}");
        let screen = compact_ui_context(&context);
        assert_eq!(screen["columns"][1]["id"], "status");
        assert_eq!(screen["columns"][1]["hidden"], true);
        assert_eq!(compact_ui_context(&Value::Null)["columns"], json!([]));
    }

    #[test]
    fn every_registered_ui_operation_is_exposed_to_the_agent_or_excluded() {
        let body = responses_body("gpt-6-luna", &[], &[]);
        let tools = body["tools"]
            .as_array()
            .expect("tools")
            .iter()
            .map(|tool| tool["name"].as_str().expect("name").to_owned())
            .collect::<BTreeSet<_>>();
        let mcp = registry()
            .iter()
            .filter(|op| exposed_to_agent(op))
            .map(|op| tool_name(op.name))
            .collect::<BTreeSet<_>>();
        assert_eq!(tools, mcp, "the agent and MCP must expose the same tools");
        assert!(tools.contains("get_object"));
        assert!(tools.contains("search"));
        assert!(tools.contains("list_objects"));
        assert!(tools.contains("ui_get_context"));
        assert!(tools.contains("ui_get_selection"));
        assert!(
            !tools.contains("accept_proposal"),
            "an agent must not be offered the commit operation"
        );
        assert!(
            !tools.contains("discard_proposal"),
            "an agent must not be offered discard"
        );
        assert!(
            !tools.contains("model_delete_group"),
            "an agent must not be offered delete"
        );
        assert!(!tools.contains("model_delete_structure"));
        assert!(tools.contains("model_rename_attribute_key"));
        assert!(
            tools.contains("model_list_attribute_definitions"),
            "the agent can list attribute definitions"
        );
        assert!(
            tools.contains("model_attribute_usage"),
            "the agent can read attribute usage"
        );
        assert!(
            tools.contains("mass_edit"),
            "mass edit stays a proposal the person approves"
        );

        let registered = registry()
            .iter()
            .filter(|op| is_ui_operation(op.name))
            .map(|op| op.name)
            .collect::<BTreeSet<_>>();
        let client = CLIENT_UI_OPERATIONS
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        assert_eq!(
            registered, client,
            "the operation registry and the web UI operation list diverged"
        );
        for name in &registered {
            let wire = tool_name(name);
            let excluded = AGENT_UI_EXCLUDED.contains(name);
            let exposed = tools.contains(&wire);
            assert!(
                excluded != exposed,
                "{name} must be exposed to the agent or listed in AGENT_UI_EXCLUDED"
            );
            if exposed {
                assert_eq!(operation_for_tool(&wire), Some(*name));
            }
        }
    }

    #[test]
    fn the_agent_is_offered_no_delete() {
        let tools = responses_body("model", &[], &[])["tools"]
            .as_array()
            .expect("tools")
            .iter()
            .map(|tool| tool["name"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        let deleting = registry()
            .iter()
            .map(|op| op.name)
            .filter(|name| operation_deletes(name))
            .collect::<Vec<_>>();
        for name in &deleting {
            assert!(
                AGENT_COMMIT_EXCLUDED.contains(name),
                "{name} deletes and must be in AGENT_COMMIT_EXCLUDED"
            );
            let wire = tool_name(name);
            assert!(
                !tools.iter().any(|tool| tool == &wire),
                "{wire} must not be offered to the agent"
            );
        }
        for required in [
            "model.delete_type",
            "model.delete_attribute",
            "model.delete_value_list",
            "model.delete_value_option",
            "model.delete_unit_family",
            "model.delete_unit",
            "model.delete_structure",
            "model.delete_group",
            "model.remove_attribute",
            "model.detach_attribute",
            "delete_object",
            "remove_link",
            "model.delete_link_type",
            "discard_proposal",
        ] {
            assert!(
                deleting.contains(&required),
                "{required} is missing from the registry"
            );
        }
        for offered in ["model_list_attribute_definitions", "model_attribute_usage"] {
            assert!(
                tools.iter().any(|tool| tool == offered),
                "{offered} is read-only and must be offered to the agent"
            );
            assert!(
                !deleting.iter().any(|name| tool_name(name) == offered),
                "{offered} must not be treated as a delete"
            );
        }
    }

    #[test]
    fn function_call_named_ui_filter_maps_to_the_operation() {
        let payload = json!({
            "output": [{
                "type": "function_call",
                "name": "ui_filter",
                "call_id": "call_1",
                "arguments": "{\"q\":\"tent\"}"
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

    const ANCESTOR_DEFS: &str = r#"{
        "model_revision": 1,
        "languages": ["en"],
        "unit_families": [],
        "attributes": [
            {
                "id": "018f0000-0000-7000-8000-000000000100",
                "key": "number",
                "labels": {},
                "language_dependent": false,
                "multiple": false,
                "kind": { "type": "text" },
                "index": { "searchable": true, "filterable": false, "sortable": true }
            },
            {
                "id": "018f0000-0000-7000-8000-000000000105",
                "key": "fat_content",
                "labels": { "en": "Fat content" },
                "language_dependent": false,
                "multiple": false,
                "kind": { "type": "text" },
                "index": { "searchable": false, "filterable": false, "sortable": false }
            },
            {
                "id": "018f0000-0000-7000-8000-000000000106",
                "key": "ripeness",
                "labels": { "en": "Ripeness" },
                "language_dependent": false,
                "multiple": false,
                "kind": { "type": "text" },
                "index": { "searchable": false, "filterable": false, "sortable": false }
            },
            {
                "id": "018f0000-0000-7000-8000-000000000107",
                "key": "badge",
                "labels": {},
                "language_dependent": false,
                "multiple": false,
                "kind": { "type": "text" },
                "index": { "searchable": false, "filterable": false, "sortable": false }
            }
        ],
        "types": [
            {
                "id": "018f0000-0000-7000-8000-000000000201",
                "key": "article",
                "labels": {},
                "key_attribute": "018f0000-0000-7000-8000-000000000100",
                "attributes": ["018f0000-0000-7000-8000-000000000100"]
            }
        ],
        "structures": [
            {
                "id": "018f0000-0000-7000-8000-000000000300",
                "key": "food",
                "groups": [
                    {
                        "id": "018f0000-0000-7000-8000-000000000310",
                        "key": "dairy",
                        "attributes": ["018f0000-0000-7000-8000-000000000105"]
                    },
                    {
                        "id": "018f0000-0000-7000-8000-000000000311",
                        "key": "cheese",
                        "parent": "018f0000-0000-7000-8000-000000000310",
                        "attributes": ["018f0000-0000-7000-8000-000000000106"]
                    }
                ],
                "classes": []
            }
        ]
    }"#;

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
