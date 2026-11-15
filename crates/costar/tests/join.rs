//! Joins against a trimmed copy of the CoStar kit holding the two scans the
//! team uses (contacts removed).

use std::path::{Path, PathBuf};

use tflop_costar::{
    FloorMaterial, JoinError, ProfileInputs, PropertyKind, derive, find_by_model_id,
};

fn kit() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/kit")
}

const INPUTS: ProfileInputs = ProfileInputs {
    scanned_area_m2: None,
    max_occupants: 200,
};

#[test]
fn bungalow_joins_to_its_residential_listing() {
    let r = find_by_model_id(&kit(), "zvKHR9DHNjJ").unwrap();
    assert_eq!(r.kind, PropertyKind::Residential);
    assert_eq!(r.source.record_id, 235_967_170);
    assert_eq!(r.year_built, Some(1920));
    assert_eq!(r.bedrooms, Some(3));
    assert_eq!(r.stories, Some(1));
    assert_eq!(r.features.hardwood_floors, Some(true));
    assert!(!r.scan.partial);
    assert!(r.latitude.is_some() && r.longitude.is_some());
    // resoValues says no garage, costarFeatureValues says yes.
    assert_eq!(r.conflicts.len(), 1);
    assert_eq!(r.conflicts[0].label, "Garage");
}

#[test]
fn bungalow_profile_is_a_sleeping_household_in_a_balloon_frame_era_home() {
    let r = find_by_model_id(&kit(), "zvKHR9DHNjJ").unwrap();
    let p = derive(&r, INPUTS);
    assert_eq!(p.occupant_count.value, 6);
    assert_eq!(p.floor_material.value, FloorMaterial::Hardwood);
    assert!(
        p.alarm.value.same_room_seconds > 2.0,
        "sleepers react slower"
    );
    let items: Vec<_> = p.verification.iter().map(|v| v.item.as_str()).collect();
    assert!(
        items.iter().any(|i| i.contains("balloon framing")),
        "{items:?}"
    );
    assert!(items.iter().any(|i| i.contains("garage")), "{items:?}");
    assert!(items.iter().any(|i| i.contains("Fireplace")), "{items:?}");
}

#[test]
fn pinemont_scan_joins_to_the_commercial_property_and_its_tenants() {
    let r = find_by_model_id(&kit(), "xKKeATjBh2m").unwrap();
    assert_eq!(r.kind, PropertyKind::Commercial);
    assert_eq!(r.source.record_id, 1_352_679);
    assert_eq!(r.scan.description.as_deref(), Some("After School Care"));
    assert!(r.scan.partial);
    assert_eq!(r.construction_material.as_deref(), Some("WoodFrame"));
    assert_eq!(r.has_sprinklers, None, "unknown, not no");
    assert_eq!(r.has_elevators, Some(false));
    assert_eq!(r.units, Some(390));
    assert!(!r.tenants.is_empty());
}

#[test]
fn pinemont_occupants_come_from_the_room_not_the_property() {
    let r = find_by_model_id(&kit(), "xKKeATjBh2m").unwrap();
    let p = derive(
        &r,
        ProfileInputs {
            scanned_area_m2: Some(69.4),
            max_occupants: 200,
        },
    );
    // 747 sq ft / 35 sq ft per person (childcare), never the 390 units.
    assert_eq!(p.occupant_count.value, 22);
    assert!(p.occupant_context.value.contains("children"));
    let items: Vec<_> = p.verification.iter().map(|v| v.item.as_str()).collect();
    assert!(
        items.iter().any(|i| i.contains("only After School Care")),
        "{items:?}"
    );
    assert!(
        items
            .iter()
            .any(|i| i.contains("Sprinkler presence unknown")),
        "{items:?}"
    );
    assert!(items.iter().any(|i| i.contains("Stair-only")), "{items:?}");
}

#[test]
fn occupant_count_respects_the_simulation_cap() {
    let r = find_by_model_id(&kit(), "xKKeATjBh2m").unwrap();
    let p = derive(
        &r,
        ProfileInputs {
            scanned_area_m2: Some(10_000.0),
            max_occupants: 200,
        },
    );
    assert_eq!(p.occupant_count.value, 200);
}

#[test]
fn unknown_model_is_reported() {
    let error = find_by_model_id(&kit(), "notAModel00").unwrap_err();
    assert!(matches!(error, JoinError::ModelNotInKit(_)));
}
