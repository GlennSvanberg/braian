use std::fs;
use std::path::PathBuf;

use openpim_model::{AttributeId, ErrorCode, LanguageTag, Object};
use openpim_rules::CompiledModel;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Case {
    name: String,
    object: Object,
    expected: Vec<Located>,
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
        let mut got = model
            .validate(&case.object)
            .into_iter()
            .map(|error| Located {
                attribute: error.attribute,
                language: error.language,
                index: error.index,
                code: error.code,
            })
            .collect::<Vec<_>>();
        let mut expected = case.expected;
        got.sort_by_key(location_key);
        expected.sort_by_key(location_key);
        assert_eq!(got, expected, "{}", case.name);
    }
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
