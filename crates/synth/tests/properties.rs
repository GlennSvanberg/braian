use openpim_rules::CompiledModel;
use openpim_synth::{Tier, definitions, objects};
use proptest::prelude::*;

proptest! {
    #![proptest_config(ProptestConfig { cases: 12, .. ProptestConfig::default() })]
    #[test]
    fn light_objects_validate(seed in any::<u64>()) {
        let defs = definitions(Tier::Light, seed);
        let model = CompiledModel::compile(&defs).unwrap();
        for object in objects(&defs, Tier::Light, seed, 2) {
            let errors = model.validate(&object);
            prop_assert!(errors.is_empty(), "{errors:?}");
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 3, .. ProptestConfig::default() })]
    #[test]
    fn medium_objects_validate(seed in any::<u64>()) {
        let defs = definitions(Tier::Medium, seed);
        let model = CompiledModel::compile(&defs).unwrap();
        let object = &objects(&defs, Tier::Medium, seed, 1)[0];
        let errors = model.validate(object);
        prop_assert!(errors.is_empty(), "{errors:?}");
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 1, .. ProptestConfig::default() })]
    #[test]
    fn heavy_objects_validate(seed in any::<u64>()) {
        let defs = definitions(Tier::Heavy, seed);
        let model = CompiledModel::compile(&defs).unwrap();
        let object = &objects(&defs, Tier::Heavy, seed, 1)[0];
        let errors = model.validate(object);
        prop_assert!(errors.is_empty(), "{errors:?}");
    }
}
