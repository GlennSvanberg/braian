use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::id::{AttributeId, GroupId, ObjectId, StructureId, TypeId};

/// One object.
///
/// `groups` holds one group per structure the object is placed in. An object
/// may sit in several structures (a catalog plus a channel tree, for example).
///
/// `values` is keyed by attribute id. The JSON shape of a value depends on the
/// attribute:
///
/// - text: a string (short or long)
/// - number: `{"value":"12.50","unit":"kg"}`, with `unit` omitted when the
///   attribute has no unit family. Integers are exact decimals with no fraction.
/// - range: `{"min":"1","max":"3","unit":"mm"}`
/// - value list: the option code
/// - boolean: `true` or `false`
/// - date: `YYYY-MM-DD`
/// - object reference: a UUIDv7 string
/// - file or media reference: a UUID string
///
/// `multiple` wraps that value in an ordered array. `language_dependent` wraps
/// it in an object keyed by language tag, and the array sits inside the
/// language when both are set: `{"sv":["a","b"],"en-GB":["c"]}`.
///
/// Attributes that do not apply are rejected. Missing attributes other than the
/// type's key attribute are allowed; completeness is a later publication gate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Object {
    pub id: ObjectId,
    pub type_id: TypeId,
    pub model_revision: u64,
    pub groups: BTreeMap<StructureId, GroupId>,
    pub values: BTreeMap<AttributeId, Value>,
}

#[cfg(test)]
mod tests {
    use super::Object;

    #[test]
    fn object_json_roundtrips() {
        let raw = r#"{
            "id": "018f0000-0000-7000-8000-000000000900",
            "type_id": "018f0000-0000-7000-8000-000000000200",
            "model_revision": 1,
            "groups": {
                "018f0000-0000-7000-8000-000000000300": "018f0000-0000-7000-8000-000000000401"
            },
            "values": {
                "018f0000-0000-7000-8000-000000000100": "SKU-1",
                "018f0000-0000-7000-8000-000000000101": {"sv": "Label", "en-GB": "Name"}
            }
        }"#;
        let object: Object = serde_json::from_str(raw).unwrap();
        let back: Object = serde_json::from_str(&serde_json::to_string(&object).unwrap()).unwrap();
        assert_eq!(object, back);
        assert_eq!(
            object
                .values
                .values()
                .next()
                .and_then(serde_json::Value::as_str),
            Some("SKU-1")
        );
    }
}
