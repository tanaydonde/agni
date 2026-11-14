//! What a detected box probably is, from its measured size and the type of
//! room it stands in. Used to pick a model of the same kind to draw; the
//! measured box stays the object's size whatever the label.

use serde::{Deserialize, Serialize};
use tflop_world_core::Dimensions3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FurnitureLabel {
    Bed,
    Sofa,
    /// Armchairs included.
    Chair,
    /// Dining and other large tables.
    Table,
    Desk,
    /// Cabinets, shelves, wardrobes, dressers, nightstands.
    Storage,
    Counter,
    Unknown,
}

/// `size` is the measured box (width is the long side); `room_type` is
/// Matterport's room classification, e.g. "Living Room".
#[must_use]
pub fn classify(size: Dimensions3, room_type: Option<&str>) -> FurnitureLabel {
    let room = room_type.unwrap_or_default().to_ascii_lowercase();
    let in_room = |name: &str| room.contains(name);
    let (long, short, height) = (
        size.width_m.max(size.depth_m),
        size.width_m.min(size.depth_m),
        size.height_m,
    );

    // Beds first: a headboard can make one as tall as a wardrobe.
    if in_room("bedroom") && (1.5..2.6).contains(&long) && short >= 0.9 && height <= 1.7 {
        FurnitureLabel::Bed
    } else if height >= 1.3 && long >= 0.3 {
        FurnitureLabel::Storage
    } else if in_room("kitchen") && (0.8..1.1).contains(&height) && short <= 0.8 && long >= 1.0 {
        FurnitureLabel::Counter
    } else if in_room("dining") && long >= 0.9 && short >= 0.6 && (0.6..1.3).contains(&height) {
        // Chair backs and things left on top make dining tables measure tall.
        FurnitureLabel::Table
    } else if (0.55..1.2).contains(&height) && long >= 1.4 && (0.8..1.3).contains(&short) {
        // Before big tables: outside a dining room, a long seat-height box is
        // far more often a sofa.
        FurnitureLabel::Sofa
    } else if long >= 1.6 && short >= 0.8 && (0.65..0.95).contains(&height) {
        FurnitureLabel::Table
    } else if in_room("bedroom") && long <= 0.8 && (0.4..0.8).contains(&height) {
        // Small and low beside a bed: a nightstand.
        FurnitureLabel::Storage
    } else if (0.6..1.25).contains(&height) && long <= 1.2 && short >= 0.4 {
        FurnitureLabel::Chair
    } else if (0.65..1.3).contains(&height)
        && (0.9..2.2).contains(&long)
        && (0.45..0.85).contains(&short)
    {
        // Monitors and lamps on top make desks measure tall.
        FurnitureLabel::Desk
    } else {
        FurnitureLabel::Unknown
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn label(width_m: f64, depth_m: f64, height_m: f64, room: &str) -> FurnitureLabel {
        classify(
            Dimensions3 {
                width_m,
                depth_m,
                height_m,
            },
            Some(room),
        )
    }

    /// Sizes measured from the demo scan.
    #[test]
    fn labels_the_demo_house_furniture() {
        assert_eq!(label(2.72, 1.24, 1.2, "Dining Room"), FurnitureLabel::Table);
        assert_eq!(label(1.47, 1.27, 1.0, "Dining Room"), FurnitureLabel::Table);
        assert_eq!(label(2.48, 2.08, 1.36, "Bedroom"), FurnitureLabel::Bed);
        assert_eq!(label(1.88, 1.80, 0.88, "Bedroom"), FurnitureLabel::Bed);
        assert_eq!(label(2.92, 0.92, 0.96, "Living Room"), FurnitureLabel::Sofa);
        assert_eq!(label(2.88, 0.92, 0.92, "Living Room"), FurnitureLabel::Sofa);
        assert_eq!(label(2.88, 1.0, 0.84, "Living Room"), FurnitureLabel::Sofa);
        assert_eq!(label(1.68, 1.68, 0.64, "Bedroom"), FurnitureLabel::Bed);
        assert_eq!(label(2.0, 1.4, 0.75, "Office"), FurnitureLabel::Table);
        assert_eq!(
            label(1.06, 1.04, 0.88, "Living Room"),
            FurnitureLabel::Chair
        );
        assert_eq!(
            label(0.64, 0.52, 0.68, "Dining Room"),
            FurnitureLabel::Chair
        );
        assert_eq!(label(1.40, 0.72, 1.0, "Living Room"), FurnitureLabel::Desk);
        assert_eq!(label(1.0, 0.72, 1.0, "Kitchen"), FurnitureLabel::Counter);
        assert_eq!(
            label(1.52, 0.36, 1.88, "Dining Room"),
            FurnitureLabel::Storage
        );
        assert_eq!(label(0.55, 0.45, 0.6, "Bedroom"), FurnitureLabel::Storage);
        assert_eq!(
            label(1.36, 0.72, 0.52, "Living Room"),
            FurnitureLabel::Unknown
        );
    }

    #[test]
    fn needs_no_room_type() {
        assert_eq!(
            classify(
                Dimensions3 {
                    width_m: 0.5,
                    depth_m: 0.5,
                    height_m: 0.9
                },
                None
            ),
            FurnitureLabel::Chair
        );
    }
}
