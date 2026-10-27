use std::collections::BTreeMap;

use braian_model::{
    Attribute, AttributeId, AttributeKind, AttributePatch, IndexFlags, Object, ObjectType, TypeId,
    Uuid, ValuePatch, apply_value_patch,
};
use braian_rules::CompiledModel;
use braian_store::testing::TestDb;
use braian_store::{Actor, BatchKind, DbLane, ObjectOp, ObjectProjection, PrincipalKind};
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
async fn principals_load_in_one_batch() {
    let db = TestDb::create().await;
    let ada = db
        .store()
        .upsert_principal("ada", PrincipalKind::Human, "Ada")
        .await
        .unwrap();
    let scout = db
        .store()
        .upsert_principal("scout", PrincipalKind::Agent, "Scout")
        .await
        .unwrap();
    let missing = braian_store::PrincipalId::new();
    let found = db
        .store()
        .principals(&[ada, missing, scout, ada])
        .await
        .unwrap();
    assert_eq!(found.len(), 2);
    assert!(
        found
            .iter()
            .any(|row| row.id == ada && row.handle.as_deref() == Some("ada"))
    );
    assert!(
        found
            .iter()
            .any(|row| row.id == scout && row.handle.as_deref() == Some("scout"))
    );
    assert!(found.iter().all(|row| row.id != missing));
    assert!(db.store().principals(&[]).await.unwrap().is_empty());
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
        singular: BTreeMap::new(),
        plural: BTreeMap::new(),
        key_attribute,
        attributes: vec![key_attribute],
        structures: None,
        defaults: BTreeMap::new(),
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
    let audit = db
        .store()
        .object_audits(&[object.id])
        .await
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(
        audit.updated_by.map(|id| id.as_uuid()),
        Some(agent.as_uuid())
    );
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
            required: false,
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

#[tokio::test]
async fn a_create_stores_a_marker_and_history_rebuilds_version_one() {
    let (db, defs, who) = setup(Tier::Light, 21).await;
    let object = objects(&defs, Tier::Light, 21, 1).pop().unwrap();
    db.store()
        .write_batch(&who, vec![ObjectOp::Create(object.clone())])
        .await
        .unwrap();

    let stored_rows = db.store().stored_changes(object.id).await.unwrap();
    let raw = stored_rows
        .iter()
        .find(|change| change.op == braian_store::ChangeOp::Create)
        .unwrap()
        .diff
        .as_ref()
        .unwrap();
    let raw_keys = raw.as_object().unwrap();
    assert!(raw_keys.contains_key("$created"), "{raw}");
    assert!(
        raw_keys.keys().all(|key| key.starts_with('$')),
        "create diff stored attribute values: {raw}"
    );

    assert_create_diff(
        db.store()
            .changes_for(object.id)
            .await
            .unwrap()
            .iter()
            .find(|change| change.op == braian_store::ChangeOp::Create)
            .unwrap()
            .diff
            .as_ref()
            .unwrap(),
        &object,
    );
    let history = db
        .store()
        .history(Some(object.id.as_uuid()), None, 10)
        .await
        .unwrap();
    assert_create_diff(
        history
            .iter()
            .find(|entry| entry.op == "create")
            .unwrap()
            .diff
            .as_ref()
            .unwrap(),
        &object,
    );

    let edit = edits(&defs, Tier::Light, 21, EditKind::OneLanguage, 0..1)
        .next()
        .unwrap();
    db.store()
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

    let version_one = db.store().values_at_version(object.id, 1).await.unwrap();
    assert_eq!(version_one, object.values);
    let current = db.store().get_object(object.id).await.unwrap().unwrap();
    assert_ne!(current.object.values, object.values);
    assert_eq!(current.version, 2);

    assert_create_diff(
        db.store()
            .changes_for(object.id)
            .await
            .unwrap()
            .iter()
            .find(|change| change.op == braian_store::ChangeOp::Create)
            .unwrap()
            .diff
            .as_ref()
            .unwrap(),
        &object,
    );
    let stored_again = db.store().stored_changes(object.id).await.unwrap();
    let raw_again = stored_again
        .iter()
        .find(|change| change.op == braian_store::ChangeOp::Create)
        .unwrap()
        .diff
        .as_ref()
        .unwrap();
    assert_eq!(raw_again, raw);
}

#[tokio::test]
async fn batch_read_pipelines_and_can_project_parts() {
    let (db, defs, who) = setup(Tier::Light, 23).await;
    let sample = objects(&defs, Tier::Light, 23, 4);
    db.store()
        .write_batch(&who, sample.iter().cloned().map(ObjectOp::Create).collect())
        .await
        .unwrap();
    let ids: Vec<_> = sample.iter().map(|object| object.id).collect();
    let loaded = db.store().get_objects(&ids).await.unwrap();
    assert_eq!(loaded.len(), sample.len());
    for (stored, object) in loaded.iter().zip(&sample) {
        assert_eq!(stored.object, *object);
    }

    let head = sample[0].id;
    let identity = db
        .store()
        .get_objects_projected(
            &[head],
            &ObjectProjection {
                groups: false,
                values: false,
                parts: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(identity.len(), 1);
    assert_eq!(identity[0].key, loaded[0].key);
    assert!(identity[0].object.values.is_empty());
    assert!(identity[0].object.groups.is_empty());

    let parts = db.store().stored_parts(head).await.unwrap();
    let part = parts.keys().next().cloned().unwrap();
    let projected = db
        .store()
        .get_objects_projected(
            &[head],
            &ObjectProjection {
                groups: true,
                values: true,
                parts: Some(vec![part.clone()]),
            },
        )
        .await
        .unwrap();
    let only = &projected[0];
    assert_eq!(only.object.groups, loaded[0].object.groups);
    assert!(!only.object.values.is_empty());
    assert!(only.object.values.len() < loaded[0].object.values.len());
    for (id, value) in &only.object.values {
        assert_eq!(loaded[0].object.values.get(id), Some(value));
    }

    let plans = explain_batch_read(
        &db.database_url(),
        &[head.as_uuid(), sample[1].id.as_uuid()],
    )
    .await;
    assert!(
        plans
            .iter()
            .any(|plan| plan.contains("Index") || plan.contains("Bitmap")),
        "batch read did not use an index:\n{}",
        plans.join("\n---\n")
    );
}

#[tokio::test]
async fn latest_value_read_uses_the_current_index_without_sorting() {
    let (db, defs, who) = setup(Tier::Light, 21).await;
    let object = objects(&defs, Tier::Light, 21, 1)
        .into_iter()
        .next()
        .unwrap();
    db.store()
        .write_batch(&who, vec![ObjectOp::Create(object.clone())])
        .await
        .unwrap();
    let edit = edits(&defs, Tier::Light, 21, EditKind::OneLanguage, 0..1)
        .next()
        .unwrap();
    db.store()
        .write_batch(
            &who,
            vec![ObjectOp::Update {
                id: object.id,
                patch: braian_model::ObjectPatch {
                    values: edit.patch,
                    groups: BTreeMap::new(),
                },
                base_version: 1,
                read_version: 1,
            }],
        )
        .await
        .unwrap();
    let current = db.store().get_object(object.id).await.unwrap().unwrap();
    assert_eq!(current.version, 2);
    assert_ne!(current.object.values, object.values);
    let seq = db.store().latest_seq(object.id).await.unwrap();
    assert!(seq > 0);

    let (client, connection) = tokio_postgres::connect(&db.database_url(), tokio_postgres::NoTls)
        .await
        .unwrap();
    tokio::spawn(async move {
        let _ = connection.await;
    });
    client
        .batch_execute("SET enable_seqscan = off")
        .await
        .unwrap();
    let counts = client
        .query_one(
            "SELECT count(*) FILTER (WHERE is_latest), count(*)
             FROM object_values WHERE object_id = $1",
            &[&object.id.as_uuid()],
        )
        .await
        .unwrap();
    let latest: i64 = counts.get(0);
    let all: i64 = counts.get(1);
    assert!(all > latest, "an edit should keep the previous version");
    assert!(latest > 0);
    let rows = client
        .query(
            "EXPLAIN SELECT part, \"values\" FROM object_values WHERE object_id = $1 AND is_latest",
            &[&object.id.as_uuid()],
        )
        .await
        .unwrap();
    let plan = rows
        .iter()
        .map(|row| row.get::<_, String>(0))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        plan.contains("object_values_current"),
        "latest read did not use the current-row index:\n{plan}"
    );
    assert!(
        !plan.to_lowercase().contains("sort"),
        "latest read sorted version rows:\n{plan}"
    );
}

#[tokio::test]
async fn a_full_bulk_pool_does_not_block_an_interactive_connection() {
    let db = TestDb::create().await;
    let bulk = db.store().on_bulk();
    assert_eq!(bulk.lane(), DbLane::Bulk);
    assert_eq!(db.store().lane(), DbLane::Interactive);
    assert!(bulk.pool_max() < db.store().pool_max());
    let mut held = Vec::new();
    for _ in 0..bulk.pool_max() {
        held.push(bulk.hold_connection().await.unwrap());
    }
    assert_eq!(bulk.pool_size(), bulk.pool_max());
    assert_eq!(bulk.pool_available(), 0);
    assert_eq!(
        db.store().pool_size(),
        0,
        "filling the bulk pool took interactive connections"
    );
    let started = std::time::Instant::now();
    let live = db.store().hold_connection().await.unwrap();
    assert!(started.elapsed() < std::time::Duration::from_millis(200));
    assert_eq!(db.store().pool_size(), 1);
    assert_eq!(bulk.pool_size(), bulk.pool_max());
    drop(live);
    drop(held);
}

#[tokio::test]
async fn a_full_bulk_pool_does_not_block_the_indexer_lane() {
    let db = TestDb::create().await;
    let bulk = db.store().on_bulk();
    let indexer = db.store().on_indexer();
    assert_eq!(indexer.lane(), DbLane::Indexer);
    assert_eq!(indexer.pool_max(), 2);
    assert!(indexer.pool_max() < bulk.pool_max());
    let mut held = Vec::new();
    for _ in 0..bulk.pool_max() {
        held.push(bulk.hold_connection().await.unwrap());
    }
    assert_eq!(bulk.pool_available(), 0);
    assert_eq!(
        indexer.pool_size(),
        0,
        "the bulk pool opened indexer connections"
    );
    let started = std::time::Instant::now();
    let live = indexer.hold_connection().await.unwrap();
    assert!(
        started.elapsed() < std::time::Duration::from_millis(200),
        "the indexer lane waited on a full bulk pool"
    );
    assert_eq!(indexer.pool_size(), 1);
    assert_eq!(bulk.pool_size(), bulk.pool_max());
    assert_eq!(
        db.store().pool_size(),
        0,
        "the indexer lane took an API connection"
    );
    drop(live);
    drop(held);
}

async fn explain_batch_read(url: &str, ids: &[braian_model::Uuid]) -> Vec<String> {
    let (client, connection) = tokio_postgres::connect(url, tokio_postgres::NoTls)
        .await
        .unwrap();
    tokio::spawn(async move {
        let _ = connection.await;
    });
    client
        .batch_execute("SET enable_seqscan = off")
        .await
        .unwrap();
    let statements = [
        "SELECT id, type_id, key, version, model_revision, deleted_at FROM objects WHERE id = ANY($1)",
        "SELECT object_id, structure_id, group_id FROM object_structures WHERE object_id = ANY($1)",
        "SELECT object_id, part, \"values\" FROM object_values WHERE object_id = ANY($1) AND is_latest",
    ];
    let mut plans = Vec::new();
    for sql in statements {
        let rows = client
            .query(&format!("EXPLAIN (ANALYZE, BUFFERS) {sql}"), &[&ids])
            .await
            .unwrap();
        let plan = rows
            .iter()
            .map(|row| row.get::<_, String>(0))
            .collect::<Vec<_>>()
            .join("\n");
        plans.push(plan);
    }
    plans
}

fn assert_create_diff(diff: &serde_json::Value, object: &Object) {
    let map = diff.as_object().expect("create diff");
    assert!(map.get("$created").is_none(), "{diff}");
    for (id, value) in &object.values {
        let entry = map
            .get(&id.as_uuid().to_string())
            .unwrap_or_else(|| panic!("missing {id} in {diff}"))
            .as_object()
            .unwrap();
        assert!(entry.get("before").is_none());
        assert_eq!(entry.get("after"), Some(value));
    }
    for key in map.keys() {
        if key.starts_with('$') {
            continue;
        }
        let id = AttributeId::from_uuid(key.parse().unwrap());
        assert!(object.values.contains_key(&id), "{key}");
    }
    if object.groups.is_empty() {
        assert!(map.get("$groups").is_none());
    } else {
        assert!(map.get("$groups").is_some());
    }
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
