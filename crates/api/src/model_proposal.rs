//! Data-model edits are proposals. Accepting one applies it to the definition set.

use axum::http::StatusCode;
use braian_model::{
    ApplyError, ChangeImpact, ModelChange, StructureErrorCode, apply_all, change_needs_reindex,
    deletes_data, interpret, preview,
};
use braian_rules::CompiledModel;
use braian_store::{Error, ProposalRow, ProposalWrite};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::auth::Identity;
use crate::problem::{Problem, store_status};
use crate::state::AppState;

pub async fn propose(
    state: &AppState,
    identity: &Identity,
    name: &str,
    args: &Value,
) -> Result<Value, Problem> {
    let cached = state.models.get(&state.store).await?;
    let changes = changes_from(name, args, &cached.defs)?;
    for change in &changes {
        crate::structures::guard_model_change(state, &identity.actor, change, false).await?;
    }
    if changes.is_empty() {
        return Err(Problem::new(
            StatusCode::BAD_REQUEST,
            "empty_model",
            "the draft has no steps",
        ));
    }
    if !identity.may_commit() && changes.iter().any(deletes_data) {
        return Err(crate::auth::delete_forbidden());
    }
    let (after, impact) = preview(&cached.defs, &changes).map_err(model_status)?;
    CompiledModel::compile(&after).map_err(Problem::definitions)?;
    let title = title_for(args, &impact.lines);
    let source = match identity.kind {
        braian_store::PrincipalKind::Agent => "agent",
        braian_store::PrincipalKind::Integration => "import",
        braian_store::PrincipalKind::Human => "user",
    };
    let id = Uuid::now_v7();
    let rows = changes
        .iter()
        .enumerate()
        .map(|(index, change)| ProposalRow {
            id: Uuid::now_v7(),
            object_id: None,
            object_key: None,
            type_key: Some("model".into()),
            base_version: i64::try_from(cached.defs.model_revision).ok(),
            patch: serde_json::to_value(change).unwrap_or(json!({})),
            status: "pending".into(),
            conflict: false,
            position: i32::try_from(index).unwrap_or(0),
        })
        .collect::<Vec<_>>();
    state
        .store
        .create_proposal(
            id,
            &title,
            source,
            identity.actor.actor,
            identity.actor.on_behalf_of,
            false,
            &rows,
        )
        .await
        .map_err(store_status)?;
    let consequence = consequence(state, &cached.defs, &changes, &impact).await?;
    Ok(json!({
        "proposal_id": id,
        "id": id,
        "needs_approval": true,
        "title": title,
        "source": source,
        "rows": rows.len(),
        "consequence": consequence,
    }))
}

pub async fn attach(state: &AppState, body: &mut Value) -> Result<(), Problem> {
    let Some(rows) = body.get("rows").and_then(Value::as_array) else {
        return Ok(());
    };
    let mut changes = Vec::new();
    for row in rows {
        if row.get("type_key").and_then(Value::as_str) != Some("model") {
            continue;
        }
        if row.get("status").and_then(Value::as_str) != Some("pending") {
            continue;
        }
        let Some(patch) = row.get("patch") else {
            continue;
        };
        if let Ok(change) = serde_json::from_value::<ModelChange>(patch.clone()) {
            changes.push(change);
        }
    }
    if changes.is_empty() {
        return Ok(());
    }
    let cached = state.models.get(&state.store).await?;
    match preview(&cached.defs, &changes) {
        Ok((after, impact)) => {
            let mut consequence = consequence(state, &cached.defs, &changes, &impact).await?;
            if let Err(errors) = CompiledModel::compile(&after) {
                consequence["definition_errors"] = json!(
                    errors
                        .into_iter()
                        .map(|error| error.message)
                        .collect::<Vec<_>>()
                );
            }
            if let Some(object) = body.as_object_mut() {
                object.insert("consequence".into(), consequence);
            }
        }
        Err(message) => {
            if let Some(object) = body.as_object_mut() {
                object.insert(
                    "consequence".into(),
                    json!({ "lines": [], "definition_errors": [message.to_string()], "affected_objects": 0, "samples": [], "jobs": [] }),
                );
            }
        }
    }
    Ok(())
}

pub async fn apply_row(
    state: &AppState,
    identity: &Identity,
    proposal_id: Uuid,
    row_id: Uuid,
    row: &Value,
) -> Result<Option<i64>, Problem> {
    let change: ModelChange =
        serde_json::from_value(row.get("patch").cloned().unwrap_or(json!({})))
            .map_err(|error| Problem::new(StatusCode::BAD_REQUEST, "model", error.to_string()))?;
    crate::structures::guard_model_change(state, &identity.actor, &change, true).await?;
    for attempt in 0..2 {
        let mut defs = match state.store.definition_set().await {
            Ok(defs) => defs,
            Err(Error::NoDefinitions) => braian_model::DefinitionSet {
                model_revision: 0,
                languages: Vec::new(),
                unit_families: Vec::new(),
                value_lists: Vec::new(),
                attributes: Vec::new(),
                types: Vec::new(),
                structures: Vec::new(),
            },
            Err(error) => return Err(store_status(error)),
        };
        let current = defs.model_revision;
        let purge = braian_model::purge_for(&defs, std::slice::from_ref(&change));
        apply_all(&mut defs, std::slice::from_ref(&change)).map_err(model_status)?;
        defs.model_revision = current + 1;
        CompiledModel::compile(&defs).map_err(Problem::definitions)?;
        match state
            .store
            .commit_proposal_row(
                &identity.actor,
                proposal_id,
                row_id,
                "accepted",
                ProposalWrite::Definitions {
                    defs,
                    purge: Box::new(purge),
                },
            )
            .await
        {
            Ok(Some(receipt)) => {
                state.models.invalidate().await;
                if change_needs_reindex(&change) {
                    state.store.request_reindex().await.map_err(store_status)?;
                }
                state.emit(json!({
                    "seq": receipt.last_seq,
                    "op": "model",
                }));
                return Ok(Some(receipt.last_seq));
            }
            Ok(None) => return Ok(None),
            Err(Error::BadRevision { .. }) if attempt == 0 => continue,
            Err(error) => return Err(store_status(error)),
        }
    }
    Err(Problem::new(
        StatusCode::CONFLICT,
        "revision_conflict",
        "definition revision changed",
    ))
}

pub fn is_model_row(row: &Value) -> bool {
    row.get("type_key").and_then(Value::as_str) == Some("model")
}

pub(crate) async fn consequence(
    state: &AppState,
    defs: &braian_model::DefinitionSet,
    changes: &[ModelChange],
    impact: &ChangeImpact,
) -> Result<Value, Problem> {
    let mut affected = 0i64;
    let mut samples = Vec::new();
    if !impact.usage_fields.is_empty() {
        for key in &impact.usage_fields {
            let Some(attribute) = defs.attributes.iter().find(|item| &item.key == key) else {
                continue;
            };
            affected += state
                .store
                .count_attribute_usage(attribute.id.as_uuid())
                .await
                .map_err(store_status)?;
            if samples.len() >= 8 {
                continue;
            }
            let ids = state
                .store
                .objects_with_attribute(attribute.id.as_uuid(), 8)
                .await
                .map_err(store_status)?;
            for id in ids {
                if samples.len() >= 8 {
                    break;
                }
                let Some(stored) = state.store.get_object(id).await.map_err(store_status)? else {
                    continue;
                };
                samples.push(json!({
                    "id": stored.object.id.as_uuid(),
                    "key": stored.key,
                }));
            }
        }
    } else if !impact.groups.is_empty() {
        let groups = parse_groups(&impact.groups);
        affected = state
            .store
            .count_in_groups(&groups)
            .await
            .map_err(store_status)?;
        samples = state
            .store
            .sample_in_groups(&groups, 8)
            .await
            .map_err(store_status)?;
    }
    let jobs = if impact.fields.is_empty() {
        Vec::new()
    } else {
        state
            .store
            .jobs_touching(&impact.fields)
            .await
            .map_err(store_status)?
    };
    let purge = braian_model::purge_for(defs, changes);
    let counts = state
        .store
        .measure_purge(&purge)
        .await
        .map_err(store_status)?;
    let detail = braian_model::consequence_lines(&counts);
    Ok(json!({
        "lines": impact.lines,
        "warnings": impact.warnings,
        "fields": impact.fields,
        "affected_objects": affected,
        "samples": samples,
        "jobs": jobs,
        "objects": counts.objects,
        "links": counts.links,
        "values": counts.values,
        "selections": counts.selections,
        "destructive": purge.destructive,
        "confirm_name": purge.confirm_name,
        "sentence": detail.first().cloned().unwrap_or_default(),
        "detail": detail,
    }))
}

fn parse_groups(groups: &[String]) -> Vec<Uuid> {
    groups
        .iter()
        .filter_map(|group| Uuid::parse_str(group).ok())
        .collect()
}

pub(crate) fn changes_from(
    name: &str,
    args: &Value,
    defs: &braian_model::DefinitionSet,
) -> Result<Vec<ModelChange>, Problem> {
    let bad = |message: String| Problem::new(StatusCode::BAD_REQUEST, "model", message);
    match name {
        "model.command" => {
            let text = text_arg(args);
            interpret(text, defs).ok_or_else(|| bad(format!("unknown command: {text}")))
        }
        "model.draft" => {
            if let Some(text) = args.get("text").and_then(Value::as_str) {
                return interpret(text, defs)
                    .ok_or_else(|| bad(format!("unknown command: {text}")));
            }
            let steps = args.get("steps").cloned().unwrap_or(json!([]));
            serde_json::from_value(steps).map_err(|error| bad(error.to_string()))
        }
        "model.move_class" => Ok(vec![ModelChange::MoveClass {
            structure: required_str(args, "structure")?,
            class_key: required_str(args, "class")?,
            group: required_str(args, "group")?,
        }]),
        "model.add_attribute" => Ok(vec![ModelChange::AddAttribute {
            structure: required_str(args, "structure")?,
            class_key: class_ref(args)?,
            attribute: required_str(args, "attribute")?,
        }]),
        "model.remove_attribute" => Ok(vec![ModelChange::RemoveAttribute {
            structure: required_str(args, "structure")?,
            class_key: class_ref(args)?,
            attribute: required_str(args, "attribute")?,
        }]),
        "model.reorder_attributes" => {
            let attributes = string_list(args, "attributes").unwrap_or_default();
            if let Some(type_key) =
                optional_str(args, "type").or_else(|| optional_str(args, "type_key"))
                && args.get("structure").is_none()
            {
                return Ok(vec![ModelChange::ReorderTypeAttributes {
                    type_key,
                    attributes,
                }]);
            }
            Ok(vec![ModelChange::ReorderAttributes {
                structure: required_str(args, "structure")?,
                class_key: class_ref(args)?,
                attributes,
            }])
        }
        "model.create_attribute" => Ok(vec![ModelChange::CreateAttribute {
            key: required_str(args, "key")?,
            label: args
                .get("label")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned(),
            kind: args
                .get("kind")
                .and_then(Value::as_str)
                .unwrap_or("text")
                .to_owned(),
            unit: args.get("unit").and_then(Value::as_str).map(str::to_owned),
            multiple: args
                .get("multiple")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            languages: args
                .get("languages")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            required: args
                .get("required")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            searchable: args
                .get("searchable")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            filterable: args
                .get("filterable")
                .and_then(Value::as_bool)
                .unwrap_or(true),
            sortable: args
                .get("sortable")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            bounds: bounds_from(args),
        }]),
        "model.attach_attribute" => Ok(vec![ModelChange::AttachAttribute {
            attribute: required_str(args, "attribute")?,
            type_key: optional_str(args, "type"),
            structure: optional_str(args, "structure"),
            group: optional_str(args, "group"),
        }]),
        "model.detach_attribute" => Ok(vec![ModelChange::DetachAttribute {
            attribute: required_str(args, "attribute")?,
            type_key: optional_str(args, "type"),
            structure: optional_str(args, "structure"),
            group: optional_str(args, "group"),
        }]),
        "model.update_type" => Ok(vec![ModelChange::UpdateType {
            key: required_str(args, "key")?,
            language: optional_str(args, "language"),
            singular: optional_str(args, "singular"),
            plural: optional_str(args, "plural"),
            structures: string_list(args, "structures"),
            defaults: string_map(args, "defaults"),
        }]),
        "model.update_attribute" => Ok(vec![ModelChange::UpdateAttribute {
            key: required_str(args, "key")?,
            label: optional_str(args, "label"),
            kind: optional_str(args, "kind"),
            unit: optional_str(args, "unit"),
            clear_unit: args
                .get("clear_unit")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            languages: args.get("languages").and_then(Value::as_bool),
            required: args.get("required").and_then(Value::as_bool),
            searchable: args.get("searchable").and_then(Value::as_bool),
            filterable: args.get("filterable").and_then(Value::as_bool),
            sortable: args.get("sortable").and_then(Value::as_bool),
            multiple: args.get("multiple").and_then(Value::as_bool),
            bounds: bounds_from(args),
        }]),
        "model.create_class" => Ok(vec![ModelChange::CreateClass {
            structure: required_str(args, "structure")?,
            class_key: required_str(args, "class")?,
            group: required_str(args, "group")?,
            parent: args
                .get("parent")
                .and_then(Value::as_str)
                .map(str::to_owned),
            attributes: vec![],
        }]),
        "model.set_parent" => Ok(vec![ModelChange::SetParent {
            structure: required_str(args, "structure")?,
            class_key: class_ref(args)?,
            parent: args
                .get("parent")
                .and_then(Value::as_str)
                .map(str::to_owned),
        }]),
        "model.rename_type_key" => Ok(vec![ModelChange::RenameTypeKey {
            key: current_ref(args)?,
            new_key: required_str(args, "new_key")?,
        }]),
        "model.rename_attribute_key" => Ok(vec![ModelChange::RenameAttributeKey {
            key: current_ref(args)?,
            new_key: required_str(args, "new_key")?,
        }]),
        "model.rename_structure_key" => Ok(vec![ModelChange::RenameStructureKey {
            structure: current_ref(args)?,
            new_key: required_str(args, "new_key")?,
        }]),
        "model.rename_group_key" => Ok(vec![ModelChange::RenameGroupKey {
            structure: required_str(args, "structure")?,
            group: args
                .get("group")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
                .unwrap_or(current_ref(args)?),
            new_key: required_str(args, "new_key")?,
        }]),
        "model.rename_value_list_key" => Ok(vec![ModelChange::RenameValueListKey {
            key: current_ref(args)?,
            new_key: required_str(args, "new_key")?,
        }]),
        "model.rename_unit_family_key" => Ok(vec![ModelChange::RenameUnitFamilyKey {
            key: current_ref(args)?,
            new_key: required_str(args, "new_key")?,
        }]),
        "model.create_structure" => Ok(vec![ModelChange::CreateStructure {
            key: required_str(args, "key")?,
            label: optional_str(args, "label").unwrap_or_default(),
        }]),
        "model.rename_structure" => Ok(vec![ModelChange::RenameStructure {
            structure: required_str(args, "structure")?,
            label: required_str(args, "label")?,
        }]),
        "model.delete_structure" => Ok(vec![ModelChange::DeleteStructure {
            structure: required_str(args, "structure")?,
        }]),
        "model.create_group" => Ok(vec![ModelChange::CreateGroup {
            structure: required_str(args, "structure")?,
            group: required_str(args, "group")?,
            parent: optional_str(args, "parent"),
            label: optional_str(args, "label").unwrap_or_default(),
        }]),
        "model.rename_group" => Ok(vec![ModelChange::RenameGroup {
            structure: required_str(args, "structure")?,
            group: required_str(args, "group")?,
            label: required_str(args, "label")?,
        }]),
        "model.move_group" => Ok(vec![ModelChange::MoveGroup {
            structure: required_str(args, "structure")?,
            group: required_str(args, "group")?,
            parent: optional_str(args, "parent"),
        }]),
        "model.delete_group" => Ok(vec![ModelChange::DeleteGroup {
            structure: required_str(args, "structure")?,
            group: required_str(args, "group")?,
            move_to: optional_str(args, "move_to"),
        }]),
        "model.delete_type" => Ok(vec![ModelChange::DeleteType {
            key: required_str(args, "type")?,
        }]),
        "model.delete_attribute" => Ok(vec![ModelChange::DeleteAttribute {
            key: required_str(args, "attribute")?,
        }]),
        "model.delete_value_list" => Ok(vec![ModelChange::DeleteValueList {
            key: required_str(args, "value_list")?,
        }]),
        "model.delete_value_option" => Ok(vec![ModelChange::DeleteValueOption {
            list: required_str(args, "value_list")?,
            option: required_str(args, "option")?,
        }]),
        "model.delete_unit_family" => Ok(vec![ModelChange::DeleteUnitFamily {
            key: required_str(args, "unit_family")?,
        }]),
        "model.delete_unit" => Ok(vec![ModelChange::DeleteUnit {
            family: required_str(args, "unit_family")?,
            unit: required_str(args, "unit")?,
        }]),
        "model.duplicate_group" => Ok(vec![ModelChange::DuplicateGroup {
            structure: required_str(args, "structure")?,
            group: required_str(args, "group")?,
            name: required_str(args, "name")?,
        }]),
        "model.create_type" => Ok(vec![ModelChange::CreateType {
            key: required_str(args, "key")?,
            label: optional_str(args, "label").unwrap_or_default(),
            plural: optional_str(args, "plural"),
            structures: string_list(args, "structures"),
            defaults: string_map(args, "defaults"),
        }]),
        "model.create_value_list" => Ok(vec![ModelChange::CreateValueList {
            key: required_str(args, "key")?,
            label: optional_str(args, "label").unwrap_or_default(),
            options: named_options(args, "options"),
        }]),
        "model.update_value_list" => Ok(vec![ModelChange::UpdateValueList {
            key: required_str(args, "key")?,
            label: optional_str(args, "label"),
            options: if args.get("options").is_some() {
                Some(named_options(args, "options"))
            } else {
                None
            },
        }]),
        "model.create_unit_family" => Ok(vec![ModelChange::CreateUnitFamily {
            key: required_str(args, "key")?,
            dimension: optional_str(args, "dimension"),
            base_unit: required_str(args, "base_unit")?,
            units: unit_specs(args),
        }]),
        "model.update_unit_family" => Ok(vec![ModelChange::UpdateUnitFamily {
            key: required_str(args, "key")?,
            dimension: optional_str(args, "dimension"),
            clear_dimension: args
                .get("clear_dimension")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            base_unit: optional_str(args, "base_unit"),
            units: if args.get("units").is_some() {
                Some(unit_specs(args))
            } else {
                None
            },
        }]),
        "model.update_structure" => Ok(vec![ModelChange::RenameStructure {
            structure: required_str(args, "structure")?,
            label: required_str(args, "label")?,
        }]),
        "model.reorder_type_attributes" => Ok(vec![ModelChange::ReorderTypeAttributes {
            type_key: args
                .get("type")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .unwrap_or(required_str(args, "type_key")?),
            attributes: string_list(args, "attributes").unwrap_or_default(),
        }]),
        "model.set_structures" => {
            let structures = args
                .get("structures")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default();
            Ok(vec![ModelChange::SetStructures {
                type_key: args
                    .get("type")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .unwrap_or(required_str(args, "type_key")?),
                structures,
            }])
        }
        other => Err(bad(format!("unknown model operation {other}"))),
    }
}

pub(crate) fn model_status(error: ApplyError) -> Problem {
    match error {
        ApplyError::Structure(error) => {
            let status = match error.code {
                StructureErrorCode::UnknownStructure | StructureErrorCode::UnknownGroup => {
                    StatusCode::NOT_FOUND
                }
                _ => StatusCode::UNPROCESSABLE_ENTITY,
            };
            Problem::new(status, error.code.as_str(), error.message)
        }
        ApplyError::Other(message) => {
            let status = if message.starts_with("unknown") {
                StatusCode::NOT_FOUND
            } else {
                StatusCode::BAD_REQUEST
            };
            Problem::new(status, "model", message)
        }
    }
}

fn bounds_from(args: &Value) -> braian_model::AttributeBounds {
    braian_model::AttributeBounds {
        min: optional_str(args, "min"),
        max: optional_str(args, "max"),
        clear_min: args
            .get("clear_min")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        clear_max: args
            .get("clear_max")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        integer: args.get("integer").and_then(Value::as_bool),
        max_length: args
            .get("max_length")
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok()),
        clear_max_length: args
            .get("clear_max_length")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        text_form: optional_str(args, "text_form"),
        value_list: optional_str(args, "value_list"),
        clear_value_list: args
            .get("clear_value_list")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        unit_family: optional_str(args, "unit_family"),
    }
}

fn string_list(args: &Value, key: &str) -> Option<Vec<String>> {
    args.get(key).and_then(Value::as_array).map(|items| {
        items
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect()
    })
}

fn string_map(args: &Value, key: &str) -> Option<std::collections::BTreeMap<String, String>> {
    args.get(key).and_then(Value::as_object).map(|map| {
        map.iter()
            .filter_map(|(key, value)| value.as_str().map(|text| (key.clone(), text.to_owned())))
            .collect()
    })
}

fn named_options(args: &Value, key: &str) -> Vec<braian_model::NamedOption> {
    args.get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let code = item
                        .get("code")
                        .or_else(|| item.get("key"))
                        .and_then(Value::as_str)?;
                    Some(braian_model::NamedOption {
                        code: code.to_owned(),
                        label: item
                            .get("label")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_owned(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn unit_specs(args: &Value) -> Vec<braian_model::UnitSpec> {
    args.get("units")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let code = item.get("code").and_then(Value::as_str)?;
                    Some(braian_model::UnitSpec {
                        code: code.to_owned(),
                        factor: item
                            .get("factor")
                            .and_then(Value::as_str)
                            .unwrap_or("1")
                            .to_owned(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn text_arg(args: &Value) -> &str {
    args.get("text").and_then(Value::as_str).unwrap_or("")
}

fn optional_str(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn required_str(args: &Value, key: &str) -> Result<String, Problem> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| Problem::new(StatusCode::BAD_REQUEST, "model", format!("missing {key}")))
}

/// Class identifier on steps that do not already have a destination group.
/// `group` is the name for one release; `class` and `class_key` still work.
fn class_ref(args: &Value) -> Result<String, Problem> {
    for key in ["group", "class", "class_key"] {
        if let Some(value) = args
            .get(key)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
        {
            return Ok(value.to_owned());
        }
    }
    Err(Problem::new(
        StatusCode::BAD_REQUEST,
        "model",
        "missing group",
    ))
}

fn current_ref(args: &Value) -> Result<String, Problem> {
    for key in ["ref", "key", "id", "structure"] {
        if let Some(value) = args
            .get(key)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
        {
            return Ok(value.to_owned());
        }
    }
    Err(Problem::new(
        StatusCode::BAD_REQUEST,
        "model",
        "missing key",
    ))
}

fn title_for(args: &Value, lines: &[String]) -> String {
    if let Some(title) = args.get("title").and_then(Value::as_str)
        && !title.is_empty()
    {
        return title.to_owned();
    }
    if let Some(text) = args.get("text").and_then(Value::as_str)
        && !text.is_empty()
    {
        return text.to_owned();
    }
    lines
        .first()
        .cloned()
        .unwrap_or_else(|| "Data model".into())
}
