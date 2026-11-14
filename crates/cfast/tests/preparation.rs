use tflop_cfast::{
    CfastPreparationWarning, HeightInferenceSource, RoomExclusionReason, prepare_cfast_scene,
};
use tflop_world_core::{Scene, resolve_opening_connections};

const ACTUAL_MATTERPORT_SCENE: &str =
    include_str!("../../../data/prepared-scenes/zvKHR9DHNjJ.json");

#[test]
fn prepares_the_actual_matterport_building_with_an_audit_report() {
    let source: Scene =
        serde_json::from_str(ACTUAL_MATTERPORT_SCENE).expect("fixture should parse");

    let prepared = prepare_cfast_scene(&source).expect("actual scan should be recoverable");

    assert_eq!(source.floors[0].rooms.len(), 19, "source remains unchanged");
    assert_eq!(prepared.report.original_room_count, 19);
    assert_eq!(prepared.report.included_room_count, 16);
    assert_eq!(prepared.report.warnings.len(), 14);
    assert_eq!(
        prepared
            .report
            .warnings
            .iter()
            .filter(|warning| matches!(
                warning,
                CfastPreparationWarning::RoomExcluded {
                    reason: RoomExclusionReason::MissingBoundary,
                    ..
                }
            ))
            .count(),
        3
    );
    assert_eq!(
        prepared
            .report
            .warnings
            .iter()
            .filter(|warning| matches!(
                warning,
                CfastPreparationWarning::RoomHeightInferred {
                    source: HeightInferenceSource::FloorMedian,
                    ..
                }
            ))
            .count(),
        2
    );
    assert_eq!(
        prepared
            .report
            .warnings
            .iter()
            .filter(|warning| matches!(
                warning,
                CfastPreparationWarning::RoomLowerElevationInferred { .. }
            ))
            .count(),
        2
    );
    assert_eq!(
        resolve_opening_connections(&prepared.scene)
            .expect("prepared topology should resolve")
            .len(),
        41
    );
}

#[test]
fn uses_a_documented_default_when_no_room_height_is_measured() {
    let mut source: Scene =
        serde_json::from_str(ACTUAL_MATTERPORT_SCENE).expect("fixture should parse");
    for room in source.floors.iter_mut().flat_map(|floor| &mut floor.rooms) {
        if let Some(dimensions) = &mut room.dimensions {
            dimensions.height_m = None;
        }
    }

    let prepared = prepare_cfast_scene(&source).expect("default height should recover the scene");

    assert!(prepared.report.warnings.iter().any(|warning| matches!(
        warning,
        CfastPreparationWarning::RoomHeightInferred {
            height_m,
            source: HeightInferenceSource::GenericDefault,
            ..
        } if *height_m == 2.7
    )));
}
