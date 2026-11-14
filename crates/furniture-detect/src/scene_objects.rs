//! Turns detected furniture into ordinary scene objects, so the world, the
//! editor, and the fire simulation treat scanned furniture like any other.

use tflop_world_core::{FurnitureObject, Mobility, Quaternion, Scene, Transform3, Vec3};

use crate::{DetectedObject, Detection};

/// Prefix for the IDs of objects that came from the scan.
pub const SCANNED_OBJECT_PREFIX: &str = "scan-";

/// How many detected objects became scene objects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MergeSummary {
    pub merged: usize,
    /// No catalog asset, unknown floor or asset, a degenerate size, or an
    /// object with the same ID already in the scene.
    pub skipped: usize,
}

/// Adds every usable detected object to its floor's `objects`.
///
/// Each becomes `scan-<detected id>`, an instance of its nearest catalog asset
/// scaled so its box is exactly the measured box, turned by the measured yaw,
/// and standing on the floor. Objects are inserted directly rather than
/// through the world's edit rules, so pieces the scan found against a wall are
/// kept; those rules apply once someone moves them.
pub fn merge_into_scene(detection: &Detection, scene: &mut Scene) -> MergeSummary {
    let mut summary = MergeSummary::default();
    for detected in &detection.objects {
        match scene_object(detected, scene) {
            Some(object) => {
                let floor = scene
                    .floors
                    .iter_mut()
                    .find(|floor| floor.id == detected.floor_id)
                    .expect("scene_object checked the floor exists");
                floor.objects.push(object);
                summary.merged += 1;
            }
            None => summary.skipped += 1,
        }
    }
    summary
}

/// The scene object for one detected box, or `None` if it can't be placed.
#[must_use]
pub fn scene_object(detected: &DetectedObject, scene: &Scene) -> Option<FurnitureObject> {
    let floor = scene
        .floors
        .iter()
        .find(|floor| floor.id == detected.floor_id)?;
    let asset_id = detected.asset_id.as_deref()?;
    let asset = scene.assets.iter().find(|asset| asset.id == asset_id)?;
    let id = format!("{SCANNED_OBJECT_PREFIX}{}", detected.id);
    let taken = scene
        .floors
        .iter()
        .flat_map(|floor| &floor.objects)
        .any(|object| object.id == id);
    if taken {
        return None;
    }

    let scale = Vec3 {
        x: ratio(detected.size_m.width_m, asset.dimensions_m.width_m)?,
        y: ratio(detected.size_m.depth_m, asset.dimensions_m.depth_m)?,
        z: ratio(detected.size_m.height_m, asset.dimensions_m.height_m)?,
    };
    if !detected.yaw_degrees.is_finite()
        || !detected.position_m.x.is_finite()
        || !detected.position_m.y.is_finite()
    {
        return None;
    }

    Some(FurnitureObject {
        id,
        asset_id: asset.id.clone(),
        transform: Transform3 {
            position_m: Vec3 {
                x: detected.position_m.x,
                y: detected.position_m.y,
                // The world requires origins exactly on the floor surface.
                z: floor.floor_z(),
            },
            rotation: yaw_quaternion(detected.yaw_degrees),
            scale,
        },
        mobility: Mobility::Movable,
    })
}

/// Rotation about +Z by `degrees`.
#[must_use]
pub fn yaw_quaternion(degrees: f64) -> Quaternion {
    let half = degrees.to_radians() / 2.0;
    Quaternion {
        x: 0.0,
        y: 0.0,
        z: half.sin(),
        w: half.cos(),
    }
}

fn ratio(measured: f64, catalog: f64) -> Option<f64> {
    let value = measured / catalog;
    (value.is_finite() && value > 0.0).then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DETECTION_SCHEMA_VERSION, DetectParams, FurnitureLabel};
    use tflop_world_core::{
        Dimensions3, Floor, SCENE_SCHEMA_VERSION, SceneSource, default_furniture_catalog,
    };

    fn scene() -> Scene {
        Scene {
            schema_version: SCENE_SCHEMA_VERSION,
            id: "scene-1".into(),
            name: None,
            source: SceneSource {
                provider: "test".into(),
                external_id: "scene-1".into(),
            },
            assets: default_furniture_catalog(),
            floors: vec![Floor {
                id: "floor-1".into(),
                label: None,
                elevation_m: Some(0.5),
                rooms: Vec::new(),
                objects: Vec::new(),
            }],
        }
    }

    fn sofa() -> DetectedObject {
        DetectedObject {
            id: "detected-001".into(),
            floor_id: "floor-1".into(),
            room_id: None,
            room_type: None,
            label: FurnitureLabel::Sofa,
            asset_id: Some("sofa-basic".into()),
            position_m: Vec3 {
                x: 2.0,
                y: 3.0,
                // Slightly off the floor, as measured.
                z: 0.503,
            },
            yaw_degrees: 90.0,
            size_m: Dimensions3 {
                width_m: 1.89,
                depth_m: 0.99,
                height_m: 0.765,
            },
            footprint_area_m2: 1.8,
        }
    }

    fn detection(objects: Vec<DetectedObject>) -> Detection {
        Detection {
            schema_version: DETECTION_SCHEMA_VERSION,
            scene_id: "scene-1".into(),
            params: DetectParams::default(),
            objects,
        }
    }

    #[test]
    fn detected_box_becomes_scaled_upright_object_on_the_floor() {
        let scene = scene();
        let object = scene_object(&sofa(), &scene).expect("sofa is placeable");

        assert_eq!(object.id, "scan-detected-001");
        assert_eq!(object.asset_id, "sofa-basic");
        assert_eq!(object.mobility, Mobility::Movable);
        assert_eq!(object.transform.position_m.z, 0.5);
        assert!((object.transform.scale.x * 2.10 - 1.89).abs() < 1e-9);
        assert!((object.transform.scale.y * 0.90 - 0.99).abs() < 1e-9);
        assert!((object.transform.scale.z * 0.85 - 0.765).abs() < 1e-9);

        let rotation = object.transform.rotation;
        assert!(rotation.is_yaw_only());
        assert!((rotation.magnitude_squared() - 1.0).abs() < 1e-12);
        assert!((rotation.z - std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-12);
        assert!((rotation.w - std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-12);
    }

    #[test]
    fn merged_scene_still_validates() {
        let mut scene = scene();
        let summary = merge_into_scene(&detection(vec![sofa()]), &mut scene);

        assert_eq!(summary, MergeSummary { merged: 1, skipped: 0 });
        assert_eq!(scene.floors[0].objects.len(), 1);
        scene.validate().expect("merged scene is valid");
    }

    #[test]
    fn unusable_detections_are_skipped() {
        let no_asset = DetectedObject {
            asset_id: None,
            ..sofa()
        };
        let unknown_asset = DetectedObject {
            id: "detected-002".into(),
            asset_id: Some("piano".into()),
            ..sofa()
        };
        let unknown_floor = DetectedObject {
            id: "detected-003".into(),
            floor_id: "floor-9".into(),
            ..sofa()
        };
        let flat = DetectedObject {
            id: "detected-004".into(),
            size_m: Dimensions3 {
                height_m: 0.0,
                ..sofa().size_m
            },
            ..sofa()
        };
        let mut scene = scene();
        let summary = merge_into_scene(
            &detection(vec![no_asset, unknown_asset, unknown_floor, flat]),
            &mut scene,
        );

        assert_eq!(summary, MergeSummary { merged: 0, skipped: 4 });
        assert!(scene.floors[0].objects.is_empty());
    }

    #[test]
    fn merging_twice_does_not_duplicate() {
        let mut scene = scene();
        let detection = detection(vec![sofa()]);
        merge_into_scene(&detection, &mut scene);
        let again = merge_into_scene(&detection, &mut scene);

        assert_eq!(again, MergeSummary { merged: 0, skipped: 1 });
        assert_eq!(scene.floors[0].objects.len(), 1);
    }
}
