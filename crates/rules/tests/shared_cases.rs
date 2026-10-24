use std::fs;
use std::path::PathBuf;

use braian_model::{AttributeId, ErrorCode, LanguageTag, Object, ObjectPatch};
use braian_rules::CompiledModel;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Case {
    name: String,
    object: Object,
    #[serde(default)]
    patch: Option<ObjectPatch>,
    expected: Vec<Located>,
    #[serde(default)]
    warnings: Vec<Located>,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
struct Located {
    #[serde(default)]
    attribute: Option<AttributeId>,
    #[serde(default)]
    language: Option<LanguageTag>,
    #[serde(default)]
    index: Option<usize>,
    code: ErrorCode,
}

#[test]
fn shared_cases_match_rust_rules() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/rules");
    let definitions = fs::read_to_string(root.join("definitions.json")).unwrap();
    let defs = serde_json::from_str(&definitions).unwrap();
    let model = CompiledModel::compile(&defs).expect("shared definitions");
    let mut cases = fs::read_dir(root.join("cases"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect::<Vec<_>>();
    cases.sort();
    assert!(
        cases.len() >= 25,
        "expected the shared rule cases, found {}",
        cases.len()
    );

    for path in cases {
        let case: Case = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        let (errors, warnings) = match &case.patch {
            Some(patch) => {
                let report = model.validate_patch(&case.object, patch);
                (report.errors, report.warnings)
            }
            None => (model.validate(&case.object), Vec::new()),
        };
        assert_eq!(
            located(errors),
            sorted(case.expected),
            "{} errors",
            case.name
        );
        assert_eq!(
            located(warnings),
            sorted(case.warnings),
            "{} warnings",
            case.name
        );
    }
}

fn located(errors: Vec<braian_model::ValidationError>) -> Vec<Located> {
    sorted(
        errors
            .into_iter()
            .map(|error| Located {
                attribute: error.attribute,
                language: error.language,
                index: error.index,
                code: error.code,
            })
            .collect(),
    )
}

fn sorted(mut errors: Vec<Located>) -> Vec<Located> {
    errors.sort_by_key(location_key);
    errors
}

fn location_key(error: &Located) -> (String, String, usize, ErrorCode) {
    (
        error.attribute.map(|id| id.to_string()).unwrap_or_default(),
        error
            .language
            .as_ref()
            .map(LanguageTag::as_str)
            .unwrap_or("")
            .to_owned(),
        error.index.unwrap_or(usize::MAX),
        error.code,
    )
}
