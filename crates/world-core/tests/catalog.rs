use std::collections::HashSet;

use tflop_world_core::{
    Floor, SCENE_SCHEMA_VERSION, Scene, SceneSource, default_furniture_catalog,
};

#[test]
fn default_catalog_has_unique_valid_assets() {
    let assets = default_furniture_catalog();
    let ids = assets
        .iter()
        .map(|asset| asset.id.as_str())
        .collect::<HashSet<_>>();

    assert_eq!(assets.len(), 5);
    assert_eq!(ids.len(), assets.len());
    assert!(assets.iter().all(|asset| asset.model_uri.is_none()));

    let scene = Scene {
        schema_version: SCENE_SCHEMA_VERSION,
        id: "catalog-test".to_owned(),
        name: Some("Catalog test".to_owned()),
        source: SceneSource {
            provider: "fixture".to_owned(),
            external_id: "catalog-test".to_owned(),
        },
        assets,
        floors: vec![Floor {
            id: "floor-1".to_owned(),
            label: None,
            elevation_m: Some(0.0),
            rooms: vec![],
            objects: vec![],
        }],
    };

    assert!(scene.validate().is_ok());
}
