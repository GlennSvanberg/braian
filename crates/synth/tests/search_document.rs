//! Search documents for the medium tier the perf runs use.

use braian_rules::CompiledModel;
use braian_search::document;
use braian_synth::{IndexBudget, Tier, definitions_with, objects_iter};

#[test]
fn medium_search_document_keeps_filter_fields_and_drops_searchable_only_copies() {
    let defs = definitions_with(
        Tier::Medium,
        1,
        IndexBudget {
            searchable: Some(10),
            ..IndexBudget::default()
        },
    );
    let model = CompiledModel::compile(&defs).expect("definitions compile");
    let object = objects_iter(&defs, Tier::Medium, 1, 0..1)
        .next()
        .expect("object");
    let doc = document(&model, &defs, &object, "SKU-00000000", "item", 1, 1, false);
    let bytes = serde_json::to_vec(&doc).expect("json").len();
    let keys = doc.as_object().expect("object").len();

    let mut own_fields = 0usize;
    let mut omitted_fields = 0usize;
    for attribute in &defs.attributes {
        let width = if attribute.language_dependent {
            defs.languages.len()
        } else {
            1
        };
        let stored = attribute.index.filterable || attribute.index.sortable;
        let searchable_only = attribute.index.searchable && !stored;
        if stored {
            own_fields += width;
        }
        if searchable_only {
            omitted_fields += width;
            let prefix = format!("a_{}", attribute.key);
            assert!(
                doc.as_object()
                    .expect("object")
                    .keys()
                    .all(|key| key != &prefix && !key.starts_with(&format!("{prefix}__"))),
                "searchable-only field {prefix} was stored again"
            );
        }
    }
    assert!(doc.get("search_sv").is_some(), "missing search blob");
    assert_eq!(omitted_fields, 50);
    assert_eq!(own_fields, 518);
    assert_eq!(keys, 312);
    // `a_<attribute id>` is longer than `a_<key>`. The field count stays the same.
    assert_eq!(bytes, 18_898);
    assert_eq!(
        defs.attributes
            .iter()
            .filter(|a| a.index.searchable)
            .count(),
        10
    );
    assert_eq!(
        defs.attributes
            .iter()
            .filter(|a| a.index.filterable)
            .count(),
        100
    );
    assert_eq!(
        defs.attributes.iter().filter(|a| a.index.sortable).count(),
        40
    );
}
