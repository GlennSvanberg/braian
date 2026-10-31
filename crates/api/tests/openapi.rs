//! The committed spec is the one clients and CI compare against.

#[test]
fn openapi_spec_matches_the_committed_file() {
    let path =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/api/openapi.json");
    let on_disk = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    let embedded = include_str!("../../../docs/api/openapi.json");
    assert_eq!(
        on_disk, embedded,
        "runtime {} diverges from include_str bake — CI mtime/cache skew",
        path.display()
    );
    let committed: serde_json::Value =
        serde_json::from_str(embedded).expect("docs/api/openapi.json is json");
    let generated = serde_json::to_value(braian_api::openapi_spec()).expect("spec serializes");
    let disk_has_delete = committed["paths"]["/v1/workspaces/{id}"]
        .get("delete")
        .is_some();
    let gen_has_delete = generated["paths"]["/v1/workspaces/{id}"]
        .get("delete")
        .is_some();
    assert_eq!(
        committed, generated,
        "docs/api/openapi.json is stale. Run `just openapi`. delete on disk={disk_has_delete} generated={gen_has_delete} path={}",
        path.display()
    );
}
