//! One way to name an attribute, a part of it, or the same thing one link away.
//!
//! Text form uses keys: `net_weight.unit`, `product>brand`, `articles>link.sort_order`.
//! The saved form uses ids: `{ "attribute", "part", "via": { "link_type", "side" } }`.
//! A language qualifier (`name@en`) is accepted. The model only has the languages it lists.

use serde_json::{Map, Value};
use uuid::Uuid;

use crate::definition::{Attribute, AttributeKind, DefinitionSet, LinkSpec, ObjectType};
use crate::id::{AttributeId, TypeId};
use crate::refs::attribute_index_field;
use crate::standard::{self, standard_ids};

/// Which end of the link the path walks, or the link object itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathSide {
    /// The caller is the source. The path uses the link's `to` label.
    To,
    /// The caller is the target. The path uses the link's `from` label.
    From,
    /// An attribute stored on the link object. The label still names the link.
    Link,
}

impl PathSide {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::To => "to",
            Self::From => "from",
            Self::Link => "link",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "to" => Some(Self::To),
            "from" => Some(Self::From),
            "link" => Some(Self::Link),
            _ => None,
        }
    }
}

/// A piece of a value. `Whole` is the attribute as it was entered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PathPart {
    #[default]
    Whole,
    Value,
    Unit,
    Min,
    Max,
    Code,
    Label,
}

impl PathPart {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Whole => "",
            Self::Value => "value",
            Self::Unit => "unit",
            Self::Min => "min",
            Self::Max => "max",
            Self::Code => "code",
            Self::Label => "label",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "" | "whole" => Some(Self::Whole),
            "value" => Some(Self::Value),
            "unit" => Some(Self::Unit),
            "min" => Some(Self::Min),
            "max" => Some(Self::Max),
            "code" => Some(Self::Code),
            "label" => Some(Self::Label),
            _ => None,
        }
    }

    pub fn word(self) -> &'static str {
        match self {
            Self::Whole => "",
            Self::Value => "value",
            Self::Unit => "unit",
            Self::Min => "min",
            Self::Max => "max",
            Self::Code => "code",
            Self::Label => "label",
        }
    }
}

/// The link step in front of an attribute. One step is the limit.
///
/// `side` is where the value lives (`to`, `from`, or `link`). `label` is which
/// end's label the text form uses, so `articles>link.sort_order` and
/// `product>link.sort_order` stay distinct. The id object stores both.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathVia {
    pub link_type: TypeId,
    pub side: PathSide,
    pub label: PathSide,
}

/// A validated address. `language` is set only for a language-dependent attribute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttributePath {
    pub via: Option<PathVia>,
    pub attribute: AttributeId,
    pub part: PathPart,
    pub language: Option<String>,
}

/// One row of `GET /v1/types/{id}/attribute-paths`.
#[derive(Debug, Clone, PartialEq)]
pub struct ListedPath {
    pub path: AttributePath,
    pub text: String,
    pub id: Value,
    pub label: String,
}

const RESERVED_FIELDS: &[&str] = &[
    "key", "type_key", "groups", "seq", "version", "deleted", "id",
];

/// Parts a kind can name, after the whole value.
pub fn extra_parts(kind: &AttributeKind) -> Vec<PathPart> {
    match kind {
        AttributeKind::Number { unit_family, .. } => {
            let mut parts = vec![PathPart::Value];
            if unit_family.is_some() {
                parts.push(PathPart::Unit);
            }
            parts
        }
        AttributeKind::Range { unit_family } => {
            let mut parts = vec![PathPart::Min, PathPart::Max];
            if unit_family.is_some() {
                parts.push(PathPart::Unit);
            }
            parts
        }
        AttributeKind::ValueList { .. } => vec![PathPart::Code, PathPart::Label],
        _ => Vec::new(),
    }
}

pub fn part_fits(kind: &AttributeKind, part: PathPart) -> bool {
    part == PathPart::Whole || extra_parts(kind).contains(&part)
}

/// Parse text or the id object. `from_type` disambiguates a shared link label.
pub fn parse_path(
    input: &Value,
    defs: &DefinitionSet,
    from_type: Option<TypeId>,
) -> Result<AttributePath, String> {
    match input {
        Value::String(text) => parse_path_text(text, defs, from_type),
        Value::Object(_) => parse_path_id(input, defs, from_type),
        _ => Err("an attribute path is text or an id object".into()),
    }
}

pub fn parse_path_text(
    text: &str,
    defs: &DefinitionSet,
    from_type: Option<TypeId>,
) -> Result<AttributePath, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("attribute path is empty".into());
    }
    if text.matches('>').count() > 1 {
        return Err("a path has one link step".into());
    }
    let (body, language) = split_language(text)?;
    let (via_token, attribute_body, link_attribute) =
        if let Some((via, rest)) = body.split_once('>') {
            let via = via.trim();
            let rest = rest.trim();
            if via.is_empty() || rest.is_empty() {
                return Err("a link path names the link and the attribute".into());
            }
            if let Some(attribute) = rest.strip_prefix("link.") {
                (Some(via), attribute.trim(), true)
            } else if rest == "link" {
                return Err("a link path names the link attribute".into());
            } else {
                (Some(via), rest, false)
            }
        } else {
            (None, body.as_str(), false)
        };
    if attribute_body.matches('.').count() > 1 {
        return Err("a path has one part".into());
    }
    let (attribute_token, part) = if let Some((name, part_text)) = attribute_body.split_once('.') {
        let part = PathPart::parse(part_text).ok_or_else(|| format!("unknown part {part_text}"))?;
        if part == PathPart::Whole {
            return Err(format!("unknown part {part_text}"));
        }
        (name.trim(), part)
    } else {
        (attribute_body, PathPart::Whole)
    };
    if attribute_token.is_empty() {
        return Err("attribute path is missing the attribute".into());
    }
    let via = match via_token {
        Some(token) => Some(resolve_via(defs, token, link_attribute, from_type)?),
        None => {
            if link_attribute {
                return Err("a link attribute names the link first".into());
            }
            None
        }
    };
    let attribute = resolve_attribute_token(defs, attribute_token)?;
    Ok(AttributePath {
        via,
        attribute,
        part,
        language,
    })
}

pub fn parse_path_id(
    value: &Value,
    defs: &DefinitionSet,
    from_type: Option<TypeId>,
) -> Result<AttributePath, String> {
    let object = value
        .as_object()
        .ok_or_else(|| "an attribute path object is a JSON object".to_owned())?;
    let attribute = object
        .get("attribute")
        .and_then(Value::as_str)
        .ok_or_else(|| "attribute path is missing the attribute".to_owned())?;
    let attribute = resolve_attribute_token(defs, attribute)?;
    let part = match object.get("part").and_then(Value::as_str) {
        None => PathPart::Whole,
        Some(text) => PathPart::parse(text).ok_or_else(|| format!("unknown part {text}"))?,
    };
    let language = match object.get("language").and_then(Value::as_str) {
        None => None,
        Some("") => None,
        Some(text) => Some(text.to_owned()),
    };
    let via = match object.get("via") {
        None | Some(Value::Null) => None,
        Some(via) => {
            let via = via
                .as_object()
                .ok_or_else(|| "via is an object".to_owned())?;
            let link_type = via
                .get("link_type")
                .and_then(Value::as_str)
                .ok_or_else(|| "via is missing link_type".to_owned())?;
            let side = via
                .get("side")
                .and_then(Value::as_str)
                .and_then(PathSide::parse)
                .ok_or_else(|| "via.side is to, from or link".to_owned())?;
            let label = via
                .get("label")
                .and_then(Value::as_str)
                .and_then(PathSide::parse)
                .unwrap_or(match side {
                    PathSide::From => PathSide::From,
                    PathSide::To | PathSide::Link => PathSide::To,
                });
            if matches!(label, PathSide::Link) {
                return Err("via.label is to or from".into());
            }
            let link_type = resolve_link_type(defs, link_type)?;
            let _ = from_type;
            Some(PathVia {
                link_type,
                side,
                label,
            })
        }
    };
    Ok(AttributePath {
        via,
        attribute,
        part,
        language,
    })
}

/// Key form. A renamed key changes this text; the id form does not.
pub fn format_path_text(path: &AttributePath, defs: &DefinitionSet) -> String {
    format_path(path, defs, false)
}

/// Id form as text, for a filter or a saved import target.
pub fn format_path_id_text(path: &AttributePath, defs: &DefinitionSet) -> String {
    format_path(path, defs, true)
}

pub fn format_path_id(path: &AttributePath) -> Value {
    let mut object = Map::new();
    object.insert(
        "attribute".into(),
        Value::String(path.attribute.as_uuid().to_string()),
    );
    if path.part != PathPart::Whole {
        object.insert("part".into(), Value::String(path.part.as_str().into()));
    }
    if let Some(language) = &path.language {
        object.insert("language".into(), Value::String(language.clone()));
    }
    if let Some(via) = &path.via {
        object.insert(
            "via".into(),
            serde_json::json!({
                "link_type": via.link_type.as_uuid().to_string(),
                "side": via.side.as_str(),
                "label": via.label.as_str(),
            }),
        );
    }
    Value::Object(object)
}

pub fn path_label(path: &AttributePath, defs: &DefinitionSet, language: &str) -> String {
    let attribute = attribute_of(defs, path.attribute);
    let name = attribute
        .map(|item| label_of(item, language))
        .unwrap_or_else(|| path.attribute.as_uuid().to_string());
    let own = match path.part {
        PathPart::Whole => name,
        part => format!("{name} ({})", part.word()),
    };
    let Some(via) = &path.via else {
        return with_language(own, path.language.as_deref());
    };
    let link = link_type(defs, via.link_type);
    let side = link
        .and_then(|item| item.link.as_ref())
        .map(|spec| match via.label {
            PathSide::From => spec.from_label.text(language),
            PathSide::To | PathSide::Link => spec.to_label.text(language),
        })
        .unwrap_or_else(|| {
            link.map(|item| item.key.clone())
                .unwrap_or_else(|| via.link_type.as_uuid().to_string())
        });
    let text = if via.side == PathSide::Link {
        format!("{side} › link › {own}")
    } else {
        format!("{side} › {own}")
    };
    with_language(text, path.language.as_deref())
}

/// The attribute exists, the part matches the kind, and the link step leaves this type.
pub fn validate_path(
    path: &AttributePath,
    defs: &DefinitionSet,
    from_type: TypeId,
) -> Result<(), String> {
    let attribute = attribute_of(defs, path.attribute)
        .ok_or_else(|| format!("unknown attribute {}", path.attribute))?;
    if !part_fits(&attribute.kind, path.part) {
        return Err(format!(
            "attribute {} has no {} part",
            attribute.key,
            path.part.word()
        ));
    }
    if let Some(language) = &path.language {
        if !attribute.language_dependent {
            return Err(format!(
                "attribute {} is not language dependent",
                attribute.key
            ));
        }
        if !defs.languages.iter().any(|tag| tag.as_str() == language) {
            return Err(format!("unknown language {language}"));
        }
    }
    let Some(via) = &path.via else {
        if !attribute_on_type(defs, from_type, path.attribute) {
            return Err(format!("attribute {} is not on this type", attribute.key));
        }
        return Ok(());
    };
    let link = link_type(defs, via.link_type)
        .ok_or_else(|| format!("unknown link type {}", via.link_type))?;
    let spec = link
        .link
        .as_ref()
        .ok_or_else(|| format!("{} is not a link type", link.key))?;
    let on_side = match via.side {
        PathSide::To | PathSide::Link => spec.from_types.contains(&from_type),
        PathSide::From => spec.to_types.contains(&from_type),
    };
    // A link attribute can be read from either end.
    let on_side = on_side
        || (via.side == PathSide::Link
            && (spec.from_types.contains(&from_type) || spec.to_types.contains(&from_type)));
    if !on_side {
        return Err(format!(
            "link type {} is not available from this type",
            link.key
        ));
    }
    match via.side {
        PathSide::Link => {
            if !link.attributes.contains(&path.attribute) {
                return Err(format!(
                    "attribute {} is not on link type {}",
                    attribute.key, link.key
                ));
            }
        }
        PathSide::To => {
            if !spec
                .to_types
                .iter()
                .any(|type_id| attribute_on_type(defs, *type_id, path.attribute))
            {
                return Err(format!(
                    "attribute {} is not on the far side of {}",
                    attribute.key, link.key
                ));
            }
        }
        PathSide::From => {
            if !spec
                .from_types
                .iter()
                .any(|type_id| attribute_on_type(defs, *type_id, path.attribute))
            {
                return Err(format!(
                    "attribute {} is not on the far side of {}",
                    attribute.key, link.key
                ));
            }
        }
    }
    Ok(())
}

/// Every path this type can read. One link step, including the link's own attributes.
pub fn list_attribute_paths(defs: &DefinitionSet, from_type: TypeId) -> Vec<ListedPath> {
    let mut listed = Vec::new();
    for id in attributes_for_type(defs, from_type) {
        push_attribute_paths(defs, None, id, &mut listed);
    }
    let Some(object_type) = defs.types.iter().find(|item| item.id == from_type) else {
        return listed;
    };
    for link in defs.types.iter().filter(|item| item.is_link()) {
        let Some(spec) = link.link.as_ref() else {
            continue;
        };
        if spec.from_types.contains(&object_type.id) {
            push_hop(defs, link, spec, PathSide::To, &mut listed);
            push_link_attributes(defs, link, spec, PathSide::To, &mut listed);
        }
        if spec.to_types.contains(&object_type.id) {
            push_hop(defs, link, spec, PathSide::From, &mut listed);
            push_link_attributes(defs, link, spec, PathSide::From, &mut listed);
        }
    }
    listed
}

/// Typesense field for a path that does not cross a link.
///
/// `number` is the object key. Timestamps and `updated_by` live on the object
/// row, not in the index. A link step is read from Postgres when a table or an
/// export asks for it.
pub fn path_index_field(path: &AttributePath, defs: &DefinitionSet) -> Result<String, String> {
    if path.via.is_some() {
        return Err("filtering across a link comes later".into());
    }
    let ids = standard_ids();
    if path.attribute == ids.number && matches!(path.part, PathPart::Whole | PathPart::Value) {
        return Ok("key".into());
    }
    if path.attribute == ids.created_at
        || path.attribute == ids.updated_at
        || path.attribute == ids.updated_by
    {
        return Err("that standard attribute is stored on the object, not in the index".into());
    }
    let attribute = attribute_of(defs, path.attribute)
        .ok_or_else(|| format!("unknown attribute {}", path.attribute))?;
    if !part_fits(&attribute.kind, path.part) {
        return Err(format!(
            "attribute {} has no {} part",
            attribute.key,
            path.part.word()
        ));
    }
    if path.part == PathPart::Label {
        return Err("a value list label is not a separate index field".into());
    }
    let base = attribute_index_field(path.attribute, path.language.as_deref());
    Ok(match path.part {
        PathPart::Unit => format!("{base}__unit"),
        PathPart::Min => format!("{base}__min"),
        PathPart::Max => format!("{base}__max"),
        PathPart::Whole | PathPart::Value | PathPart::Code | PathPart::Label => base,
    })
}

/// Rewrite a filter so each attribute address becomes a Typesense field.
///
/// `net_weight.unit = g` and `net_weight.unit:=g` both become the unit field.
/// `type_key`, `groups` and an existing `a_<id>` field are left in place.
pub fn translate_filter_expression(filter: &str, defs: &DefinitionSet) -> Result<String, String> {
    if filter.trim().is_empty() {
        return Ok(String::new());
    }
    let mut parts = Vec::new();
    for part in filter.split("&&") {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        parts.push(translate_filter_clause(part, defs)?);
    }
    Ok(parts.join(" && "))
}

/// `net_weight:asc` sorts on the base value. `500 g` is 500 when the base is grams.
pub fn translate_sort_expression(sort: &str, defs: &DefinitionSet) -> Result<String, String> {
    if sort.trim().is_empty() {
        return Ok(String::new());
    }
    let mut parts = Vec::new();
    for part in sort.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        parts.push(translate_sort_clause(part, defs)?);
    }
    Ok(parts.join(","))
}

/// A stored `a_<key>` or `a_<id>` field, or a key, as id-form path text.
pub fn stored_field_as_path(field: &str, defs: &DefinitionSet) -> String {
    let field = field.trim();
    if field.is_empty() || RESERVED_FIELDS.contains(&field) || field.starts_with("search_") {
        return field.to_owned();
    }
    if field.contains('>') {
        return field.to_owned();
    }
    // The whole token wins before a suffix is split off, so a key such as
    // `size__min` or `name__en` is that attribute.
    if let Some(attribute) = attribute_from_head(defs, field) {
        return format_path(
            &AttributePath {
                via: None,
                attribute,
                part: PathPart::Whole,
                language: None,
            },
            defs,
            true,
        );
    }
    let body = field.strip_prefix("a_").unwrap_or(field);
    let (head, suffixes) = split_index_suffixes(body, defs);
    let Some(attribute) = attribute_from_head(defs, head) else {
        return field.to_owned();
    };
    let mut part = PathPart::Whole;
    let mut language = None;
    for suffix in suffixes {
        match suffix {
            "unit" => part = PathPart::Unit,
            "min" => part = PathPart::Min,
            "max" => part = PathPart::Max,
            other => language = Some(other.replace('_', "-")),
        }
    }
    format_path(
        &AttributePath {
            via: None,
            attribute,
            part,
            language,
        },
        defs,
        true,
    )
}

fn translate_filter_clause(clause: &str, defs: &DefinitionSet) -> Result<String, String> {
    let Some((field, operator, value)) = split_clause(clause) else {
        return Ok(clause.to_owned());
    };
    if leave_field(field) {
        return Ok(clause.to_owned());
    }
    if field.starts_with("a_") && !looks_like_path(field) {
        let path_text = stored_field_as_path(field, defs);
        if path_text == field {
            return Ok(clause.to_owned());
        }
        let path = parse_path_text(&path_text, defs, None)?;
        let index = path_index_field(&path, defs)?;
        let operator = if operator == "=" { ":=" } else { operator };
        return Ok(format!("{index}{operator}{value}"));
    }
    let path = parse_path_text(field, defs, None)?;
    let index = path_index_field(&path, defs)?;
    let operator = if operator == "=" { ":=" } else { operator };
    Ok(format!("{index}{operator}{value}"))
}

fn translate_sort_clause(clause: &str, defs: &DefinitionSet) -> Result<String, String> {
    let (field, direction) = clause
        .rsplit_once(':')
        .filter(|(_, direction)| {
            direction.eq_ignore_ascii_case("asc") || direction.eq_ignore_ascii_case("desc")
        })
        .unwrap_or((clause, ""));
    if leave_field(field) {
        return Ok(clause.to_owned());
    }
    let path = if field.starts_with("a_") {
        let text = stored_field_as_path(field, defs);
        if text == field {
            return Ok(clause.to_owned());
        }
        parse_path_text(&text, defs, None)?
    } else {
        parse_path_text(field, defs, None)?
    };
    let index = path_index_field(&path, defs)?;
    if direction.is_empty() {
        Ok(index)
    } else {
        Ok(format!("{index}:{direction}"))
    }
}

fn leave_field(field: &str) -> bool {
    RESERVED_FIELDS.contains(&field) || field.starts_with("search_") || field.is_empty()
}

fn looks_like_path(field: &str) -> bool {
    field.contains('>')
        || field.contains('@')
        || field
            .rsplit('.')
            .next()
            .is_some_and(|part| matches!(part, "value" | "unit" | "min" | "max" | "code" | "label"))
}

fn split_clause(clause: &str) -> Option<(&str, &str, &str)> {
    const OPERATORS: &[&str] = &[":=", ":>=", ":<=", ":>", ":<", "!=", ":", "="];
    let mut found = None;
    for operator in OPERATORS {
        if let Some(index) = clause.find(operator) {
            let better = found.map(|(at, _)| index < at).unwrap_or(true);
            if better {
                found = Some((index, *operator));
            }
        }
    }
    let (index, operator) = found?;
    let field = clause[..index].trim();
    let value = clause[index + operator.len()..].trim();
    if field.is_empty() {
        return None;
    }
    Some((field, operator, value))
}

fn format_path(path: &AttributePath, defs: &DefinitionSet, ids: bool) -> String {
    let attribute = if ids {
        path.attribute.as_uuid().to_string()
    } else {
        attribute_of(defs, path.attribute)
            .map(|item| item.key.clone())
            .unwrap_or_else(|| path.attribute.as_uuid().to_string())
    };
    let mut text = String::new();
    if let Some(via) = &path.via {
        text.push_str(&via_token(defs, via, ids));
        text.push('>');
        if via.side == PathSide::Link {
            text.push_str("link.");
        }
    }
    text.push_str(&attribute);
    if path.part != PathPart::Whole {
        text.push('.');
        text.push_str(path.part.as_str());
    }
    if let Some(language) = &path.language {
        text.push('@');
        text.push_str(language);
    }
    text
}

fn via_token(defs: &DefinitionSet, via: &PathVia, ids: bool) -> String {
    if ids {
        return via.link_type.as_uuid().to_string();
    }
    let Some(link) = link_type(defs, via.link_type) else {
        return via.link_type.as_uuid().to_string();
    };
    let Some(spec) = link.link.as_ref() else {
        return link.key.clone();
    };
    match via.label {
        PathSide::From => spec.from_label.key.clone(),
        PathSide::To | PathSide::Link => spec.to_label.key.clone(),
    }
}

fn push_hop(
    defs: &DefinitionSet,
    link: &ObjectType,
    spec: &LinkSpec,
    side: PathSide,
    listed: &mut Vec<ListedPath>,
) {
    let far = match side {
        PathSide::To => &spec.to_types,
        PathSide::From => &spec.from_types,
        PathSide::Link => return,
    };
    let via = PathVia {
        link_type: link.id,
        side,
        label: side,
    };
    for type_id in far {
        for id in attributes_for_type(defs, *type_id) {
            push_attribute_paths(defs, Some(via.clone()), id, listed);
        }
    }
}

fn push_link_attributes(
    defs: &DefinitionSet,
    link: &ObjectType,
    _spec: &LinkSpec,
    approach: PathSide,
    listed: &mut Vec<ListedPath>,
) {
    let via = PathVia {
        link_type: link.id,
        side: PathSide::Link,
        label: approach,
    };
    for id in &link.attributes {
        push_attribute_paths(defs, Some(via.clone()), *id, listed);
    }
}

fn push_attribute_paths(
    defs: &DefinitionSet,
    via: Option<PathVia>,
    id: AttributeId,
    listed: &mut Vec<ListedPath>,
) {
    let Some(attribute) = attribute_of(defs, id) else {
        return;
    };
    let mut languages = vec![None];
    if attribute.language_dependent {
        for tag in &defs.languages {
            languages.push(Some(tag.as_str().to_owned()));
        }
    }
    let mut parts = vec![PathPart::Whole];
    parts.extend(extra_parts(&attribute.kind));
    for language in languages {
        for part in &parts {
            let path = AttributePath {
                via: via.clone(),
                attribute: id,
                part: *part,
                language: language.clone(),
            };
            let label = path_label(&path, defs, "en");
            if listed
                .iter()
                .any(|item| item.text == format_path_text(&path, defs))
            {
                continue;
            }
            listed.push(ListedPath {
                text: format_path_text(&path, defs),
                id: format_path_id(&path),
                label,
                path,
            });
        }
    }
}

fn attributes_for_type(defs: &DefinitionSet, type_id: TypeId) -> Vec<AttributeId> {
    let Some(object_type) = defs.types.iter().find(|item| item.id == type_id) else {
        return Vec::new();
    };
    let mut ids = Vec::new();
    for id in &object_type.attributes {
        if !ids.contains(id) {
            ids.push(*id);
        }
    }
    let structures = object_type.structures.clone();
    for structure in &defs.structures {
        let allowed = match &structures {
            None => true,
            Some(list) => list.contains(&structure.id),
        };
        if !allowed {
            continue;
        }
        for group in &structure.groups {
            for id in &group.attributes {
                if !ids.contains(id) {
                    ids.push(*id);
                }
            }
            for class in &structure.classes {
                if class.group == group.id {
                    for id in &class.attributes {
                        if !ids.contains(id) {
                            ids.push(*id);
                        }
                    }
                }
            }
        }
    }
    ids
}

fn attribute_on_type(defs: &DefinitionSet, type_id: TypeId, attribute: AttributeId) -> bool {
    attributes_for_type(defs, type_id).contains(&attribute)
}

fn resolve_via(
    defs: &DefinitionSet,
    token: &str,
    link_attribute: bool,
    from_type: Option<TypeId>,
) -> Result<PathVia, String> {
    let mut found: Vec<PathVia> = Vec::new();
    for link in defs.types.iter().filter(|item| item.is_link()) {
        let Some(spec) = link.link.as_ref() else {
            continue;
        };
        let token_matches = |label: &str| {
            label.eq_ignore_ascii_case(token)
                || link.key.eq_ignore_ascii_case(token)
                || link.id.as_uuid().to_string().eq_ignore_ascii_case(token)
        };
        let push = |side: PathSide, found: &mut Vec<PathVia>| {
            let candidate = PathVia {
                link_type: link.id,
                side: if link_attribute { PathSide::Link } else { side },
                label: match side {
                    PathSide::From => PathSide::From,
                    PathSide::To | PathSide::Link => PathSide::To,
                },
            };
            if !found.iter().any(|item| item == &candidate) {
                found.push(candidate);
            }
        };
        if token_matches(&spec.to_label.key)
            && from_type.is_none_or(|type_id| {
                spec.from_types.contains(&type_id)
                    || link_attribute && spec.to_types.contains(&type_id)
            })
        {
            // The `to` label is what the source says. A link attribute can also
            // be named from the other end when that label is the only token.
            if from_type.is_none_or(|type_id| spec.from_types.contains(&type_id)) {
                push(PathSide::To, &mut found);
            }
        }
        if token_matches(&spec.from_label.key)
            && from_type.is_none_or(|type_id| spec.to_types.contains(&type_id))
        {
            push(PathSide::From, &mut found);
        }
        if token_matches(&link.key) || token_matches(&link.id.as_uuid().to_string()) {
            let side = if from_type.is_some_and(|type_id| spec.to_types.contains(&type_id)) {
                PathSide::From
            } else {
                PathSide::To
            };
            push(side, &mut found);
        }
    }
    match found.len() {
        1 => Ok(found.remove(0)),
        0 => Err(format!("unknown link {token}")),
        _ => Err(format!("link {token} is ambiguous")),
    }
}

fn resolve_link_type(defs: &DefinitionSet, token: &str) -> Result<TypeId, String> {
    defs.types
        .iter()
        .find(|item| {
            item.is_link()
                && (item.key.eq_ignore_ascii_case(token)
                    || item.id.as_uuid().to_string().eq_ignore_ascii_case(token))
        })
        .map(|item| item.id)
        .ok_or_else(|| format!("unknown link type {token}"))
}

fn resolve_attribute_token(defs: &DefinitionSet, token: &str) -> Result<AttributeId, String> {
    if let Some(id) = attribute_from_head(defs, token) {
        return Ok(id);
    }
    Err(format!("unknown attribute {token}"))
}

fn attribute_from_head(defs: &DefinitionSet, head: &str) -> Option<AttributeId> {
    let dashed = underscore_uuid(head);
    if let Ok(id) = Uuid::parse_str(&dashed) {
        let attribute = AttributeId::from_uuid(id);
        if defs.attributes.iter().any(|item| item.id == attribute)
            || standard::is_standard_attribute(attribute)
        {
            return Some(attribute);
        }
    }
    defs.attributes
        .iter()
        .find(|item| item.key.eq_ignore_ascii_case(head))
        .map(|item| item.id)
}

fn underscore_uuid(text: &str) -> String {
    if text.len() == 36 && text.chars().filter(|character| *character == '_').count() == 4 {
        return text.replace('_', "-");
    }
    text.to_owned()
}

fn split_index_suffixes<'a>(body: &'a str, defs: &DefinitionSet) -> (&'a str, Vec<&'a str>) {
    if attribute_from_head(defs, body).is_some() {
        return (body, Vec::new());
    }
    let mut suffixes = Vec::new();
    let mut head = body;
    while let Some((prefix, suffix)) = head.rsplit_once("__") {
        if matches!(suffix, "unit" | "min" | "max") || is_language_suffix(suffix) {
            suffixes.insert(0, suffix);
            head = prefix;
            if attribute_from_head(defs, head).is_some() {
                break;
            }
            continue;
        }
        break;
    }
    (head, suffixes)
}

fn is_language_suffix(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 16
        && text
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
        && !matches!(text, "unit" | "min" | "max")
}

fn split_language(text: &str) -> Result<(String, Option<String>), String> {
    let Some((body, language)) = text.rsplit_once('@') else {
        return Ok((text.to_owned(), None));
    };
    if language.is_empty()
        || language.contains('>')
        || language.contains('.')
        || language.contains('@')
    {
        return Err(format!("invalid language qualifier @{language}"));
    }
    if !language
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || character == '-')
    {
        return Err(format!("invalid language qualifier @{language}"));
    }
    Ok((body.to_owned(), Some(language.to_owned())))
}

fn attribute_of(defs: &DefinitionSet, id: AttributeId) -> Option<&Attribute> {
    defs.attributes.iter().find(|item| item.id == id)
}

fn link_type(defs: &DefinitionSet, id: TypeId) -> Option<&ObjectType> {
    defs.types
        .iter()
        .find(|item| item.id == id && item.is_link())
}

fn label_of(attribute: &Attribute, language: &str) -> String {
    attribute
        .labels
        .iter()
        .find(|(tag, text)| tag.as_str() == language && !text.is_empty())
        .or_else(|| {
            attribute
                .labels
                .iter()
                .find(|(tag, text)| tag.as_str() == "en" && !text.is_empty())
        })
        .map(|(_, text)| text.clone())
        .unwrap_or_else(|| attribute.key.clone())
}

fn with_language(text: String, language: Option<&str>) -> String {
    match language {
        Some(language) => format!("{text} ({language})"),
        None => text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn catalog() -> DefinitionSet {
        serde_json::from_str(include_str!("path_catalog.json")).unwrap()
    }

    fn article(defs: &DefinitionSet) -> TypeId {
        defs.types
            .iter()
            .find(|item| item.key == "article")
            .unwrap()
            .id
    }

    fn product(defs: &DefinitionSet) -> TypeId {
        defs.types
            .iter()
            .find(|item| item.key == "product")
            .unwrap()
            .id
    }

    #[test]
    fn text_and_id_forms_round_trip() {
        let defs = catalog();
        let samples = [
            "net_weight",
            "net_weight.value",
            "net_weight.unit",
            "size.min",
            "size.max",
            "allergens.code",
            "allergens.label",
            "number",
            "name@en",
            "product>brand",
            "articles>number",
            "articles>link.sort_order",
        ];
        for text in samples {
            let from = if text.starts_with("product>") || text.starts_with("articles>link") {
                if text.starts_with("product>") {
                    article(&defs)
                } else {
                    product(&defs)
                }
            } else if text.starts_with("articles>") {
                product(&defs)
            } else {
                article(&defs)
            };
            let path = parse_path_text(text, &defs, Some(from))
                .unwrap_or_else(|error| panic!("{text}: {error}"));
            validate_path(&path, &defs, from).unwrap_or_else(|error| panic!("{text}: {error}"));
            assert_eq!(format_path_text(&path, &defs), text, "{text}");
            let id = format_path_id(&path);
            let again = parse_path_id(&id, &defs, Some(from)).unwrap();
            assert_eq!(again, path, "{id}");
            assert_eq!(format_path_id(&again), id);
        }
    }

    #[test]
    fn a_wrong_part_an_unknown_link_and_two_steps_are_refused() {
        let defs = catalog();
        let article = article(&defs);
        let bad_part = parse_path_text("name.unit", &defs, Some(article)).unwrap();
        assert!(validate_path(&bad_part, &defs, article).is_err());
        assert!(parse_path_text("net_weight.colour", &defs, Some(article)).is_err());
        assert!(parse_path_text("missing>brand", &defs, Some(article)).is_err());
        assert!(parse_path_text("product>brand>name", &defs, Some(article)).is_err());
        let wrong_end = parse_path_text("articles>number", &defs, Some(article)).unwrap_err();
        assert!(
            wrong_end.contains("unknown link") || wrong_end.contains("ambiguous"),
            "{wrong_end}"
        );
    }

    #[test]
    fn labels_name_the_part_and_the_link() {
        let defs = catalog();
        let article = article(&defs);
        let unit = parse_path_text("net_weight.unit", &defs, Some(article)).unwrap();
        assert_eq!(path_label(&unit, &defs, "en"), "Net weight (unit)");
        let brand = parse_path_text("product>brand", &defs, Some(article)).unwrap();
        assert_eq!(path_label(&brand, &defs, "en"), "Product › Brand");
        let sort =
            parse_path_text("articles>link.sort_order", &defs, Some(product(&defs))).unwrap();
        assert_eq!(
            path_label(&sort, &defs, "en"),
            "Articles › link › Sort order"
        );
    }

    #[test]
    fn the_unit_filter_and_the_base_sort_use_index_fields() {
        let defs = catalog();
        let filter = translate_filter_expression("net_weight.unit = g", &defs).unwrap();
        let weight = defs
            .attributes
            .iter()
            .find(|item| item.key == "net_weight")
            .unwrap();
        let unit_field = format!("{}__unit", attribute_index_field(weight.id, None));
        let base = attribute_index_field(weight.id, None);
        assert_eq!(filter, format!("{unit_field}:=g"));
        assert_eq!(
            translate_sort_expression("net_weight:asc", &defs).unwrap(),
            format!("{base}:asc")
        );
        assert!(translate_filter_expression("product>brand = Meadow", &defs).is_err());
    }

    #[test]
    fn an_old_index_field_becomes_a_path_and_stays_a_path() {
        let defs = catalog();
        let weight = defs
            .attributes
            .iter()
            .find(|item| item.key == "net_weight")
            .unwrap();
        let legacy = format!("{}__unit", attribute_index_field(weight.id, None));
        let path = stored_field_as_path(&legacy, &defs);
        assert_eq!(path, format!("{}.unit", weight.id.as_uuid()));
        assert_eq!(stored_field_as_path(&path, &defs), path);
        assert_eq!(stored_field_as_path("type_key", &defs), "type_key");
    }

    #[test]
    fn a_key_containing_a_double_underscore_is_not_a_suffix() {
        let mut defs = catalog();
        for (id, key) in [
            ("018f0000-0000-7000-8000-0000000001c1", "size__min"),
            ("018f0000-0000-7000-8000-0000000001e1", "name__en"),
        ] {
            let attribute: Attribute = serde_json::from_value(json!({
                "id": id,
                "key": key,
                "labels": { "en": key },
                "language_dependent": false,
                "multiple": false,
                "kind": { "type": "text", "max_length": 32 },
                "index": { "searchable": false, "filterable": true, "sortable": false }
            }))
            .unwrap();
            defs.attributes.push(attribute);
        }
        let size = defs
            .attributes
            .iter()
            .find(|item| item.key == "size")
            .unwrap();
        let name = defs
            .attributes
            .iter()
            .find(|item| item.key == "name")
            .unwrap();
        let size_min = defs
            .attributes
            .iter()
            .find(|item| item.key == "size__min")
            .unwrap();
        let name_en = defs
            .attributes
            .iter()
            .find(|item| item.key == "name__en")
            .unwrap();
        assert_eq!(
            stored_field_as_path("size__min", &defs),
            size_min.id.as_uuid().to_string()
        );
        assert_eq!(
            stored_field_as_path("name__en", &defs),
            name_en.id.as_uuid().to_string()
        );
        assert_eq!(
            stored_field_as_path("name__en__sv", &defs),
            format!("{}@sv", name_en.id.as_uuid())
        );
        assert_eq!(
            stored_field_as_path(
                &format!("{}__min", attribute_index_field(size.id, None)),
                &defs
            ),
            format!("{}.min", size.id.as_uuid())
        );
        assert_eq!(
            stored_field_as_path(
                &format!("{}__en", attribute_index_field(name.id, None)),
                &defs
            ),
            format!("{}@en", name.id.as_uuid())
        );
        assert_eq!(stored_field_as_path("retired_attr", &defs), "retired_attr");
    }

    #[test]
    fn the_article_list_includes_standard_attributes_parts_and_one_link_step() {
        let defs = catalog();
        let listed = list_attribute_paths(&defs, article(&defs));
        let texts: Vec<_> = listed.iter().map(|item| item.text.as_str()).collect();
        for expected in [
            "number",
            "name",
            "name@en",
            "created_at",
            "net_weight",
            "net_weight.value",
            "net_weight.unit",
            "size.min",
            "size.max",
            "allergens.code",
            "allergens.label",
            "product>brand",
            "product>link.sort_order",
        ] {
            assert!(texts.contains(&expected), "missing {expected} in {texts:?}");
        }
        assert!(!texts.iter().any(|text| text.matches('>').count() > 1));
    }

    #[test]
    fn id_json_accepts_a_key_and_formats_ids() {
        let defs = catalog();
        let value = json!({
            "attribute": "net_weight",
            "part": "unit"
        });
        let path = parse_path(&value, &defs, Some(article(&defs))).unwrap();
        let id = format_path_id(&path);
        assert_eq!(id["part"], "unit");
        assert!(id.get("via").is_none());
        assert_eq!(id["attribute"], path.attribute.as_uuid().to_string());
    }
}
