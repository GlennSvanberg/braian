//! The same rules as `openpim-rules`, reachable from JavaScript.
//!
//! Definitions and objects are JSON strings, so the WebAssembly build and the
//! native crate run the same serde path and produce the same report.

use openpim_model::{DefinitionSet, Object, ObjectPatch};
use openpim_rules::CompiledModel;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct Validator {
    model: CompiledModel,
}

#[wasm_bindgen]
impl Validator {
    #[wasm_bindgen(constructor)]
    pub fn new(definitions_json: &str) -> Result<Validator, JsValue> {
        let defs: DefinitionSet = serde_json::from_str(definitions_json)
            .map_err(|err| JsValue::from_str(&err.to_string()))?;
        let model = CompiledModel::compile(&defs).map_err(|errors| {
            JsValue::from_str(
                &serde_json::to_string(&errors)
                    .unwrap_or_else(|_| "invalid definitions".to_owned()),
            )
        })?;
        Ok(Validator { model })
    }

    /// JSON array of validation errors. Identical to the native report.
    pub fn validate(&self, object_json: &str) -> Result<String, JsValue> {
        let object: Object =
            serde_json::from_str(object_json).map_err(|err| JsValue::from_str(&err.to_string()))?;
        Ok(serde_json::to_string(&self.model.validate(&object))
            .expect("validation report serializes"))
    }

    /// JSON `{errors, warnings}` for a patch. Same report as the native crate.
    pub fn validate_patch(&self, object_json: &str, patch_json: &str) -> Result<String, JsValue> {
        let object: Object =
            serde_json::from_str(object_json).map_err(|err| JsValue::from_str(&err.to_string()))?;
        let patch: ObjectPatch =
            serde_json::from_str(patch_json).map_err(|err| JsValue::from_str(&err.to_string()))?;
        Ok(
            serde_json::to_string(&self.model.validate_patch(&object, &patch))
                .expect("patch report serializes"),
        )
    }
}
