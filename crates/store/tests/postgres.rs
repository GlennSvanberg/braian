use std::collections::BTreeMap;

use openpim_model::{Object, ObjectType, TypeId, Uuid, apply_value_patch};
use openpim_rules::CompiledModel;
use openpim_store::testing::TestDb;
use openpim_store::{Actor, BatchKind, ObjectOp, PrincipalKind};
use openpim_synth::{EditKind, Tier, definitions, edits, objects, objects_iter};

fn actor(
    principal: openpim_store::PrincipalId,
    behalf: Option<openpim_store::PrincipalId>,
) -> Actor {
    Actor {
        actor: principal,
        on_behalf_of: behalf,
        kind: BatchKind::Interactive,
        source: "test".into(),
    }
}

async fn setup(tier: Tier, seed: u64) -> (TestDb, openpim_model::DefinitionSet, Actor) {
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
    other.id = openpim_model::ObjectId::from_uuid(Uuid::now_v7());
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
        id: openpim_model::ObjectId::from_uuid(Uuid::now_v7()),
        ..first.clone()
    };
    let error = db
        .store()
        .write_batch(&who, vec![ObjectOp::Create(duplicate)])
        .await
        .unwrap_err();
    assert!(error.to_string().contains("duplicate key"), "{error}");

    db.store()
        .write_batch(&who, vec![ObjectOp::Delete { id: first.id }])
        .await
        .unwrap();
    let reused = Object {
        id: openpim_model::ObjectId::from_uuid(Uuid::now_v7()),
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
                set: edit.patch.clone(),
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
        .find(|change| change.op == openpim_store::ChangeOp::Update)
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
        .write_batch(&who, vec![ObjectOp::Delete { id: object.id }])
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
        .find(|change| change.op == openpim_store::ChangeOp::Delete)
        .unwrap();
    assert!(delete.diff.is_none());
    let missing = db
        .store()
        .write_batch(&who, vec![ObjectOp::Delete { id: object.id }])
        .await
        .unwrap_err();
    assert!(missing.to_string().contains("deleted"), "{missing}");
}
