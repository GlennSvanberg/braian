use std::collections::BTreeMap;

use braian_model::{
    Attribute, AttributeId, AttributeKind, AttributePatch, IndexFlags, Object, ObjectType, TypeId,
    Uuid, ValuePatch, apply_value_patch,
};
use braian_rules::CompiledModel;
use braian_store::testing::TestDb;
use braian_store::{Actor, BatchKind, ObjectOp, PrincipalKind};
use braian_synth::{EditKind, Tier, definitions, edits, objects, objects_iter};

fn actor(principal: braian_store::PrincipalId, behalf: Option<braian_store::PrincipalId>) -> Actor {
    Actor {
        actor: principal,
        on_behalf_of: behalf,
        kind: BatchKind::Interactive,
        source: "test".into(),
    }
}

async fn setup(tier: Tier, seed: u64) -> (TestDb, braian_model::DefinitionSet, Actor) {
    let db = TestDb::create().await;
    let defs = definitions(tier, seed);
    let human = db
        .store()
        .create_principal(PrincipalKind::Human, "ada")
        .await
        .unwrap();
    let who = actor(human, None);
    db.store().put_definitions(&who, &defs).await.unwrap();
    (db, defs, who)
}

#[tokio::test]
async fn light_and_medium_objects_roundtrip() {
    for (tier, seeds) in [
        (Tier::Light, &[1u64, 2, 9, 42][..]),
        (Tier::Medium, &[3][..]),
        (Tier::Heavy, &[1][..]),
    ] {
        for seed in seeds {
            let (db, defs, who) = setup(tier, *seed).await;
            let model = CompiledModel::compile(&defs).unwrap();
            let sample: Vec<_> = objects_iter(&defs, tier, *seed, 0..2).collect();
            for object in &sample {
                assert!(model.validate(object).is_empty(), "{tier:?}");
            }
            let one = sample[0].clone();
            db.store()
                .write_batch(&who, vec![ObjectOp::Create(one.clone())])
                .await
                .unwrap();
            let stored = db.store().get_object(one.id).await.unwrap().unwrap();
            assert_eq!(stored.object, one);
            assert_eq!(stored.version, 1);
            assert!(stored.key.starts_with("SKU-"));

            let report = db
                .store()
                .bulk_create(
                    &Actor {
                        kind: BatchKind::Bulk,
                        ..who.clone()
                    },
                    futures::stream::iter(sample.into_iter().skip(1)),
                    10,
                )
                .await
                .unwrap();
            assert!(report.errors.is_empty(), "{:?}", report.errors);
            let again = db
                .store()
                .get_by_key(one.type_id, &stored.key)
                .await
                .unwrap();
            assert_eq!(again.unwrap().object, one);
        }
    }
}

#[tokio::test]
async fn key_is_unique_per_type_and_reusable_after_delete() {
    let (db, mut defs, who) = setup(Tier::Light, 4).await;
    let key_attribute = defs.types[0].key_attribute;
    let second = TypeId::from_uuid(Uuid::now_v7());
    defs.types.push(ObjectType {
        id: second,
        key: "other".into(),
        labels: BTreeMap::new(),
        key_attribute,
        attributes: vec![key_attribute],
    });
    defs.model_revision = 2;
    db.store().put_definitions(&who, &defs).await.unwrap();

    let first = objects(&defs, Tier::Light, 4, 1).pop().unwrap();
    let mut other = first.clone();
    other.id = braian_model::ObjectId::from_uuid(Uuid::now_v7());
    other.type_id = second;
    other.groups.clear();
    other.values.retain(|id, _| *id == key_attribute);

    db.store()
        .write_batch(
            &who,
            vec![
                ObjectOp::Create(first.clone()),
                ObjectOp::Create(other.clone()),
            ],
        )
        .await
        .unwrap();
    assert!(
        db.store()
            .get_by_key(
                second,
                &db.store().get_object(first.id).await.unwrap().unwrap().key
            )
            .await
            .unwrap()
            .is_some()
    );

    let duplicate = Object {
        id: braian_model::ObjectId::from_uuid(Uuid::now_v7()),
        ..first.clone()
    };
    let error = db
        .store()
        .write_batch(&who, vec![ObjectOp::Create(duplicate)])
        .await
        .unwrap_err();
    assert!(error.to_string().contains("duplicate key"), "{error}");

    db.store()
        .write_batch(
            &who,
            vec![ObjectOp::Delete {
                id: first.id,
                base_version: None,
            }],
        )
        .await
        .unwrap();
    let reused = Object {
        id: braian_model::ObjectId::from_uuid(Uuid::now_v7()),
        ..first
    };
    db.store()
        .write_batch(&who, vec![ObjectOp::Create(reused.clone())])
        .await
        .unwrap();
    let found = db
        .store()
        .get_by_key(reused.type_id, "SKU-00000000")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(found.object.id, reused.id);
    assert!(
        db.store()
            .get_object(first.id)
            .await
            .unwrap()
            .unwrap()
            .deleted_at
            .is_some()
    );
}

#[tokio::test]
async fn language_diff_records_only_the_changed_language() {
    let (db, defs, who) = setup(Tier::Light, 5).await;
    let object = objects(&defs, Tier::Light, 5, 1).pop().unwrap();
    db.store()
        .write_batch(&who, vec![ObjectOp::Create(object.clone())])
        .await
        .unwrap();
    let edit = edits(&defs, Tier::Light, 5, EditKind::OneLanguage, 0..1)
        .next()
        .unwrap();
    let receipt = db
        .store()
        .write_batch(
            &who,
            vec![ObjectOp::Update {
                id: object.id,
                patch: braian_model::ObjectPatch {
                    values: edit.patch.clone(),
                    groups: BTreeMap::new(),
                },
                base_version: 1,
                read_version: 1,
            }],
        )
        .await
        .unwrap();
    assert_eq!(receipt.versions, vec![(object.id, 2)]);

    let mut expected = object.clone();
    apply_value_patch(&mut expected.values, &edit.patch);
    let stored = db.store().get_object(object.id).await.unwrap().unwrap();
    assert_eq!(stored.object.values, expected.values);
    assert_eq!(stored.version, 2);

    let changes = db.store().changes_for(object.id).await.unwrap();
    let update = changes
        .iter()
        .find(|change| change.op == braian_store::ChangeOp::Update)
        .unwrap();
    let diff = update.diff.as_ref().unwrap().as_object().unwrap();
    assert_eq!(diff.len(), 1);
    let entry = diff.values().next().unwrap().as_object().unwrap();
    let before = entry.get("before").unwrap().as_object().unwrap();
    let after = entry.get("after").unwrap().as_object().unwrap();
    assert_eq!(before.len(), 1);
    assert_eq!(
        before.keys().collect::<Vec<_>>(),
        after.keys().collect::<Vec<_>>()
    );
    assert_ne!(before, after);

    let partition = db.store().change_partition(update.seq).await.unwrap();
    assert!(partition.starts_with("changes_20"), "{partition}");
    assert_ne!(partition, "changes_default");
}

#[tokio::test]
async fn a_failed_batch_rolls_back() {
    let (db, defs, who) = setup(Tier::Light, 6).await;
    let mut sample = objects(&defs, Tier::Light, 6, 2);
    let first = sample.remove(0);
    let mut second = sample.remove(0);
    second.values = first.values.clone();
    let error = db
        .store()
        .write_batch(
            &who,
            vec![ObjectOp::Create(first.clone()), ObjectOp::Create(second)],
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("duplicate key"), "{error}");
    assert!(db.store().get_object(first.id).await.unwrap().is_none());
}

#[tokio::test]
async fn an_agent_needs_on_behalf_of() {
    let (db, defs, who) = setup(Tier::Light, 8).await;
    let agent = db
        .store()
        .create_principal(PrincipalKind::Agent, "scout")
        .await
        .unwrap();
    let object = objects(&defs, Tier::Light, 8, 1).pop().unwrap();
    let error = db
        .store()
        .write_batch(&actor(agent, None), vec![ObjectOp::Create(object.clone())])
        .await
        .unwrap_err();
    assert!(error.to_string().contains("on_behalf_of"), "{error}");

    let human = who.actor;
    db.store()
        .write_batch(
            &actor(agent, Some(human)),
            vec![ObjectOp::Create(object.clone())],
        )
        .await
        .unwrap();
    let changes = db.store().changes_for(object.id).await.unwrap();
    assert_eq!(changes[0].actor, agent);
    assert_eq!(changes[0].on_behalf_of, Some(human));
}

#[tokio::test]
async fn soft_delete_keeps_values_and_writes_a_change() {
    let (db, defs, who) = setup(Tier::Light, 11).await;
    let object = objects(&defs, Tier::Light, 11, 1).pop().unwrap();
    db.store()
        .write_batch(&who, vec![ObjectOp::Create(object.clone())])
        .await
        .unwrap();
    let receipt = db
        .store()
        .write_batch(
            &who,
            vec![ObjectOp::Delete {
                id: object.id,
                base_version: None,
            }],
        )
        .await
        .unwrap();
    assert_eq!(receipt.versions[0].1, 2);
    let stored = db.store().get_object(object.id).await.unwrap().unwrap();
    assert!(stored.deleted_at.is_some());
    assert_eq!(stored.object.values, object.values);
    assert!(
        db.store()
            .get_by_key(object.type_id, &stored.key)
            .await
            .unwrap()
            .is_none()
    );
    let delete = db
        .store()
        .changes_for(object.id)
        .await
        .unwrap()
        .into_iter()
        .find(|change| change.op == braian_store::ChangeOp::Delete)
        .unwrap();
    assert!(delete.diff.is_none());
    let missing = db
        .store()
        .write_batch(
            &who,
            vec![ObjectOp::Delete {
                id: object.id,
                base_version: None,
            }],
        )
        .await
        .unwrap_err();
    assert!(missing.to_string().contains("deleted"), "{missing}");
}

#[tokio::test]
async fn adding_attributes_does_not_move_existing_values() {
    let (db, mut defs, who) = setup(Tier::Light, 13).await;
    let object = objects(&defs, Tier::Light, 13, 1).pop().unwrap();
    let sku = defs.types[0].key_attribute;
    db.store()
        .write_batch(&who, vec![ObjectOp::Create(object.clone())])
        .await
        .unwrap();
    let before = db.store().stored_parts(object.id).await.unwrap();
    let original_parts = parts_containing(&before, sku);
    assert_eq!(original_parts.len(), 1);

    for index in 0..150 {
        defs.attributes.push(Attribute {
            id: AttributeId::from_uuid(Uuid::from_u128(50_000 + index)),
            key: format!("0{index:04}"),
            labels: BTreeMap::new(),
            language_dependent: false,
            multiple: false,
            kind: AttributeKind::Text {
                max_length: None,
                form: braian_model::TextForm::Short,
            },
            index: IndexFlags::default(),
        });
    }
    defs.model_revision += 1;
    db.store().put_definitions(&who, &defs).await.unwrap();

    db.store()
        .write_batch(
            &who,
            vec![ObjectOp::Update {
                id: object.id,
                patch: braian_model::ObjectPatch {
                    values: ValuePatch {
                        attributes: BTreeMap::from([(
                            sku,
                            AttributePatch::Replace {
                                value: serde_json::json!("SKU-STABLE"),
                            },
                        )]),
                    },
                    groups: BTreeMap::new(),
                },
                base_version: 1,
                read_version: 1,
            }],
        )
        .await
        .unwrap();

    let after = db.store().stored_parts(object.id).await.unwrap();
    assert_eq!(parts_containing(&after, sku), original_parts);
    let stored = db.store().get_object(object.id).await.unwrap().unwrap();
    assert_eq!(
        stored.object.values.get(&sku),
        Some(&serde_json::json!("SKU-STABLE"))
    );
}

#[tokio::test]
async fn edits_merge_per_attribute_language_and_structure() {
    let (db, defs, who) = setup(Tier::Light, 17).await;
    let object = objects(&defs, Tier::Light, 17, 1).pop().unwrap();
    db.store()
        .write_batch(&who, vec![ObjectOp::Create(object.clone())])
        .await
        .unwrap();
    let edit = edits(&defs, Tier::Light, 17, EditKind::OneLanguage, 0..1)
        .next()
        .unwrap();
    let (attribute, first_language) = match edit.patch.attributes.iter().next().unwrap() {
        (id, braian_model::AttributePatch::Languages { values }) => {
            (*id, values.keys().next().unwrap().clone())
        }
        other => panic!("expected a language patch, got {other:?}"),
    };
    let other_language = defs
        .languages
        .iter()
        .find(|language| *language != &first_language)
        .unwrap()
        .clone();

    db.store()
        .write_batch(
            &who,
            vec![value_update(object.id, edit.patch.clone(), 1, 1)],
        )
        .await
        .unwrap();
    let merged = db
        .store()
        .write_batch(
            &who,
            vec![value_update(
                object.id,
                one_language(attribute, &other_language, serde_json::json!("other")),
                1,
                2,
            )],
        )
        .await
        .unwrap();
    assert_eq!(merged.versions[0].1, 3);

    let conflict = db
        .store()
        .write_batch(
            &who,
            vec![value_update(
                object.id,
                one_language(attribute, &first_language, serde_json::json!("clash")),
                1,
                3,
            )],
        )
        .await
        .unwrap_err();
    match conflict {
        braian_store::Error::Conflict {
            current_version,
            conflicts,
        } => {
            assert_eq!(current_version, 3);
            assert!(
                conflicts.iter().any(|conflict| {
                    conflict.attribute == Some(attribute)
                        && conflict.language.as_ref() == Some(&first_language)
                }),
                "{conflicts:?}"
            );
        }
        other => panic!("expected a conflict, got {other}"),
    }

    let plain = defs
        .attributes
        .iter()
        .find(|attribute| {
            !attribute.language_dependent && attribute.id != defs.types[0].key_attribute
        })
        .unwrap();
    db.store()
        .write_batch(
            &who,
            vec![value_update(
                object.id,
                ValuePatch {
                    attributes: BTreeMap::from([(
                        plain.id,
                        AttributePatch::Replace {
                            value: serde_json::json!("plain"),
                        },
                    )]),
                },
                1,
                3,
            )],
        )
        .await
        .unwrap();
    let again = db
        .store()
        .write_batch(
            &who,
            vec![value_update(
                object.id,
                ValuePatch {
                    attributes: BTreeMap::from([(
                        plain.id,
                        AttributePatch::Replace {
                            value: serde_json::json!("plain-2"),
                        },
                    )]),
                },
                1,
                4,
            )],
        )
        .await
        .unwrap_err();
    assert!(
        matches!(again, braian_store::Error::Conflict { .. }),
        "{again}"
    );

    let catalog = defs.structures[0].id;
    let root = defs.structures[0].groups[0].id;
    let channel = defs.structures[1].id;
    db.store()
        .write_batch(
            &who,
            vec![ObjectOp::Update {
                id: object.id,
                patch: braian_model::ObjectPatch {
                    values: ValuePatch::default(),
                    groups: BTreeMap::from([(catalog, Some(root))]),
                },
                base_version: 4,
                read_version: 4,
            }],
        )
        .await
        .unwrap();
    db.store()
        .write_batch(
            &who,
            vec![ObjectOp::Update {
                id: object.id,
                patch: braian_model::ObjectPatch {
                    values: ValuePatch::default(),
                    groups: BTreeMap::from([(channel, None)]),
                },
                base_version: 4,
                read_version: 5,
            }],
        )
        .await
        .unwrap();
    let stored = db.store().get_object(object.id).await.unwrap().unwrap();
    assert_eq!(stored.object.groups.get(&catalog), Some(&root));
    assert!(!stored.object.groups.contains_key(&channel));
    assert_eq!(stored.object.model_revision, defs.model_revision);

    let placement_conflict = db
        .store()
        .write_batch(
            &who,
            vec![ObjectOp::Update {
                id: object.id,
                patch: braian_model::ObjectPatch {
                    values: ValuePatch::default(),
                    groups: BTreeMap::from([(catalog, Some(defs.structures[0].groups[1].id))]),
                },
                base_version: 4,
                read_version: 6,
            }],
        )
        .await
        .unwrap_err();
    assert!(
        matches!(placement_conflict, braian_store::Error::Conflict { .. }),
        "{placement_conflict}"
    );

    let delete_conflict = db
        .store()
        .write_batch(
            &who,
            vec![ObjectOp::Delete {
                id: object.id,
                base_version: Some(1),
            }],
        )
        .await
        .unwrap_err();
    assert!(
        matches!(delete_conflict, braian_store::Error::Conflict { .. }),
        "{delete_conflict}"
    );
}

#[tokio::test]
async fn stale_read_base_ahead_key_change_and_language_removal() {
    let (db, defs, who) = setup(Tier::Light, 19).await;
    let mut sample = objects(&defs, Tier::Light, 19, 2);
    let object = sample.remove(0);
    let other = sample.remove(0);
    db.store()
        .write_batch(
            &who,
            vec![
                ObjectOp::Create(object.clone()),
                ObjectOp::Create(other.clone()),
            ],
        )
        .await
        .unwrap();

    let original_key = object
        .values
        .get(&defs.types[0].key_attribute)
        .and_then(|value| value.as_str())
        .unwrap()
        .to_owned();
    db.store()
        .write_batch(
            &who,
            vec![value_update(
                object.id,
                ValuePatch {
                    attributes: BTreeMap::from([(
                        defs.types[0].key_attribute,
                        AttributePatch::Replace {
                            value: serde_json::json!("SKU-NEXT"),
                        },
                    )]),
                },
                1,
                1,
            )],
        )
        .await
        .unwrap();
    let stale = db
        .store()
        .write_batch(
            &who,
            vec![value_update(object.id, ValuePatch::default(), 1, 1)],
        )
        .await
        .unwrap_err();
    assert!(
        matches!(stale, braian_store::Error::StaleRead { current_version: 2 }),
        "{stale}"
    );
    let ahead = db
        .store()
        .write_batch(
            &who,
            vec![value_update(object.id, ValuePatch::default(), 9, 2)],
        )
        .await
        .unwrap_err();
    assert!(
        matches!(ahead, braian_store::Error::BaseAhead { current_version: 2 }),
        "{ahead}"
    );

    let found = db
        .store()
        .get_by_key(object.type_id, "SKU-NEXT")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(found.object.id, object.id);
    assert!(
        db.store()
            .get_by_key(object.type_id, &original_key)
            .await
            .unwrap()
            .is_none()
    );
    let collision = db
        .store()
        .write_batch(
            &who,
            vec![value_update(
                other.id,
                ValuePatch {
                    attributes: BTreeMap::from([(
                        defs.types[0].key_attribute,
                        AttributePatch::Replace {
                            value: serde_json::json!("SKU-NEXT"),
                        },
                    )]),
                },
                1,
                1,
            )],
        )
        .await
        .unwrap_err();
    assert!(
        matches!(collision, braian_store::Error::DuplicateKey { .. }),
        "{collision}"
    );

    let edit = edits(&defs, Tier::Light, 19, EditKind::OneLanguage, 0..1)
        .next()
        .unwrap();
    let (attribute, language) = match edit.patch.attributes.iter().next().unwrap() {
        (id, braian_model::AttributePatch::Languages { values }) => {
            (*id, values.keys().next().unwrap().clone())
        }
        other => panic!("expected a language patch, got {other:?}"),
    };
    // The object is still at version 2. Removal uses that version as both base and read.
    db.store()
        .write_batch(
            &who,
            vec![value_update(
                object.id,
                clear_language(attribute, &language),
                2,
                2,
            )],
        )
        .await
        .unwrap();
    let stored = db.store().get_object(object.id).await.unwrap().unwrap();
    let value = stored
        .object
        .values
        .get(&attribute)
        .and_then(|value| value.get(language.as_str()));
    assert_eq!(value, None);
}

fn value_update(id: braian_model::ObjectId, values: ValuePatch, base: u64, read: u64) -> ObjectOp {
    ObjectOp::Update {
        id,
        patch: braian_model::ObjectPatch {
            values,
            groups: BTreeMap::new(),
        },
        base_version: base,
        read_version: read,
    }
}

fn one_language(
    id: AttributeId,
    language: &braian_model::LanguageTag,
    value: serde_json::Value,
) -> ValuePatch {
    let mut values = BTreeMap::new();
    values.insert(language.clone(), Some(value));
    ValuePatch {
        attributes: BTreeMap::from([(id, AttributePatch::Languages { values })]),
    }
}

fn clear_language(id: AttributeId, language: &braian_model::LanguageTag) -> ValuePatch {
    let mut values = BTreeMap::new();
    values.insert(language.clone(), None);
    ValuePatch {
        attributes: BTreeMap::from([(id, AttributePatch::Languages { values })]),
    }
}

fn parts_containing(parts: &BTreeMap<String, serde_json::Value>, id: AttributeId) -> Vec<String> {
    let key = id.as_uuid().to_string();
    parts
        .iter()
        .filter(|(_, document)| {
            document
                .as_object()
                .is_some_and(|document| document.contains_key(&key))
        })
        .map(|(part, _)| part.clone())
        .collect()
}
