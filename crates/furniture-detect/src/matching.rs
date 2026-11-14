//! Picks the catalog asset closest in size to a measured box.
//!
//! What the furniture is doesn't matter, only how big it is: the measured box
//! stays the object's size, and the asset is just what gets drawn there.

use tflop_world_core::{Dimensions3, FurnitureAsset};

/// The asset whose footprint and height are closest to `size`, ignoring
/// orientation (a 0.5 × 0.55 m chair matches a 0.55 × 0.5 m box). Distance is
/// the sum of absolute log ratios per dimension, so twice too big counts the
/// same as half too small. `None` only if the catalog is empty.
#[must_use]
pub fn nearest_asset(size: Dimensions3, assets: &[FurnitureAsset]) -> Option<&FurnitureAsset> {
    let measured = sorted(size);
    assets.iter().min_by(|a, b| {
        distance(measured, sorted(a.dimensions_m))
            .total_cmp(&distance(measured, sorted(b.dimensions_m)))
    })
}

/// `[long side, short side, height]`.
fn sorted(size: Dimensions3) -> [f64; 3] {
    [
        size.width_m.max(size.depth_m),
        size.width_m.min(size.depth_m),
        size.height_m,
    ]
}

fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    const FLOOR_M: f64 = 0.01;
    a.iter()
        .zip(b)
        .map(|(a, b)| (a.max(FLOOR_M) / b.max(FLOOR_M)).ln().abs())
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tflop_world_core::default_furniture_catalog;

    fn nearest(width_m: f64, depth_m: f64, height_m: f64) -> String {
        let catalog = default_furniture_catalog();
        let size = Dimensions3 {
            width_m,
            depth_m,
            height_m,
        };
        nearest_asset(size, &catalog).unwrap().id.clone()
    }

    #[test]
    fn matches_by_size_regardless_of_orientation() {
        assert_eq!(nearest(2.0, 0.9, 0.85), "sofa-basic");
        assert_eq!(nearest(0.55, 0.5, 0.9), "chair-basic");
        assert_eq!(nearest(2.7, 1.2, 0.8), "meeting-table-basic");
        assert_eq!(nearest(1.2, 0.6, 0.75), "desk-basic");
        assert_eq!(nearest(1.0, 0.4, 1.9), "storage-cabinet-basic");
    }

    #[test]
    fn empty_catalog_matches_nothing() {
        let size = Dimensions3 {
            width_m: 1.0,
            depth_m: 1.0,
            height_m: 1.0,
        };
        assert!(nearest_asset(size, &[]).is_none());
    }
}
