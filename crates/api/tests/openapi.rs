//! The committed spec is the one clients and CI compare against.

#[test]
fn openapi_spec_matches_the_committed_file() {
    // include_str so docs/api is a compile input under CI mtime stabilization.
    let committed: serde_json::Value =
        serde_json::from_str(include_str!("../../../docs/api/openapi.json"))
            .expect("docs/api/openapi.json is json");
    let generated = serde_json::to_value(braian_api::openapi_spec()).expect("spec serializes");
    assert_eq!(
        committed, generated,
        "docs/api/openapi.json is stale. Run `just openapi`."
    );
}
