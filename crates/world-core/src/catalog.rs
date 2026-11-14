use crate::{Dimensions3, FurnitureAsset};

/// Returns the small built-in furniture catalog used by imported demo scenes.
///
/// Assets intentionally omit `model_uri`. Renderers should draw placeholder
/// boxes using these dimensions until normalized GLB files are assigned.
#[must_use]
pub fn default_furniture_catalog() -> Vec<FurnitureAsset> {
    vec![
        asset("chair-basic", "chair", 0.50, 0.55, 0.85),
        asset("desk-basic", "desk", 1.40, 0.70, 0.75),
        asset("meeting-table-basic", "meeting_table", 2.40, 1.10, 0.75),
        asset("sofa-basic", "sofa", 2.10, 0.90, 0.85),
        asset("storage-cabinet-basic", "storage_cabinet", 1.00, 0.45, 1.80),
    ]
}

fn asset(id: &str, category: &str, width_m: f64, depth_m: f64, height_m: f64) -> FurnitureAsset {
    FurnitureAsset {
        id: id.to_owned(),
        category: category.to_owned(),
        model_uri: None,
        dimensions_m: Dimensions3 {
            width_m,
            depth_m,
            height_m,
        },
    }
}
