//! Targets, deterministic matching, and the two Decisions calls.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::time::Instant;

use braian_model::{
    Attribute, AttributeId, AttributeKind, DefinitionSet, Group, GroupId, LanguageTag, Structure,
};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::decisions::{
    Answer, ChoiceOption, ChoiceValue, Decider, DecisionRequest, DecisionResponse, Probability,
    choice_question, explicit_cache, shared_input, usage_record, within_json_limits,
};
use super::parse::{HeaderCol, normalize_header};

const LOW: f64 = 0.6;
const MAX_CHOICES: usize = 255;
const MAX_QUESTIONS: usize = 400;
const TENANT: &str = "braian";

#[derive(Debug, Clone, PartialEq)]
pub struct Target {
    pub id: String,
    pub label: String,
    pub kind: String,
    pub key_attribute: bool,
    pub source: String,
    pub attribute_key: Option<String>,
    pub language: Option<String>,
    pub structure_key: Option<String>,
    pub group_key: Option<String>,
    pub unit: Option<String>,
    pub attribute_id: Option<String>,
    pub structure_id: Option<String>,
    pub group_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct HeaderInput {
    pub index: usize,
    pub raw: String,
    #[allow(dead_code)]
    pub name: String,
    pub empty_ratio: f64,
    pub samples: Vec<String>,
}

impl HeaderInput {
    pub fn from_parsed(headers: &[HeaderCol], sample: &[Vec<String>]) -> Vec<Self> {
        headers
            .iter()
            .map(|header| {
                let samples = sample
                    .iter()
                    .filter_map(|row| row.get(header.index))
                    .filter(|cell| !cell.is_empty())
                    .take(20)
                    .cloned()
                    .collect();
                Self {
                    index: header.index,
                    raw: header.raw.clone(),
                    name: header.name.clone(),
                    empty_ratio: header.empty_ratio,
                    samples,
                }
            })
            .collect()
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Proposal {
    pub ai: String,
    #[serde(rename = "type")]
    pub type_choice: ChoiceProposal,
    pub id_column: ChoiceProposal,
    pub columns: Vec<ColumnProposal>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ChoiceProposal {
    pub value: Option<String>,
    pub confidence: f64,
    pub source: String,
    pub alternatives: Vec<Alt>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ColumnProposal {
    pub col: usize,
    pub target: String,
    pub confidence: f64,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub alternatives: Vec<Alt>,
    pub low: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Alt {
    pub value: String,
    pub p: f64,
}

#[derive(Debug, Clone)]
pub struct ManualLine {
    pub col: usize,
    pub target: String,
}

#[derive(Debug, Clone)]
struct Scored {
    target: String,
    score: f64,
}

pub fn targets_for(defs: &DefinitionSet, type_key: &str) -> Vec<Target> {
    if super::entity::is_model(type_key) {
        return super::entity::targets(type_key);
    }
    let Some(object_type) = defs.types.iter().find(|item| {
        item.key == type_key || item.id.as_uuid().to_string().eq_ignore_ascii_case(type_key)
    }) else {
        return Vec::new();
    };
    let key_attribute = defs
        .attributes
        .iter()
        .find(|attribute| attribute.id == object_type.key_attribute)
        .map(|attribute| attribute.key.clone());
    let mut provenance: Vec<(AttributeId, Provenance)> = Vec::new();
    let mut seen = HashSet::new();
    for id in &object_type.attributes {
        if seen.insert(*id) {
            provenance.push((
                *id,
                Provenance::Type {
                    type_key: object_type.key.clone(),
                    type_label: object_type.singular_label("en"),
                },
            ));
        }
    }
    let mut structures: Vec<&Structure> = defs.structures.iter().collect();
    structures.sort_by(|left, right| left.key.cmp(&right.key));
    for structure in structures {
        for group in groups_in_order(structure) {
            let mut ids = group.attributes.clone();
            for class in &structure.classes {
                if class.group == group.id {
                    ids.extend(class.attributes.iter().copied());
                }
            }
            for id in ids {
                if seen.insert(id) {
                    provenance.push((
                        id,
                        Provenance::Group {
                            structure: structure.key.clone(),
                            group: group.key.clone(),
                        },
                    ));
                }
            }
        }
    }
    let mut targets = Vec::new();
    for (id, source) in provenance {
        let Some(attribute) = defs.attributes.iter().find(|item| item.id == id) else {
            continue;
        };
        targets.extend(attribute_targets(
            defs,
            attribute,
            &source,
            key_attribute.as_deref() == Some(attribute.key.as_str()),
        ));
    }
    for structure in defs.structures.iter() {
        targets.push(Target {
            id: format!("membership:{}", structure.key),
            label: format!("Group in {}", structure.key),
            kind: "membership".to_owned(),
            key_attribute: false,
            source: structure.key.clone(),
            attribute_key: None,
            language: None,
            structure_key: Some(structure.key.clone()),
            group_key: None,
            unit: None,
            attribute_id: None,
            structure_id: Some(structure.id.as_uuid().to_string()),
            group_id: None,
        });
    }
    push_link_targets(defs, object_type, &mut targets);
    targets.sort_by(|left, right| left.id.cmp(&right.id));
    targets
}

enum Provenance {
    Type {
        #[allow(dead_code)]
        type_key: String,
        type_label: String,
    },
    Group {
        structure: String,
        group: String,
    },
}

fn push_link_targets(
    defs: &DefinitionSet,
    object_type: &braian_model::ObjectType,
    targets: &mut Vec<Target>,
) {
    let taken = |targets: &[Target], key: &str| {
        targets
            .iter()
            .any(|target| target.attribute_key.as_deref() == Some(key))
    };
    for link in defs.types.iter().filter(|item| item.is_link()) {
        let Some(spec) = link.link.as_ref() else {
            continue;
        };
        if spec.from_types.contains(&object_type.id) && !taken(targets, &spec.to_label.key) {
            targets.push(link_target(
                link,
                &spec.to_label.key,
                &spec.to_label.text("en"),
                true,
            ));
        }
        if spec.to_types.contains(&object_type.id) && !taken(targets, &spec.from_label.key) {
            targets.push(link_target(
                link,
                &spec.from_label.key,
                &spec.from_label.text("en"),
                false,
            ));
        }
    }
}

fn link_target(link: &braian_model::ObjectType, key: &str, label: &str, outgoing: bool) -> Target {
    Target {
        id: format!("link:{}:{}", link.key, if outgoing { "out" } else { "in" }),
        label: label.to_owned(),
        kind: "link".to_owned(),
        key_attribute: false,
        source: label.to_owned(),
        attribute_key: Some(key.to_owned()),
        language: None,
        structure_key: None,
        group_key: None,
        unit: None,
        attribute_id: Some(link.id.as_uuid().to_string()),
        structure_id: None,
        group_id: None,
    }
}

fn attribute_targets(
    defs: &DefinitionSet,
    attribute: &Attribute,
    source: &Provenance,
    key_attribute: bool,
) -> Vec<Target> {
    let kind = kind_name(&attribute.kind);
    let unit = unit_of(defs, attribute);
    let (id_prefix, source_label) = match source {
        Provenance::Type { type_label, .. } => {
            (format!("type:{}", attribute.key), type_label.clone())
        }
        Provenance::Group { structure, group } => (
            format!("group:{structure}/{group}:{}", attribute.key),
            format!("{structure} / {group}"),
        ),
    };
    if !attribute.language_dependent || defs.languages.is_empty() {
        let mut targets = vec![Target {
            id: id_prefix.clone(),
            label: label_of(&attribute.labels, &attribute.key),
            kind: kind.into(),
            key_attribute,
            source: source_label.clone(),
            attribute_key: Some(attribute.key.clone()),
            language: None,
            structure_key: structure_key(source),
            group_key: group_key(source),
            unit,
            attribute_id: Some(attribute.id.as_uuid().to_string()),
            structure_id: structure_id_of(defs, source),
            group_id: group_id_of(defs, source),
        }];
        targets.extend(part_targets(
            attribute,
            &id_prefix,
            &source_label,
            source,
            defs,
            kind,
        ));
        return targets;
    }
    let mut targets = Vec::new();
    let default_lang = defs.languages[0].as_str().to_owned();
    targets.push(Target {
        id: id_prefix.clone(),
        label: format!(
            "{} ({default_lang})",
            language_label(&attribute.labels, &defs.languages[0], &attribute.key)
        ),
        kind: kind.into(),
        key_attribute,
        source: source_label.clone(),
        attribute_key: Some(attribute.key.clone()),
        language: Some(default_lang.clone()),
        structure_key: structure_key(source),
        group_key: group_key(source),
        unit: unit.clone(),
        attribute_id: Some(attribute.id.as_uuid().to_string()),
        structure_id: structure_id_of(defs, source),
        group_id: group_id_of(defs, source),
    });
    for language in &defs.languages {
        // The unsuffixed target already writes the default language, and it
        // carries the same label. A second `@sv` row is the duplicate the
        // picker showed as "Description (sv)" twice.
        if language.as_str() == default_lang {
            continue;
        }
        targets.push(Target {
            id: format!("{id_prefix}@{}", language.as_str()),
            label: format!(
                "{} ({})",
                language_label(&attribute.labels, language, &attribute.key),
                language.as_str()
            ),
            kind: kind.into(),
            key_attribute: false,
            source: source_label.clone(),
            attribute_key: Some(attribute.key.clone()),
            language: Some(language.as_str().to_owned()),
            structure_key: structure_key(source),
            group_key: group_key(source),
            unit: unit.clone(),
            attribute_id: Some(attribute.id.as_uuid().to_string()),
            structure_id: structure_id_of(defs, source),
            group_id: group_id_of(defs, source),
        });
    }
    targets.extend(part_targets(
        attribute,
        &id_prefix,
        &source_label,
        source,
        defs,
        kind,
    ));
    targets
}

fn part_targets(
    attribute: &Attribute,
    id_prefix: &str,
    source_label: &str,
    source: &Provenance,
    defs: &DefinitionSet,
    kind: &str,
) -> Vec<Target> {
    braian_model::extra_parts(&attribute.kind)
        .into_iter()
        .map(|part| {
            let word = part.word();
            let part_kind = if matches!(
                part,
                braian_model::PathPart::Unit | braian_model::PathPart::Label
            ) {
                "text"
            } else {
                kind
            };
            Target {
                id: format!("{id_prefix}.{word}"),
                label: format!("{} ({word})", label_of(&attribute.labels, &attribute.key)),
                kind: part_kind.to_owned(),
                key_attribute: false,
                source: source_label.to_owned(),
                attribute_key: Some(attribute.key.clone()),
                language: None,
                structure_key: structure_key(source),
                group_key: group_key(source),
                unit: unit_of(defs, attribute),
                attribute_id: Some(attribute.id.as_uuid().to_string()),
                structure_id: structure_id_of(defs, source),
                group_id: group_id_of(defs, source),
            }
        })
        .collect()
}

pub(crate) fn path_part(id: &str) -> Option<&str> {
    let body = id
        .rsplit_once('@')
        .filter(|(_, language)| !language.is_empty() && !language.contains(':'))
        .map(|(body, _)| body)
        .unwrap_or(id);
    let part = body.rsplit_once('.')?.1;
    matches!(part, "value" | "unit" | "min" | "max" | "code" | "label").then_some(part)
}

fn structure_key(source: &Provenance) -> Option<String> {
    match source {
        Provenance::Group { structure, .. } => Some(structure.clone()),
        Provenance::Type { .. } => None,
    }
}

fn group_key(source: &Provenance) -> Option<String> {
    match source {
        Provenance::Group { group, .. } => Some(group.clone()),
        Provenance::Type { .. } => None,
    }
}

fn groups_in_order(structure: &Structure) -> Vec<&Group> {
    let mut by_parent: BTreeMap<Option<GroupId>, Vec<&Group>> = BTreeMap::new();
    let mut groups: Vec<&Group> = structure.groups.iter().collect();
    groups.sort_by(|left, right| left.key.cmp(&right.key));
    for group in groups {
        by_parent.entry(group.parent).or_default().push(group);
    }
    let mut ordered = Vec::new();
    fn walk<'a>(
        parent: Option<GroupId>,
        by_parent: &BTreeMap<Option<GroupId>, Vec<&'a Group>>,
        ordered: &mut Vec<&'a Group>,
    ) {
        let Some(children) = by_parent.get(&parent) else {
            return;
        };
        for child in children {
            ordered.push(child);
            walk(Some(child.id), by_parent, ordered);
        }
    }
    walk(None, &by_parent, &mut ordered);
    ordered
}

pub fn kind_name(kind: &AttributeKind) -> &'static str {
    match kind {
        AttributeKind::Text { .. } => "text",
        AttributeKind::Number { .. } => "number",
        AttributeKind::Range { .. } => "range",
        AttributeKind::ValueList { .. } => "value_list",
        AttributeKind::Boolean => "boolean",
        AttributeKind::Date => "date",
        AttributeKind::ObjectRef { .. } => "object_ref",
        AttributeKind::FileRef { .. } => "file_ref",
    }
}

fn unit_of(defs: &DefinitionSet, attribute: &Attribute) -> Option<String> {
    let family = attribute.kind.unit_family_id()?;
    defs.unit_families
        .iter()
        .find(|item| item.id == family)
        .map(|item| item.base_unit.clone())
}

fn label_of(labels: &BTreeMap<LanguageTag, String>, fallback: &str) -> String {
    for (tag, label) in labels {
        let text = tag.as_str();
        if text == "en" || text.starts_with("en-") {
            return label.clone();
        }
    }
    labels
        .values()
        .next()
        .cloned()
        .unwrap_or_else(|| fallback.to_owned())
}

fn language_label(
    labels: &BTreeMap<LanguageTag, String>,
    language: &LanguageTag,
    fallback: &str,
) -> String {
    labels
        .get(language)
        .cloned()
        .unwrap_or_else(|| fallback.to_owned())
}

pub fn score_column(header: &str, target: &Target, units: &[String]) -> f64 {
    if target.id == "unmapped" {
        return 0.0;
    }
    let name = normalize_header(header);
    if name.is_empty() {
        return 0.0;
    }
    let Some(key) = target.attribute_key.as_deref() else {
        let membership = target
            .structure_key
            .as_deref()
            .map(normalize_header)
            .unwrap_or_default();
        return if name == membership || name == format!("group_{membership}") {
            1.0
        } else {
            jw_score(&name, &membership)
        };
    };
    let key_norm = normalize_header(key);
    let label_norm = normalize_header(&target.label);
    if let Some(part) = path_part(&target.id) {
        let part_norm = normalize_header(part);
        if name == label_norm
            || name == format!("{key_norm}_{part_norm}")
            || name == format!("{key_norm}{part_norm}")
        {
            return 1.0;
        }
        // "net_weight" is the whole attribute, not every part of it.
        if name == key_norm || name == normalize_header(label_without_language(&target.label)) {
            return 0.0;
        }
    }
    // "Name (en)" is the same field as a header "Name".
    let bare_norm = normalize_header(label_without_language(&target.label));
    let exact_labels = match &target.language {
        Some(language) if target.id.contains('@') => {
            name == format!("{key_norm}_{}", normalize_header(language))
                || name == label_norm
                || name == bare_norm
        }
        Some(_) => name == key_norm || name == label_norm || name == bare_norm,
        None => name == key_norm || name == label_norm || name == bare_norm,
    };
    if exact_labels && !(target.id.contains('@') && name == key_norm) {
        return 1.0;
    }
    if let Some(language) = &target.language
        && target.id.contains('@')
        && name == format!("{key_norm}_{}", normalize_header(language))
    {
        return 0.97;
    }
    if !target.id.contains('@')
        && let Some(rest) = name.strip_prefix(&format!("{key_norm}_"))
        && (units.iter().any(|unit| normalize_header(unit) == rest)
            || target
                .language
                .as_ref()
                .is_some_and(|language| normalize_header(language) == rest))
    {
        return 0.97;
    }
    if target.key_attribute && is_synonym(&name, &["sku", "article_no", "item_no", "part_no"]) {
        return 0.95;
    }
    if key_norm == "gtin" && is_synonym(&name, &["ean", "gtin", "barcode"]) {
        return 0.95;
    }
    if target.id.contains('@') {
        let lang = target.language.as_deref().unwrap_or("");
        // "Description" is the attribute, not evidence for every language.
        // Jaro otherwise scores `description` against `description_en` at
        // about 0.86 and the column looks certain while listing rivals.
        if name == key_norm {
            return 0.0;
        }
        return jw_score(&name, &format!("{key_norm}_{}", normalize_header(lang)))
            .max(jw_score(&name, &label_norm))
            .max(jw_score(&name, &bare_norm));
    }
    jw_score(&name, &key_norm)
        .max(jw_score(&name, &label_norm))
        .max(jw_score(&name, &bare_norm))
}

fn label_without_language(label: &str) -> &str {
    let Some(open) = label.rfind(" (") else {
        return label;
    };
    if label.ends_with(')') {
        &label[..open]
    } else {
        label
    }
}

fn is_synonym(name: &str, words: &[&str]) -> bool {
    words.contains(&name)
}

fn jw_score(left: &str, right: &str) -> f64 {
    if left.is_empty() || right.is_empty() {
        return 0.0;
    }
    let score = strsim::jaro_winkler(left, right);
    if score >= 0.92 { score * 0.9 } else { 0.0 }
}

fn rank_targets(header: &str, targets: &[Target], units: &[String]) -> Vec<Scored> {
    let mut scored: Vec<Scored> = targets
        .iter()
        .map(|target| Scored {
            target: target.id.clone(),
            score: score_column(header, target, units),
        })
        .filter(|item| item.score > 0.0)
        .collect();
    scored.sort_by(|left, right| {
        right
            .score
            .partial_cmp(&left.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                let left_link = left.target.starts_with("link:");
                let right_link = right.target.starts_with("link:");
                left_link
                    .cmp(&right_link)
                    .then_with(|| left.target.cmp(&right.target))
            })
    });
    scored
}

fn decided(scored: &[Scored]) -> Option<&Scored> {
    let best = scored.first()?;
    if best.score < 0.95 {
        return None;
    }
    let second = scored.get(1).map(|item| item.score).unwrap_or(0.0);
    if second < 0.90 { Some(best) } else { None }
}

pub fn type_coverage(defs: &DefinitionSet, headers: &[HeaderInput]) -> Vec<(String, usize)> {
    let mut rows = Vec::new();
    for object_type in &defs.types {
        let targets = targets_for(defs, &object_type.key);
        let units = unit_codes(defs);
        let mut hits = 0usize;
        for header in headers {
            if header.empty_ratio >= 1.0 {
                continue;
            }
            let ranked = rank_targets(&header.raw, &targets, &units);
            if ranked.first().is_some_and(|item| item.score >= 1.0) {
                hits += 1;
            }
        }
        rows.push((object_type.key.clone(), hits));
    }
    rows.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    rows
}

fn unit_codes(defs: &DefinitionSet) -> Vec<String> {
    defs.unit_families
        .iter()
        .flat_map(|family| family.units.iter().map(|unit| unit.code.clone()))
        .collect()
}

#[derive(Clone)]
struct ColumnDraft {
    col: usize,
    target: String,
    confidence: f64,
    source: String,
    reason: Option<String>,
    alternatives: Vec<Alt>,
    pinned: bool,
    candidates: Vec<Alt>,
}

#[allow(clippy::too_many_arguments)]
pub async fn propose<D: Decider>(
    decider: Option<&D>,
    defs: &DefinitionSet,
    headers: &[HeaderInput],
    file_name: &str,
    sheet: &str,
    owner: Uuid,
    forced_type: Option<&str>,
    forced_id: Option<Option<usize>>,
    manual: &[ManualLine],
) -> (Proposal, Value) {
    let units = unit_codes(defs);
    let coverage = type_coverage(defs, headers);
    let mut type_choice = match forced_type {
        Some(key) => ChoiceProposal {
            value: Some(key.to_owned()),
            confidence: 1.0,
            source: "manual".into(),
            alternatives: Vec::new(),
        },
        None if defs.types.len() == 1 => ChoiceProposal {
            value: Some(defs.types[0].key.clone()),
            confidence: 1.0,
            source: "deterministic".into(),
            alternatives: Vec::new(),
        },
        None => {
            let decided = match coverage.as_slice() {
                [(key, top), (_, second), ..] if *top > 0 && *top >= second.saturating_mul(2) => {
                    Some(key.clone())
                }
                [(key, top)] if *top > 0 => Some(key.clone()),
                _ => None,
            };
            ChoiceProposal {
                value: decided,
                confidence: if coverage.first().is_some_and(|(_, hits)| *hits > 0) {
                    1.0
                } else {
                    0.0
                },
                source: "deterministic".into(),
                alternatives: coverage
                    .iter()
                    .skip(1)
                    .take(3)
                    .map(|(key, hits)| Alt {
                        value: key.clone(),
                        p: if coverage[0].1 == 0 {
                            0.0
                        } else {
                            *hits as f64 / coverage[0].1 as f64
                        },
                    })
                    .collect(),
            }
        }
    };
    let type_known_before = type_choice.value.clone();
    let mut id_choice = ChoiceProposal {
        value: forced_id.flatten().map(|index| format!("col:{index}")).or(
            if forced_id == Some(None) {
                Some("none".into())
            } else {
                None
            },
        ),
        confidence: if forced_id.is_some() { 1.0 } else { 0.0 },
        source: if forced_id.is_some() {
            "manual".into()
        } else {
            "deterministic".into()
        },
        alternatives: Vec::new(),
    };
    if forced_id.is_none()
        && let Some(type_key) = type_choice.value.clone()
        && let Some((column, confidence)) = deterministic_id(defs, &type_key, headers, &units)
    {
        id_choice = ChoiceProposal {
            value: Some(format!("col:{column}")),
            confidence,
            source: "deterministic".into(),
            alternatives: Vec::new(),
        };
    }
    let mut usage = Vec::new();
    let mut ai = "pending";
    let model_json = model_summary(defs);
    let file_json = file_summary(file_name, sheet, headers);
    let input = shared_input(model_json, file_json);
    let call1 = call1_questions(defs, headers, &type_choice, &id_choice, &coverage);
    if !call1.is_empty() {
        if let Some(decider) = decider {
            let questions = call1;
            {
                let request = DecisionRequest {
                    model: super::decisions::DECISIONS_MODEL,
                    prompt_cache_key: cache_key(defs.model_revision),
                    safety_identifier: safety_id(owner),
                    prompt_cache_options: explicit_cache(),
                    input: input.clone(),
                    questions,
                };
                let started = Instant::now();
                match decider.decide(&request).await {
                    Ok(response) => {
                        apply_call1(
                            &request.questions,
                            &response,
                            &mut type_choice,
                            &mut id_choice,
                        );
                        usage.push(usage_record(
                            "type",
                            request.questions.len(),
                            started.elapsed(),
                            response.usage.as_ref(),
                        ));
                        ai = "used";
                    }
                    Err(error) => {
                        tracing::info!(%error, "decisions type call unavailable");
                        ai = "unavailable";
                    }
                }
            }
        } else {
            ai = "unavailable";
        }
    }
    let type_key = type_choice.value.clone().or(type_known_before);
    let targets = type_key
        .as_deref()
        .map(|key| targets_for(defs, key))
        .unwrap_or_default();
    let mut drafts = deterministic_columns(headers, &targets, &units);
    for line in manual {
        if let Some(draft) = drafts.iter_mut().find(|draft| draft.col == line.col)
            && (line.target == "unmapped" || targets.iter().any(|target| target.id == line.target))
        {
            draft.target = line.target.clone();
            draft.confidence = 1.0;
            draft.source = "manual".into();
            draft.reason = None;
            draft.pinned = true;
            draft.alternatives.clear();
        }
    }
    let need_columns = drafts
        .iter()
        .any(|draft| !draft.pinned && draft.reason.as_deref() != Some("empty"));
    if need_columns && ai != "unavailable" {
        if let Some(decider) = decider {
            let batches = call2_questions(headers, &drafts, &targets, &units);
            if batches.is_empty() {
                if ai == "pending" {
                    ai = "skipped";
                }
            } else {
                let mut failed = false;
                let mut answers = Vec::new();
                for questions in batches {
                    let request = DecisionRequest {
                        model: super::decisions::DECISIONS_MODEL,
                        prompt_cache_key: cache_key(defs.model_revision),
                        safety_identifier: safety_id(owner),
                        prompt_cache_options: explicit_cache(),
                        input: input.clone(),
                        questions,
                    };
                    let started = Instant::now();
                    match decider.decide(&request).await {
                        Ok(response) => {
                            usage.push(usage_record(
                                "columns",
                                request.questions.len(),
                                started.elapsed(),
                                response.usage.as_ref(),
                            ));
                            answers.push((request.questions, response));
                        }
                        Err(error) => {
                            tracing::info!(%error, "decisions column call unavailable");
                            failed = true;
                            break;
                        }
                    }
                }
                if failed {
                    ai = "unavailable";
                } else {
                    apply_call2(&answers, &mut drafts);
                    ai = "used";
                }
            }
        } else if ai == "pending" {
            ai = "unavailable";
        }
    } else if ai == "pending" {
        ai = "skipped";
    }
    if ai == "unavailable" {
        for draft in &mut drafts {
            if draft.source == "decisions" {
                draft.target = "unmapped".into();
                draft.source = "deterministic".into();
                draft.confidence = 0.0;
                draft.reason = Some("low_confidence".into());
            }
        }
    }
    let columns = dedupe_drafts(drafts);
    if ai == "pending" {
        ai = "skipped";
    }
    (
        Proposal {
            ai: ai.to_owned(),
            type_choice,
            id_column: id_choice,
            columns,
        },
        Value::Array(usage),
    )
}

fn deterministic_id(
    defs: &DefinitionSet,
    type_key: &str,
    headers: &[HeaderInput],
    units: &[String],
) -> Option<(usize, f64)> {
    let targets = targets_for(defs, type_key);
    let key = targets.iter().find(|target| target.key_attribute)?;
    for header in headers {
        if header.empty_ratio >= 1.0 {
            continue;
        }
        let ranked = rank_targets(&header.raw, &targets, units);
        let Some(pinned) = decided(&ranked) else {
            continue;
        };
        if pinned.target == key.id && sample_is_identifier(&header.samples) {
            return Some((header.index, pinned.score));
        }
    }
    None
}

fn sample_is_identifier(samples: &[String]) -> bool {
    if samples.is_empty() {
        return false;
    }
    let nonempty: Vec<&str> = samples
        .iter()
        .map(String::as_str)
        .filter(|cell| !cell.is_empty())
        .collect();
    if nonempty.len() as f64 / samples.len() as f64 + f64::EPSILON < 0.99 {
        return false;
    }
    let unique: BTreeSet<&str> = nonempty.iter().copied().collect();
    unique.len() == nonempty.len()
}

fn deterministic_columns(
    headers: &[HeaderInput],
    targets: &[Target],
    units: &[String],
) -> Vec<ColumnDraft> {
    headers
        .iter()
        .map(|header| {
            if header.empty_ratio >= 1.0 {
                return ColumnDraft {
                    col: header.index,
                    target: "unmapped".into(),
                    confidence: 1.0,
                    source: "deterministic".into(),
                    reason: Some("empty".into()),
                    alternatives: Vec::new(),
                    pinned: true,
                    candidates: Vec::new(),
                };
            }
            let ranked = rank_targets(&header.raw, targets, units);
            if let Some(best) = decided(&ranked) {
                ColumnDraft {
                    col: header.index,
                    target: best.target.clone(),
                    confidence: best.score,
                    source: "deterministic".into(),
                    reason: None,
                    alternatives: alts_from_scored(&ranked, &best.target),
                    pinned: true,
                    candidates: ranked
                        .iter()
                        .map(|item| Alt {
                            value: item.target.clone(),
                            p: item.score,
                        })
                        .collect(),
                }
            } else {
                let alternatives = ranked
                    .iter()
                    .take(3)
                    .filter(|item| item.score >= 0.05)
                    .map(|item| Alt {
                        value: item.target.clone(),
                        p: item.score,
                    })
                    .collect();
                ColumnDraft {
                    col: header.index,
                    target: "unmapped".into(),
                    confidence: ranked.first().map(|item| item.score).unwrap_or(0.0),
                    source: "deterministic".into(),
                    reason: Some("low_confidence".into()),
                    alternatives,
                    pinned: false,
                    candidates: ranked
                        .iter()
                        .map(|item| Alt {
                            value: item.target.clone(),
                            p: item.score,
                        })
                        .collect(),
                }
            }
        })
        .collect()
}

fn alts_from_scored(ranked: &[Scored], chosen: &str) -> Vec<Alt> {
    ranked
        .iter()
        .filter(|item| item.target != chosen && item.score >= 0.05)
        .take(3)
        .map(|item| Alt {
            value: item.target.clone(),
            p: item.score,
        })
        .collect()
}

fn call1_questions(
    defs: &DefinitionSet,
    headers: &[HeaderInput],
    type_choice: &ChoiceProposal,
    id_choice: &ChoiceProposal,
    coverage: &[(String, usize)],
) -> Vec<super::decisions::ChoiceQuestion> {
    let mut questions = Vec::new();
    if type_choice.value.is_none() && defs.types.len() >= 2 {
        let mut types: Vec<&braian_model::ObjectType> = defs.types.iter().collect();
        types.sort_by(|left, right| {
            let left_hits = coverage
                .iter()
                .find(|(key, _)| key == &left.key)
                .map(|(_, hits)| *hits)
                .unwrap_or(0);
            let right_hits = coverage
                .iter()
                .find(|(key, _)| key == &right.key)
                .map(|(_, hits)| *hits)
                .unwrap_or(0);
            right_hits
                .cmp(&left_hits)
                .then_with(|| left.key.cmp(&right.key))
        });
        types.truncate(MAX_CHOICES);
        if types.len() >= 2 {
            questions.push(choice_question(
                "type",
                "Which object type does each row of this file describe?",
                types
                    .into_iter()
                    .map(|object_type| ChoiceOption {
                        value: object_type.key.clone(),
                        description: Some(type_description(defs, object_type)),
                    })
                    .collect(),
            ));
        }
    }
    if id_choice.value.is_none() {
        let mut columns: Vec<&HeaderInput> = headers
            .iter()
            .filter(|header| header.empty_ratio < 1.0)
            .collect();
        columns.sort_by(|left, right| {
            identifier_rank(right)
                .partial_cmp(&identifier_rank(left))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        columns.truncate(254);
        let mut choices: Vec<ChoiceOption> = columns
            .into_iter()
            .map(|header| ChoiceOption {
                value: format!("col:{}", header.index),
                description: Some(column_description(header)),
            })
            .collect();
        choices.push(ChoiceOption {
            value: "none".into(),
            description: Some("No column identifies existing objects; every row is new".into()),
        });
        if choices.len() >= 2 {
            questions.push(choice_question(
                "id_column",
                "Which column holds the unique identifier (the key attribute) of each object, used to find existing objects?",
                choices,
            ));
        }
    }
    questions
}

fn type_description(defs: &DefinitionSet, object_type: &braian_model::ObjectType) -> String {
    let label = object_type.singular_label("en");
    let mut keys = Vec::new();
    for id in object_type.attributes.iter().take(30) {
        if let Some(attribute) = defs.attributes.iter().find(|item| item.id == *id) {
            keys.push(attribute.key.clone());
        }
    }
    format!("{label}. attributes: {}", keys.join(", "))
}

fn column_description(header: &HeaderInput) -> String {
    let samples: Vec<&str> = header.samples.iter().take(3).map(String::as_str).collect();
    if samples.is_empty() {
        header.raw.clone()
    } else {
        format!("{} — e.g. {}", header.raw, samples.join(", "))
    }
}

fn identifier_rank(header: &HeaderInput) -> f64 {
    if header.samples.is_empty() {
        return 0.0;
    }
    let nonempty = header
        .samples
        .iter()
        .filter(|cell| !cell.is_empty())
        .count();
    let unique = header.samples.iter().collect::<BTreeSet<_>>().len();
    nonempty as f64 / header.samples.len() as f64 * unique as f64 / header.samples.len() as f64
}

fn call2_questions(
    headers: &[HeaderInput],
    drafts: &[ColumnDraft],
    targets: &[Target],
    units: &[String],
) -> Vec<Vec<super::decisions::ChoiceQuestion>> {
    let mut questions = Vec::new();
    for draft in drafts {
        if draft.pinned || draft.reason.as_deref() == Some("empty") {
            continue;
        }
        let Some(header) = headers.iter().find(|header| header.index == draft.col) else {
            continue;
        };
        let choices = cap_choices(header, targets, units);
        if choices.len() < 2 {
            continue;
        }
        let raw = header.raw.replace('"', "'");
        questions.push(choice_question(
            format!("col:{}", draft.col),
            format!("Which data model field should column \"{raw}\" be imported into?"),
            choices,
        ));
    }
    questions
        .chunks(MAX_QUESTIONS)
        .map(|chunk| chunk.to_vec())
        .collect()
}

pub fn cap_choices(
    header: &HeaderInput,
    targets: &[Target],
    units: &[String],
) -> Vec<ChoiceOption> {
    let mut ranked = rank_targets(&header.raw, targets, units);
    for target in targets {
        if !ranked.iter().any(|item| item.target == target.id) {
            let bonus = if compatible(header, target) {
                0.001
            } else {
                0.0
            };
            ranked.push(Scored {
                target: target.id.clone(),
                score: bonus,
            });
        }
    }
    ranked.sort_by(|left, right| {
        right
            .score
            .partial_cmp(&left.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.target.cmp(&right.target))
    });
    let mut picked: Vec<String> = ranked
        .into_iter()
        .map(|item| item.target)
        .take(MAX_CHOICES - 1)
        .collect();
    picked.push("unmapped".into());
    picked.truncate(MAX_CHOICES);
    picked
        .into_iter()
        .map(|id| {
            let description = if id == "unmapped" {
                Some("Do not import this column".into())
            } else {
                targets.iter().find(|target| target.id == id).map(|target| {
                    format!(
                        "{} · {} · {} · from {}",
                        target.label,
                        target.kind,
                        target
                            .language
                            .as_deref()
                            .or(target.unit.as_deref())
                            .unwrap_or("—"),
                        target.source
                    )
                })
            };
            ChoiceOption {
                value: id,
                description,
            }
        })
        .collect()
}

fn compatible(header: &HeaderInput, target: &Target) -> bool {
    let samples: Vec<&str> = header
        .samples
        .iter()
        .map(String::as_str)
        .filter(|cell| !cell.is_empty())
        .collect();
    if samples.is_empty() {
        return false;
    }
    let numbers = samples
        .iter()
        .filter(|cell| parse_loose_number(cell).is_some())
        .count();
    let dates = samples.iter().filter(|cell| looks_like_date(cell)).count();
    let distinct = samples.iter().collect::<BTreeSet<_>>().len();
    match target.kind.as_str() {
        "number" | "range" => numbers * 2 >= samples.len(),
        "date" => dates * 2 >= samples.len(),
        "value_list" | "boolean" => distinct <= 20,
        _ => false,
    }
}

fn parse_loose_number(text: &str) -> Option<()> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    let numeric: String = trimmed
        .chars()
        .filter(|character| {
            character.is_ascii_digit()
                || *character == '.'
                || *character == ','
                || *character == '-'
        })
        .collect();
    if numeric.chars().any(|character| character.is_ascii_digit()) {
        Some(())
    } else {
        None
    }
}

fn looks_like_date(text: &str) -> bool {
    let parts: Vec<&str> = text.split(['-', '/']).collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|part| part.chars().all(|character| character.is_ascii_digit()))
}

fn listed(questions: &[super::decisions::ChoiceQuestion], name: &str) -> Option<HashSet<String>> {
    questions
        .iter()
        .find(|question| question.name == name)
        .map(|question| {
            question
                .choices
                .iter()
                .map(|choice| choice.value.clone())
                .collect()
        })
}

fn apply_call1(
    questions: &[super::decisions::ChoiceQuestion],
    response: &DecisionResponse,
    type_choice: &mut ChoiceProposal,
    id_choice: &mut ChoiceProposal,
) {
    for answer in &response.answers {
        match answer.name() {
            Some("type") if type_choice.source != "manual" => {
                let Some(allowed) = listed(questions, "type") else {
                    continue;
                };
                if let Some(choice) = choice_of(answer).filter(|choice| allowed.contains(choice)) {
                    *type_choice = ChoiceProposal {
                        value: Some(choice),
                        confidence: confidence_of(answer),
                        source: "decisions".into(),
                        alternatives: alternatives_of(answer, 0.05),
                    };
                }
            }
            Some("id_column") if id_choice.source != "manual" => {
                let Some(allowed) = listed(questions, "id_column") else {
                    continue;
                };
                if let Some(choice) = choice_of(answer).filter(|choice| allowed.contains(choice)) {
                    *id_choice = ChoiceProposal {
                        value: Some(choice),
                        confidence: confidence_of(answer),
                        source: "decisions".into(),
                        alternatives: alternatives_of(answer, 0.05),
                    };
                }
            }
            _ => {}
        }
    }
}

fn apply_call2(
    responses: &[(Vec<super::decisions::ChoiceQuestion>, DecisionResponse)],
    drafts: &mut [ColumnDraft],
) {
    for (questions, response) in responses {
        for answer in &response.answers {
            let Some(name) = answer.name() else { continue };
            let Some(index) = name
                .strip_prefix("col:")
                .and_then(|text| text.parse::<usize>().ok())
            else {
                continue;
            };
            let Some(draft) = drafts
                .iter_mut()
                .find(|draft| draft.col == index && !draft.pinned)
            else {
                continue;
            };
            if matches!(answer, Answer::Refusal { .. }) {
                draft.reason = Some("refusal".into());
                draft.target = "unmapped".into();
                draft.source = "decisions".into();
                draft.confidence = 0.0;
                continue;
            }
            let Some(choice) = choice_of(answer) else {
                draft.reason = Some("refusal".into());
                continue;
            };
            let allowed = listed(questions, name).unwrap_or_default();
            if !allowed.contains(&choice) {
                draft.reason = Some("undecided".into());
                draft.target = "unmapped".into();
                draft.source = "decisions".into();
                draft.confidence = 0.0;
                continue;
            }
            draft.target = choice;
            draft.confidence = confidence_of(answer);
            draft.source = "decisions".into();
            draft.reason = None;
            draft.candidates = answer_candidates(answer);
            draft.alternatives = alternatives_of(answer, 0.05);
        }
    }
}

fn choice_of(answer: &Answer) -> Option<String> {
    match answer {
        Answer::Choice {
            choice: ChoiceValue::Text(text),
            ..
        } => Some(text.clone()),
        _ => None,
    }
}

fn confidence_of(answer: &Answer) -> f64 {
    match answer {
        Answer::Choice { confidence, .. } => *confidence,
        _ => 0.0,
    }
}

fn alternatives_of(answer: &Answer, floor: f64) -> Vec<Alt> {
    let chosen = choice_of(answer);
    probabilities(answer)
        .into_iter()
        .filter(|item| item.probability >= floor && chosen.as_deref() != item.value.as_text())
        .take(3)
        .filter_map(|item| {
            item.value.as_text().map(|text| Alt {
                value: text.to_owned(),
                p: item.probability,
            })
        })
        .collect()
}

fn answer_candidates(answer: &Answer) -> Vec<Alt> {
    let mut items = Vec::new();
    if let Some(choice) = choice_of(answer) {
        items.push(Alt {
            value: choice,
            p: confidence_of(answer),
        });
    }
    for alt in alternatives_of(answer, 0.0) {
        if !items.iter().any(|item| item.value == alt.value) {
            items.push(alt);
        }
    }
    items.sort_by(|left, right| {
        right
            .p
            .partial_cmp(&left.p)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    items
}

fn probabilities(answer: &Answer) -> Vec<Probability> {
    match answer {
        Answer::Choice { probabilities, .. } => probabilities.clone(),
        _ => Vec::new(),
    }
}

fn dedupe_drafts(mut drafts: Vec<ColumnDraft>) -> Vec<ColumnProposal> {
    let mut used = HashSet::new();
    let mut pinned = Vec::new();
    let mut rest = Vec::new();
    for draft in drafts.drain(..) {
        if draft.pinned && draft.target != "unmapped" {
            used.insert(draft.target.clone());
            pinned.push(draft);
        } else if draft.pinned {
            pinned.push(draft);
        } else {
            rest.push(draft);
        }
    }
    rest.sort_by(|left, right| {
        right
            .confidence
            .partial_cmp(&left.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut index = 0;
    while index < rest.len() {
        let target = rest[index].target.clone();
        let confidence = rest[index].confidence;
        let free = target == "unmapped" || used.insert(target.clone());
        if free {
            if target == "unmapped" && rest[index].reason.is_none() && confidence < LOW {
                rest[index].reason = Some("low_confidence".into());
            }
            index += 1;
            continue;
        }
        let next = rest[index]
            .candidates
            .iter()
            .find(|candidate| {
                candidate.value != target
                    && candidate.p >= 0.15
                    && (candidate.value == "unmapped" || !used.contains(&candidate.value))
            })
            .cloned();
        if let Some(candidate) = next {
            rest[index].target = candidate.value.clone();
            rest[index].confidence = candidate.p;
            rest[index].candidates.retain(|item| item.value != target);
            let draft = rest.remove(index);
            let insert_at = rest
                .iter()
                .position(|item| item.confidence < draft.confidence)
                .unwrap_or(rest.len());
            rest.insert(insert_at, draft);
        } else {
            rest[index].target = "unmapped".into();
            rest[index].reason = Some("conflict".into());
            index += 1;
        }
    }
    pinned.extend(rest);
    pinned.sort_by_key(|draft| draft.col);
    pinned
        .into_iter()
        .map(|draft| {
            let runner = draft
                .alternatives
                .iter()
                .map(|alt| alt.p)
                .fold(0.0_f64, f64::max);
            let confidence = if draft.source == "manual" {
                draft.confidence
            } else {
                cap_confidence(draft.confidence, runner)
            };
            ColumnProposal {
                low: confidence < LOW,
                col: draft.col,
                target: draft.target,
                confidence,
                source: draft.source,
                reason: draft.reason,
                alternatives: draft.alternatives,
            }
        })
        .collect()
}

/// A close rival means the winner is not certain. 1.0 beside 0.86 becomes 0.57.
fn cap_confidence(best: f64, second: f64) -> f64 {
    if second >= 0.8 && best - second < 0.25 {
        return (best - second * 0.5).clamp(0.0, 0.99);
    }
    best
}

pub fn cache_key(revision: u64) -> String {
    let digest = Sha256::digest(format!("{revision}{TENANT}").as_bytes());
    let hexed = hex::encode(digest);
    format!("braian-import-{}", &hexed[..16])
}

pub fn safety_id(owner: Uuid) -> String {
    let digest = Sha256::digest(owner.as_bytes());
    let hexed = hex::encode(digest);
    hexed[..32].to_owned()
}

pub fn model_summary(defs: &DefinitionSet) -> Value {
    let mut types = Vec::new();
    let mut type_list: Vec<&braian_model::ObjectType> = defs.types.iter().collect();
    type_list.sort_by(|left, right| left.key.cmp(&right.key));
    for object_type in type_list {
        let targets = targets_for(defs, &object_type.key);
        let attributes: Vec<Value> = targets
            .iter()
            .filter(|target| target.attribute_key.is_some() && target.structure_key.is_none())
            .map(target_json)
            .collect();
        types.push(serde_json::json!({
            "key": object_type.key,
            "label": object_type.singular_label("en"),
            "key_attribute": defs.attributes.iter().find(|item| item.id == object_type.key_attribute).map(|item| item.key.clone()).unwrap_or_default(),
            "attributes": attributes,
        }));
    }
    let mut structures = Vec::new();
    let mut structure_list: Vec<&Structure> = defs.structures.iter().collect();
    structure_list.sort_by(|left, right| left.key.cmp(&right.key));
    for structure in structure_list {
        let mut groups = Vec::new();
        for group in groups_in_order(structure) {
            let attributes: Vec<Value> = targets_for_group(defs, structure, group)
                .into_iter()
                .map(|target| target_json(&target))
                .collect();
            groups.push(serde_json::json!({
                "key": group.key,
                "label": group.key,
                "attributes": attributes,
            }));
        }
        structures.push(serde_json::json!({
            "key": structure.key,
            "groups": groups,
        }));
    }
    let mut summary = serde_json::json!({
        "model_revision": defs.model_revision,
        "languages": defs.languages.iter().map(|language| language.as_str()).collect::<Vec<_>>(),
        "types": types,
        "structures": structures,
    });
    if !within_json_limits(&summary) {
        strip_labels(&mut summary);
    }
    if !within_json_limits(&summary)
        && let Some(object) = summary.as_object_mut()
    {
        object.remove("structures");
    }
    summary
}

fn targets_for_group(defs: &DefinitionSet, structure: &Structure, group: &Group) -> Vec<Target> {
    let mut ids = group.attributes.clone();
    for class in &structure.classes {
        if class.group == group.id {
            ids.extend(class.attributes.iter().copied());
        }
    }
    let mut targets = Vec::new();
    for id in ids {
        let Some(attribute) = defs.attributes.iter().find(|item| item.id == id) else {
            continue;
        };
        targets.extend(attribute_targets(
            defs,
            attribute,
            &Provenance::Group {
                structure: structure.key.clone(),
                group: group.key.clone(),
            },
            false,
        ));
    }
    targets
}

fn target_json(target: &Target) -> Value {
    let mut map = Map::new();
    map.insert("id".into(), Value::String(target.id.clone()));
    map.insert("kind".into(), Value::String(target.kind.clone()));
    map.insert("label".into(), Value::String(target.label.clone()));
    if let Some(unit) = &target.unit {
        map.insert("unit".into(), Value::String(unit.clone()));
    }
    Value::Object(map)
}

fn strip_labels(value: &mut Value) {
    match value {
        Value::Object(map) => {
            if map.contains_key("label") && map.contains_key("id") {
                map.remove("label");
            }
            for child in map.values_mut() {
                strip_labels(child);
            }
        }
        Value::Array(items) => {
            for item in items {
                strip_labels(item);
            }
        }
        _ => {}
    }
}

pub fn file_summary(file_name: &str, sheet: &str, headers: &[HeaderInput]) -> Value {
    let mut columns = Vec::new();
    for header in headers {
        if header.empty_ratio >= 1.0 {
            continue;
        }
        let samples: Vec<String> = header
            .samples
            .iter()
            .take(20)
            .map(|cell| cell.chars().take(200).collect())
            .collect();
        columns.push(serde_json::json!({
            "id": format!("col:{}", header.index),
            "header": header.raw,
            "samples": samples,
        }));
    }
    let mut value = serde_json::json!({
        "file_name": file_name,
        "sheet": sheet,
        "columns": columns,
    });
    if !within_json_limits(&value) {
        shrink_samples(&mut value, 10, 200);
    }
    if !within_json_limits(&value) {
        shrink_samples(&mut value, 5, 80);
    }
    value
}

fn shrink_samples(value: &mut Value, rows: usize, clip: usize) {
    let Some(columns) = value.get_mut("columns").and_then(Value::as_array_mut) else {
        return;
    };
    for column in columns {
        let Some(samples) = column.get_mut("samples").and_then(Value::as_array_mut) else {
            continue;
        };
        samples.truncate(rows);
        for sample in samples {
            if let Some(text) = sample.as_str() {
                *sample = Value::String(text.chars().take(clip).collect());
            }
        }
    }
}

pub fn target_wire(targets: &[Target]) -> Value {
    Value::Array(
        targets
            .iter()
            .map(|target| {
                serde_json::json!({
                    "id": target.id,
                    "label": target.label,
                    "kind": target.kind,
                    "key": target.key_attribute,
                    "source": target.source,
                    "language": target.language,
                    "unit": target.unit,
                    "attribute_id": target.attribute_id,
                    "structure_id": target.structure_id,
                    "group_id": target.group_id,
                })
            })
            .collect(),
    )
}

pub fn find_target<'a>(targets: &'a [Target], id: &str) -> Option<&'a Target> {
    targets.iter().find(|target| target_matches(target, id))
}

fn target_matches(target: &Target, id: &str) -> bool {
    if target.id == id {
        return true;
    }
    let stored = stored_target_id(target);
    if stored.as_deref() == Some(id) {
        return true;
    }
    // A mapping saved before paths used `type:<attribute id>`.
    let Some(rest) = id.strip_prefix("type:") else {
        return false;
    };
    if stored.as_deref() == Some(rest) {
        return true;
    }
    path_part(id).is_none()
        && path_part(&target.id).is_none()
        && target.id.starts_with("type:")
        && target.attribute_id.as_deref() == Some(rest)
}

/// Id form of a target. Decisions and the picker keep the key form in `id`.
/// A type-level attribute is stored as an attribute path (`<id>` or `<id>.unit`).
fn stored_target_id(target: &Target) -> Option<String> {
    let (body, language) = target
        .id
        .rsplit_once('@')
        .filter(|(_, language)| !language.is_empty() && !language.contains(':'))
        .unwrap_or((target.id.as_str(), ""));
    let part = path_part(body).unwrap_or("");
    let next = if body.starts_with("membership:") {
        format!("membership:{}", target.structure_id.as_deref()?)
    } else if body.starts_with("link:") {
        let side = body.rsplit_once(':')?.1;
        format!("link:{}:{side}", target.attribute_id.as_deref()?)
    } else if body.starts_with("group:") {
        let base = format!(
            "group:{}/{}:{}",
            target.structure_id.as_deref()?,
            target.group_id.as_deref()?,
            target.attribute_id.as_deref()?
        );
        if part.is_empty() {
            base
        } else {
            format!("{base}.{part}")
        }
    } else if body.starts_with("type:") || path_part(body).is_some() {
        let id = target.attribute_id.as_deref()?;
        if part.is_empty() {
            id.to_owned()
        } else {
            format!("{id}.{part}")
        }
    } else {
        return None;
    };
    if language.is_empty() {
        Some(next)
    } else {
        Some(format!("{next}@{language}"))
    }
}

fn structure_id_of(defs: &DefinitionSet, source: &Provenance) -> Option<String> {
    let Provenance::Group { structure, .. } = source else {
        return None;
    };
    defs.structures
        .iter()
        .find(|item| &item.key == structure)
        .map(|item| item.id.as_uuid().to_string())
}

fn group_id_of(defs: &DefinitionSet, source: &Provenance) -> Option<String> {
    let Provenance::Group { structure, group } = source else {
        return None;
    };
    let structure = defs.structures.iter().find(|item| &item.key == structure)?;
    structure
        .groups
        .iter()
        .find(|item| &item.key == group)
        .map(|item| item.id.as_uuid().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn defs() -> DefinitionSet {
        serde_json::from_str(include_str!("../../../../testdata/rules/definitions.json")).unwrap()
    }

    fn header(index: usize, raw: &str, samples: &[&str]) -> HeaderInput {
        HeaderInput {
            index,
            raw: raw.into(),
            name: normalize_header(raw),
            empty_ratio: 0.0,
            samples: samples.iter().map(|cell| (*cell).to_owned()).collect(),
        }
    }

    #[test]
    fn type_names_come_from_the_singular_label() {
        let mut defs = defs();
        let object_type = defs
            .types
            .iter_mut()
            .find(|item| item.key == "item")
            .unwrap();
        object_type
            .labels
            .insert(LanguageTag::parse("de").unwrap(), "Variante".into());
        object_type
            .singular
            .insert(LanguageTag::parse("de").unwrap(), "Variante".into());
        object_type
            .singular
            .insert(LanguageTag::parse("en").unwrap(), "Variant".into());
        assert_eq!(object_type.singular_label("en"), "Variant");
        let summary = model_summary(&defs);
        let label = summary["types"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["key"] == "item")
            .unwrap()["label"]
            .as_str();
        assert_eq!(label, Some("Variant"));
        assert!(
            targets_for(&defs, "item")
                .iter()
                .any(|target| target.id.starts_with("type:") && target.source == "Variant")
        );
    }

    #[test]
    fn builds_type_language_group_and_membership_targets() {
        let defs = defs();
        let targets = targets_for(&defs, "item");
        assert!(
            targets
                .iter()
                .any(|target| target.id == "type:sku" && target.key_attribute)
        );
        assert!(targets.iter().any(|target| {
            target.id == "type:name" && target.language.as_deref() == Some("sv")
        }));
        assert!(!targets.iter().any(|target| target.id == "type:name@sv"));
        assert!(targets.iter().any(|target| target.id == "type:name@en-GB"));
        let name_labels: Vec<&str> = targets
            .iter()
            .filter(|target| target.attribute_key.as_deref() == Some("name"))
            .map(|target| target.label.as_str())
            .collect();
        let mut unique = name_labels.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), name_labels.len(), "{name_labels:?}");
        assert!(
            targets
                .iter()
                .any(|target| target.id == "group:catalog/root:color")
        );
        assert!(
            targets
                .iter()
                .any(|target| target.id == "group:catalog/child:finish")
        );
        assert!(
            targets
                .iter()
                .any(|target| target.id == "membership:catalog")
        );
        assert!(!targets.iter().any(|target| target.id == "action"));
        assert_eq!(
            targets
                .iter()
                .filter(|target| target.attribute_key.as_deref() == Some("sku"))
                .count(),
            1
        );
    }

    #[test]
    fn matches_exact_suffix_synonym_and_jaro() {
        let defs = defs();
        let targets = targets_for(&defs, "item");
        let units = unit_codes(&defs);
        let sku = rank_targets("article_no", &targets, &units);
        assert_eq!(sku[0].target, "type:sku");
        assert!((sku[0].score - 0.95).abs() < 1e-9);
        assert!(decided(&sku).is_some());
        let name = rank_targets("name_sv", &targets, &units);
        assert_eq!(name[0].target, "type:name");
        let bare = rank_targets("Name", &targets, &units);
        assert_eq!(bare[0].target, "type:name");
        let rival = bare.get(1).map(|item| item.score).unwrap_or(0.0);
        assert!(
            rival < 0.5,
            "a bare header must not grow a language rival, got {rival}"
        );
        let weight = rank_targets("weight_kg", &targets, &units);
        assert_eq!(weight[0].target, "type:weight");
        assert!((weight[0].score - 0.97).abs() < 1e-9);
        let action = rank_targets("operation", &targets, &units);
        assert!(action.iter().all(|item| item.target != "action"));
    }

    #[test]
    fn bare_header_is_not_a_near_match_for_every_language() {
        let bare = sample_target(
            "type:description",
            "Description (sv)",
            Some("sv"),
            "description",
        );
        let swedish = sample_target(
            "type:description@sv",
            "Description (sv)",
            Some("sv"),
            "description",
        );
        let english = sample_target(
            "type:description@en",
            "Description (en)",
            Some("en"),
            "description",
        );
        assert!((score_column("Description", &bare, &[]) - 1.0).abs() < 1e-9);
        assert_eq!(score_column("Description", &swedish, &[]), 0.0);
        assert_eq!(score_column("Description", &english, &[]), 0.0);
        assert!((super::cap_confidence(1.0, 0.86) - 0.57).abs() < 1e-9);
        assert!((super::cap_confidence(1.0, 0.2) - 1.0).abs() < 1e-9);
    }

    fn sample_target(id: &str, label: &str, language: Option<&str>, key: &str) -> Target {
        Target {
            id: id.into(),
            label: label.into(),
            kind: "text".into(),
            key_attribute: false,
            source: "Item".into(),
            attribute_key: Some(key.into()),
            language: language.map(str::to_owned),
            structure_key: None,
            group_key: None,
            unit: None,
            attribute_id: None,
            structure_id: None,
            group_id: None,
        }
    }

    #[test]
    fn caps_choices_at_the_documented_maximum() {
        let defs = defs();
        let targets = targets_for(&defs, "item");
        let units = unit_codes(&defs);
        let header = header(0, "Mystery", &["x"]);
        let choices = cap_choices(&header, &targets, &units);
        assert!(choices.len() <= 255);
        assert!(choices.len() >= 2);
        assert!(choices.iter().any(|choice| choice.value == "unmapped"));
        assert!(choices.iter().all(|choice| choice.value != "action"));
        let unique: BTreeSet<_> = choices.iter().map(|choice| choice.value.as_str()).collect();
        assert_eq!(unique.len(), choices.len());
    }

    #[test]
    fn greedy_dedupe_keeps_unmapped_reusable_and_conflicts_below_threshold() {
        let drafts = vec![
            ColumnDraft {
                col: 0,
                target: "type:sku".into(),
                confidence: 1.0,
                source: "deterministic".into(),
                reason: None,
                alternatives: Vec::new(),
                pinned: true,
                candidates: Vec::new(),
            },
            ColumnDraft {
                col: 1,
                target: "type:sku".into(),
                confidence: 0.8,
                source: "decisions".into(),
                reason: None,
                alternatives: vec![Alt {
                    value: "type:name".into(),
                    p: 0.2,
                }],
                pinned: false,
                candidates: vec![
                    Alt {
                        value: "type:sku".into(),
                        p: 0.8,
                    },
                    Alt {
                        value: "type:name".into(),
                        p: 0.2,
                    },
                ],
            },
            ColumnDraft {
                col: 2,
                target: "type:sku".into(),
                confidence: 0.7,
                source: "decisions".into(),
                reason: None,
                alternatives: vec![Alt {
                    value: "type:weight".into(),
                    p: 0.1,
                }],
                pinned: false,
                candidates: vec![
                    Alt {
                        value: "type:sku".into(),
                        p: 0.7,
                    },
                    Alt {
                        value: "type:weight".into(),
                        p: 0.1,
                    },
                ],
            },
        ];
        let columns = dedupe_drafts(drafts);
        assert_eq!(columns[0].target, "type:sku");
        assert_eq!(columns[1].target, "type:name");
        assert_eq!(columns[2].target, "unmapped");
        assert_eq!(columns[2].reason.as_deref(), Some("conflict"));
    }

    #[test]
    fn model_and_file_json_stay_inside_the_documented_limits() {
        let defs = defs();
        let summary = model_summary(&defs);
        assert!(within_json_limits(&summary));
        let again = model_summary(&defs);
        assert_eq!(summary, again);
        let headers = vec![header(0, "Artnr", &["A-1", "A-2"])];
        let file = file_summary("prices.csv", "CSV", &headers);
        assert!(within_json_limits(&file));
        assert_eq!(file["columns"][0]["samples"].as_array().unwrap().len(), 2);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn deterministic_file_skips_the_decider() {
        let defs = defs();
        let headers = vec![
            header(0, "sku", &["A-1", "A-2"]),
            header(1, "name", &["Stol", "Bord"]),
        ];
        let (proposal, _) = propose::<super::super::decisions::HttpDecider>(
            None,
            &defs,
            &headers,
            "items.csv",
            "CSV",
            Uuid::nil(),
            None,
            None,
            &[],
        )
        .await;
        assert_eq!(proposal.type_choice.value.as_deref(), Some("item"));
        assert_eq!(proposal.columns[0].target, "type:sku");
        assert_eq!(proposal.columns[1].target, "type:name");
        assert_eq!(proposal.ai, "skipped");
        assert_eq!(proposal.id_column.value.as_deref(), Some("col:0"));
    }

    struct Script(std::sync::Mutex<Vec<DecisionResponse>>);

    impl Decider for Script {
        fn decide(
            &self,
            _request: &DecisionRequest,
        ) -> impl std::future::Future<
            Output = Result<DecisionResponse, super::super::decisions::DecisionError>,
        > + Send {
            let next = self.0.lock().expect("script").remove(0);
            async move { Ok(next) }
        }
    }

    fn answered(name: &str, choice: &str) -> DecisionResponse {
        DecisionResponse {
            model: String::new(),
            answers: vec![Answer::Choice {
                name: Some(name.into()),
                choice: ChoiceValue::Text(choice.into()),
                confidence: 0.4,
                probabilities: Vec::new(),
            }],
            usage: None,
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn unknown_choice_and_refusal_stay_undecided() {
        let defs = defs();
        let headers = vec![header(0, "Mystery", &["aa", "bb"])];
        let script = Script(std::sync::Mutex::new(vec![
            answered("id_column", "not-a-column"),
            DecisionResponse {
                model: String::new(),
                answers: vec![Answer::Refusal {
                    name: Some("col:0".into()),
                }],
                usage: None,
            },
        ]));
        let (proposal, _) = propose(
            Some(&script),
            &defs,
            &headers,
            "mystery.csv",
            "CSV",
            Uuid::nil(),
            None,
            None,
            &[],
        )
        .await;
        assert!(proposal.id_column.value.is_none());
        assert_eq!(proposal.columns[0].target, "unmapped");
        assert_eq!(proposal.columns[0].reason.as_deref(), Some("refusal"));

        let script = Script(std::sync::Mutex::new(vec![
            answered("id_column", "none"),
            answered("col:0", "not-a-field"),
        ]));
        let (proposal, _) = propose(
            Some(&script),
            &defs,
            &headers,
            "mystery.csv",
            "CSV",
            Uuid::nil(),
            None,
            None,
            &[],
        )
        .await;
        assert_eq!(proposal.id_column.value.as_deref(), Some("none"));
        assert_eq!(proposal.columns[0].target, "unmapped");
        assert_eq!(proposal.columns[0].reason.as_deref(), Some("undecided"));
    }
}
