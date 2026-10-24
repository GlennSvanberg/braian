use std::collections::HashSet;

use braian_model::{AttributeKind, DefinitionSet, ErrorCode};
use braian_rules::CompiledModel;
use braian_synth::{
    ExportFormat, Tier, all_mutations, definitions, export, measure, objects, write_demo,
};

#[test]
fn tiers_have_the_planned_shape() {
    for tier in Tier::ALL {
        let defs = definitions(tier, 1);
        assert_eq!(defs.languages.len(), tier.language_count());
        assert!(defs.languages.iter().any(|tag| tag.as_str().contains('-')));
        let model = CompiledModel::compile(&defs).expect("definitions");
        let type_id = model.type_id("item").unwrap();
        let groups = groups(&model);
        let applicable = model.applicable_ids(type_id, &groups).unwrap();
        assert_eq!(applicable.len(), tier.attribute_count());
        let localized = defs
            .attributes
            .iter()
            .filter(|attribute| attribute.language_dependent)
            .count();
        assert_eq!(localized, tier.attribute_count() * 30 / 100);
        let multiple = defs
            .attributes
            .iter()
            .filter(|attribute| attribute.multiple)
            .count();
        assert_eq!(multiple, tier.attribute_count() / 10);
        let unused = model.attribute_by_key("unused").unwrap();
        assert!(!applicable.contains(&unused.id));
        let key = model
            .attribute(model.key_attribute(type_id).unwrap())
            .unwrap();
        assert_eq!(key.key, "sku");
        assert!(!key.language_dependent && !key.multiple);

        let mut shapes = HashSet::new();
        for attribute in defs
            .attributes
            .iter()
            .filter(|attribute| attribute.key != "unused")
        {
            shapes.insert(match &attribute.kind {
                AttributeKind::Text { .. } => "text",
                AttributeKind::Number {
                    unit_family: Some(_),
                    ..
                } => "number-unit",
                AttributeKind::Number { .. } => "number",
                AttributeKind::ValueList { .. } => "list",
                AttributeKind::Boolean => "bool",
                AttributeKind::Date => "date",
                AttributeKind::FileRef { .. } => "file",
                AttributeKind::Range { .. } => "range",
                AttributeKind::ObjectRef { .. } => "ref",
            });
        }
        assert_eq!(shapes.len(), 7, "{shapes:?}");
    }
}

#[test]
fn same_seed_produces_the_same_bytes() {
    let first =
        serde_json::to_vec(&objects(&definitions(Tier::Light, 4), Tier::Light, 4, 2)).unwrap();
    let second =
        serde_json::to_vec(&objects(&definitions(Tier::Light, 4), Tier::Light, 4, 2)).unwrap();
    assert_eq!(first, second);
    let other =
        serde_json::to_vec(&objects(&definitions(Tier::Light, 5), Tier::Light, 5, 2)).unwrap();
    assert_ne!(first, other);
}

#[test]
fn generated_light_objects_match_the_fill_rate_and_validate() {
    let defs = definitions(Tier::Light, 3);
    let model = CompiledModel::compile(&defs).unwrap();
    let generated = objects(&defs, Tier::Light, 3, 4);
    assert_eq!(generated[0].values.len(), 30);
    for object in &generated {
        let errors = model.validate(object);
        assert!(errors.is_empty(), "{errors:?}");
        let localized = object
            .values
            .iter()
            .find(|(id, _)| model.attribute(**id).unwrap().language_dependent);
        let (_, value) = localized.expect("a filled localized attribute");
        assert_eq!(value.as_object().unwrap().len(), defs.languages.len());
    }
}

#[test]
fn definitions_roundtrip_json_and_yaml() {
    for tier in [Tier::Light, Tier::Medium] {
        let defs = definitions(tier, 2);
        let json = serde_json::to_string(&defs).unwrap();
        let back: DefinitionSet = serde_json::from_str(&json).unwrap();
        assert_eq!(defs, back);
        let yaml = export(&defs, ExportFormat::Yaml).unwrap();
        let from_yaml: DefinitionSet = serde_norway::from_str(&yaml).unwrap();
        assert_eq!(defs, from_yaml);
    }
}

#[test]
fn mutations_fail_with_the_expected_error() {
    for seed in 1..=3 {
        let defs = definitions(Tier::Light, seed);
        let model = CompiledModel::compile(&defs).unwrap();
        let object = &objects(&defs, Tier::Light, seed, 1)[0];
        let mutations = all_mutations(object, &defs);
        let mut seen = HashSet::new();
        for (broken, expected) in &mutations {
            let errors = model.validate(broken);
            assert_eq!(errors.len(), 1, "{expected:?} produced {errors:?}");
            assert_eq!(errors[0].code, expected.code);
            assert_eq!(errors[0].attribute, expected.attribute);
            assert_eq!(errors[0].language, expected.language);
            assert_eq!(errors[0].index, expected.index);
            seen.insert(expected.code);
        }
        for code in ErrorCode::ALL {
            // Integer, range and object-reference failures need attribute kinds the
            // synthetic tiers do not generate. Those kinds are covered in braian-rules.
            if matches!(
                code,
                ErrorCode::NotInteger | ErrorCode::RangeInverted | ErrorCode::InvalidObjectRef
            ) {
                continue;
            }
            assert!(seen.contains(&code), "missing {code}");
        }
    }
}

#[test]
fn heavy_objects_are_larger_than_light_ones() {
    let light = measure(Tier::Light, 1, 1);
    let heavy = measure(Tier::Heavy, 1, 1);
    assert!(heavy.uuid_json.mean > light.uuid_json.mean);
    assert!(heavy.uuid_json.mean > heavy.human_json.mean);
    assert!(heavy.language_parts_sum.mean > heavy.independent.mean);
}

#[test]
fn demo_files_roundtrip_through_rust_validation() {
    let dir = std::env::temp_dir().join(format!("braian-demo-1-{}", std::process::id()));
    let summary = write_demo(&dir, 1).unwrap();
    assert_eq!(summary.valid, 10);
    assert_eq!(summary.broken, 4);
    let reports = std::fs::read_to_string(dir.join("rust-reports.jsonl")).unwrap();
    let lines: Vec<_> = reports.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(lines.len(), 14);
    assert!(lines[..10].iter().all(|line| *line == "[]"));
    assert!(lines[10..].iter().all(|line| *line != "[]"));
    let _ = std::fs::remove_dir_all(&dir);
}

fn groups(
    model: &CompiledModel,
) -> std::collections::BTreeMap<braian_model::StructureId, braian_model::GroupId> {
    std::collections::BTreeMap::from([
        (
            model.structure_id("catalog").unwrap(),
            model.group_id("catalog", "leaf").unwrap(),
        ),
        (
            model.structure_id("channel").unwrap(),
            model.group_id("channel", "flat").unwrap(),
        ),
    ])
}
