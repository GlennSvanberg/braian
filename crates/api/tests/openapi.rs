//! The committed spec is the one clients and CI compare against.

#[test]
fn openapi_spec_matches_the_committed_file() {
    let path =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/api/openapi.json");
    let committed: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("read {}: {error}", path.display())),
    )
    .expect("docs/api/openapi.json is json");
    let generated = serde_json::to_value(openpim_api::openapi_spec()).expect("spec serializes");
    assert_eq!(
        committed, generated,
        "docs/api/openapi.json is stale. Run `just openapi`."
    );
}
