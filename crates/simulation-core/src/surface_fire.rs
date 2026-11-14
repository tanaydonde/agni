use std::collections::{BTreeMap, HashMap, HashSet};

use serde::{Deserialize, Serialize};
use tflop_world_core::{
    Dimensions3, Floor, FurnitureAsset, FurnitureObject, Scene, SceneValidationErrors, Vec3,
};
use thiserror::Error;

use crate::{GridBuildError, GridCellKind, GridCoordinate, SimulationGrid, grid::room_contains};

pub const SURFACE_FIRE_SCHEMA_VERSION: u32 = 1;
/// Maximum length of a furniture fire cell on any local axis.
pub const FURNITURE_FIRE_CELL_SIZE_M: f64 = 0.25;
const MAX_FURNITURE_FIRE_CELLS_PER_OBJECT: usize = 100_000;
/// Horizontal distance a hose stream reaches inside a room.
pub const DEFAULT_NOZZLE_REACH_M: f64 = 5.0;
/// Effective cooling of one handline: a 1.75-inch line flows about 9.5 kg/s,
/// of which a fraction actually absorbs heat from the fire.
pub const DEFAULT_NOZZLE_COOLING_KW: f64 = 1_000.0;
/// Half the spread of a nozzle's stream: a 30 degree cone.
pub const DEFAULT_NOZZLE_HALF_ANGLE_DEGREES: f64 = 15.0;
/// Burning cells this close to the nozzle are wetted whichever way it points.
const NOZZLE_SPLASH_RADIUS_M: f64 = 0.5;

/// Approximate combustion properties used by the fast Rust surface model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FireMaterialProfile {
    pub id: String,
    pub combustible: bool,
    pub ignition_temperature_c: f64,
    pub ignition_energy_kj_per_m2: f64,
    pub peak_hrr_kw_per_m2: f64,
    pub fuel_energy_kj_per_m2: f64,
    pub heat_transfer_fraction: f64,
    pub cooling_kw_per_m2: f64,
}

impl FireMaterialProfile {
    #[must_use]
    pub fn noncombustible(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            combustible: false,
            ignition_temperature_c: 0.0,
            ignition_energy_kj_per_m2: 0.0,
            peak_hrr_kw_per_m2: 0.0,
            fuel_energy_kj_per_m2: 0.0,
            heat_transfer_fraction: 0.0,
            cooling_kw_per_m2: 0.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FurnitureFireMaterial {
    pub category: String,
    pub material: FireMaterialProfile,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FireMaterialCatalog {
    pub floor: FireMaterialProfile,
    pub default_furniture: FireMaterialProfile,
    pub furniture: Vec<FurnitureFireMaterial>,
}

impl Default for FireMaterialCatalog {
    fn default() -> Self {
        Self {
            // Demo assumptions until a building supplies actual finish data.
            floor: material("generic_carpet", 290.0, 180.0, 180.0, 4_500.0, 0.28, 0.8),
            default_furniture: material(
                "generic_furniture",
                340.0,
                220.0,
                260.0,
                6_000.0,
                0.24,
                0.6,
            ),
            furniture: vec![
                furniture_material(
                    "chair",
                    material("wood_chair", 330.0, 210.0, 240.0, 5_000.0, 0.24, 0.6),
                ),
                furniture_material(
                    "desk",
                    material("wood_desk", 350.0, 260.0, 210.0, 6_000.0, 0.22, 0.6),
                ),
                furniture_material(
                    "meeting_table",
                    material("wood_table", 350.0, 280.0, 200.0, 6_500.0, 0.22, 0.6),
                ),
                furniture_material(
                    "sofa",
                    material("upholstered_sofa", 260.0, 110.0, 420.0, 8_000.0, 0.30, 0.5),
                ),
                furniture_material(
                    "storage_cabinet",
                    material("wood_cabinet", 350.0, 260.0, 230.0, 7_000.0, 0.22, 0.6),
                ),
            ],
        }
    }
}

impl FireMaterialCatalog {
    fn furniture_material(&self, category: &str) -> &FireMaterialProfile {
        self.furniture
            .iter()
            .find(|entry| entry.category == category)
            .map_or(&self.default_furniture, |entry| &entry.material)
    }

    fn validate(&self) -> Result<(), SurfaceFireError> {
        validate_material(&self.floor)?;
        validate_material(&self.default_furniture)?;
        let mut categories = HashSet::new();
        for entry in &self.furniture {
            if entry.category.trim().is_empty() {
                return Err(SurfaceFireError::InvalidMaterial {
                    material_id: entry.material.id.clone(),
                    message: "furniture category cannot be empty",
                });
            }
            if !categories.insert(entry.category.as_str()) {
                return Err(SurfaceFireError::DuplicateFurnitureCategory {
                    category: entry.category.clone(),
                });
            }
            validate_material(&entry.material)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SurfaceFireConfig {
    pub cell_size_m: f64,
    pub ambient_temperature_c: f64,
    pub furniture_radiation_range_m: f64,
    /// A burning node goes out once the suppression cooling it has received
    /// adds up to its current heat release over this many seconds, so larger
    /// fires take longer to knock down. With the default nozzle, a burning
    /// sofa takes about 20 seconds.
    #[serde(default = "default_knockdown_seconds")]
    pub suppression_knockdown_seconds: f64,
}

const fn default_knockdown_seconds() -> f64 {
    30.0
}

impl Default for SurfaceFireConfig {
    fn default() -> Self {
        Self {
            cell_size_m: 0.5,
            ambient_temperature_c: 20.0,
            furniture_radiation_range_m: 1.5,
            suppression_knockdown_seconds: default_knockdown_seconds(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SurfaceFireNodeSource {
    FloorCell {
        floor_id: String,
        coordinate: GridCoordinate,
    },
    Furniture {
        object_id: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CombustionState {
    Unignited,
    Heating,
    Burning,
    BurnedOut,
    /// Put out by suppression with fuel left. Reignites if it reheats past
    /// its ignition energy.
    Extinguished,
}

/// Identifies one cell in an object's local width/depth/height fire grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FurnitureFireCell {
    pub column: usize,
    pub row: usize,
    /// Zero is the layer touching the floor.
    pub layer: usize,
    pub columns: usize,
    pub rows: usize,
    pub layers: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SurfaceFireNode {
    pub id: String,
    pub source: SurfaceFireNodeSource,
    pub floor_id: String,
    pub room_id: String,
    pub position_m: Vec3,
    pub dimensions_m: Dimensions3,
    /// Present for furniture; absent for floor cells.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub furniture_cell: Option<FurnitureFireCell>,
    pub material: FireMaterialProfile,
    pub state: CombustionState,
    pub ignition_time_seconds: Option<f64>,
    pub absorbed_heat_kj: f64,
    pub fuel_remaining_kj: f64,
    area_m2: f64,
    /// Suppression cooling received while burning, toward knockdown.
    #[serde(default)]
    cooling_applied_kj: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SurfaceFireNodeFrame {
    pub id: String,
    pub source: SurfaceFireNodeSource,
    pub room_id: String,
    pub position_m: Vec3,
    pub dimensions_m: Dimensions3,
    /// Lets a renderer place fire on only the burning part of an object.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub furniture_cell: Option<FurnitureFireCell>,
    pub material_id: String,
    pub state: CombustionState,
    pub ignition_time_seconds: Option<f64>,
    pub surface_temperature_c: f64,
    pub heat_release_rate_kw: f64,
    pub fuel_remaining_fraction: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoomHeatRelease {
    pub room_id: String,
    pub heat_release_rate_kw: f64,
}

/// Renderer-facing state for one instant. Unheated nodes are omitted so floor
/// grids do not make every frame unnecessarily large.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SurfaceFireFrame {
    pub schema_version: u32,
    pub scene_id: String,
    pub time_seconds: f64,
    pub active_nodes: Vec<SurfaceFireNodeFrame>,
    pub room_heat_release: Vec<RoomHeatRelease>,
}

/// Water from one nozzle over one step.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Suppression {
    /// Only nodes in this room are reached; walls block the stream.
    pub room_id: String,
    pub position_m: Vec3,
    /// Horizontal distance from `position_m` that the stream reaches.
    pub reach_m: f64,
    pub cooling_kw: f64,
    /// Unit vector the nozzle points along. `None` wets everything in reach,
    /// all around.
    #[serde(default)]
    pub direction: Option<(f64, f64)>,
    /// Half the spread of the stream around `direction`.
    #[serde(default = "default_half_angle")]
    pub half_angle_degrees: f64,
}

const fn default_half_angle() -> f64 {
    DEFAULT_NOZZLE_HALF_ANGLE_DEGREES
}

impl Suppression {
    pub fn validate(&self) -> Result<(), SurfaceFireError> {
        let field = if self.room_id.trim().is_empty() {
            Some("room_id")
        } else if !self.position_m.x.is_finite() || !self.position_m.y.is_finite() {
            Some("position_m")
        } else if !self.reach_m.is_finite() || self.reach_m <= 0.0 {
            Some("reach_m")
        } else if !self.cooling_kw.is_finite() || self.cooling_kw < 0.0 {
            Some("cooling_kw")
        } else if self.direction.is_some_and(|(x, y)| {
            !x.is_finite() || !y.is_finite() || ((x * x + y * y).sqrt() - 1.0).abs() > 1.0e-6
        }) {
            Some("direction")
        } else if !self.half_angle_degrees.is_finite()
            || self.half_angle_degrees <= 0.0
            || self.half_angle_degrees > 180.0
        {
            Some("half_angle_degrees")
        } else {
            None
        };
        field.map_or(Ok(()), |field| {
            Err(SurfaceFireError::InvalidSuppression { field })
        })
    }
}

/// What one suppression call did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SuppressionOutcome {
    /// Burning nodes the stream reached, including ones it put out.
    pub burning_reached: usize,
    pub extinguished_node_ids: Vec<String>,
}

#[derive(Debug)]
pub struct SurfaceFireWorld {
    scene_id: String,
    config: SurfaceFireConfig,
    time_seconds: f64,
    nodes: Vec<SurfaceFireNode>,
    node_indices: HashMap<String, usize>,
    connections: Vec<Vec<HeatConnection>>,
}

#[derive(Debug, Clone, Copy)]
struct HeatConnection {
    neighbor: usize,
    weight: f64,
}

impl SurfaceFireWorld {
    pub fn from_scene(
        scene: &Scene,
        materials: &FireMaterialCatalog,
        config: SurfaceFireConfig,
    ) -> Result<Self, SurfaceFireError> {
        scene.validate()?;
        materials.validate()?;
        validate_config(config)?;

        let grids = scene
            .floors
            .iter()
            .map(|floor| {
                SimulationGrid::from_floor(floor, &scene.assets, config.cell_size_m)
                    .map(|grid| (floor, grid))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut nodes = Vec::new();
        let mut floor_nodes = HashMap::new();
        let mut furniture_nodes = HashMap::<String, Vec<usize>>::new();

        if materials.floor.combustible {
            for (floor, grid) in &grids {
                for cell in grid.cells().iter().filter(|cell| {
                    matches!(cell.kind, GridCellKind::Floor | GridCellKind::Furniture)
                        && !cell.room_ids.is_empty()
                }) {
                    let room_id = cell.room_ids[0].clone();
                    let id = floor_node_id(&floor.id, cell.coordinate);
                    let area_m2 = config.cell_size_m * config.cell_size_m;
                    let index = nodes.len();
                    nodes.push(new_node(NewNode {
                        id,
                        source: SurfaceFireNodeSource::FloorCell {
                            floor_id: floor.id.clone(),
                            coordinate: cell.coordinate,
                        },
                        floor_id: floor.id.clone(),
                        room_id,
                        position_m: cell.center_m,
                        dimensions_m: Dimensions3 {
                            width_m: config.cell_size_m,
                            depth_m: config.cell_size_m,
                            height_m: 0.02,
                        },
                        furniture_cell: None,
                        area_m2,
                        material: materials.floor.clone(),
                    }));
                    floor_nodes.insert((floor.id.clone(), cell.coordinate), index);
                }
            }
        }

        for (floor, grid) in &grids {
            for object in &floor.objects {
                let asset = find_asset(&scene.assets, object)?;
                let material = materials.furniture_material(&asset.category);
                if !material.combustible {
                    continue;
                }
                let room_id = object_room_id(grid, floor, object)?;
                let dimensions_m = scaled_dimensions(asset, object);
                let (columns, rows, layers) = furniture_cell_counts(&object.id, dimensions_m)?;
                let cell_width_m = dimensions_m.width_m / columns as f64;
                let cell_depth_m = dimensions_m.depth_m / rows as f64;
                let cell_height_m = dimensions_m.height_m / layers as f64;
                let total_area_m2 = dimensions_m.width_m * dimensions_m.depth_m;
                let cell_count = columns * rows * layers;
                let (yaw_sine, yaw_cosine) = furniture_yaw_sin_cos(object);
                let mut cell_indices = Vec::with_capacity(cell_count);
                for layer in 0..layers {
                    for row in 0..rows {
                        for column in 0..columns {
                            let furniture_cell = FurnitureFireCell {
                                column,
                                row,
                                layer,
                                columns,
                                rows,
                                layers,
                            };
                            let local_x =
                                -dimensions_m.width_m / 2.0 + (column as f64 + 0.5) * cell_width_m;
                            let local_y =
                                -dimensions_m.depth_m / 2.0 + (row as f64 + 0.5) * cell_depth_m;
                            let index = nodes.len();
                            nodes.push(new_node(NewNode {
                                id: object_cell_node_id(&object.id, furniture_cell),
                                source: SurfaceFireNodeSource::Furniture {
                                    object_id: object.id.clone(),
                                },
                                floor_id: floor.id.clone(),
                                room_id: room_id.clone(),
                                position_m: Vec3 {
                                    x: object.transform.position_m.x + local_x * yaw_cosine
                                        - local_y * yaw_sine,
                                    y: object.transform.position_m.y
                                        + local_x * yaw_sine
                                        + local_y * yaw_cosine,
                                    z: object.transform.position_m.z
                                        + (layer as f64 + 0.5) * cell_height_m,
                                },
                                dimensions_m: Dimensions3 {
                                    width_m: cell_width_m,
                                    depth_m: cell_depth_m,
                                    height_m: cell_height_m,
                                },
                                furniture_cell: Some(furniture_cell),
                                // Preserve the object's previous total fuel and peak HRR
                                // while distributing them across independent cells.
                                area_m2: total_area_m2 / cell_count as f64,
                                material: material.clone(),
                            }));
                            cell_indices.push(index);
                        }
                    }
                }
                furniture_nodes.insert(object.id.clone(), cell_indices);
            }
        }

        let mut connections = vec![Vec::new(); nodes.len()];
        let mut connected_pairs = HashSet::new();
        connect_floor_neighbors(&grids, &floor_nodes, &mut connections, &mut connected_pairs);
        connect_furniture_to_floor(
            &grids,
            &nodes,
            &floor_nodes,
            &furniture_nodes,
            &mut connections,
            &mut connected_pairs,
        );
        connect_furniture_cells(
            &nodes,
            &furniture_nodes,
            &mut connections,
            &mut connected_pairs,
        );
        connect_nearby_furniture(
            &nodes,
            &furniture_nodes,
            config.furniture_radiation_range_m,
            &mut connections,
            &mut connected_pairs,
        );

        let node_indices = nodes
            .iter()
            .enumerate()
            .map(|(index, node)| (node.id.clone(), index))
            .collect();
        Ok(Self {
            scene_id: scene.id.clone(),
            config,
            time_seconds: 0.0,
            nodes,
            node_indices,
            connections,
        })
    }

    #[must_use]
    pub fn nodes(&self) -> &[SurfaceFireNode] {
        &self.nodes
    }

    #[must_use]
    pub fn node(&self, node_id: &str) -> Option<&SurfaceFireNode> {
        self.node_indices
            .get(node_id)
            .and_then(|index| self.nodes.get(*index))
    }

    #[must_use]
    pub fn connection_count(&self) -> usize {
        self.connections.iter().map(Vec::len).sum::<usize>() / 2
    }

    pub fn ignite_node(&mut self, node_id: &str) -> Result<SurfaceFireFrame, SurfaceFireError> {
        let index =
            *self
                .node_indices
                .get(node_id)
                .ok_or_else(|| SurfaceFireError::NodeNotFound {
                    node_id: node_id.to_owned(),
                })?;
        let node = &mut self.nodes[index];
        if matches!(
            node.state,
            CombustionState::Burning | CombustionState::BurnedOut
        ) {
            return Err(SurfaceFireError::NodeCannotIgnite {
                node_id: node_id.to_owned(),
                state: node.state,
            });
        }
        node.state = CombustionState::Burning;
        node.ignition_time_seconds = Some(self.time_seconds);
        node.absorbed_heat_kj = ignition_energy(node);
        Ok(self.snapshot())
    }

    pub fn ignite_object(&mut self, object_id: &str) -> Result<SurfaceFireFrame, SurfaceFireError> {
        self.ignite_node(&object_node_id(object_id))
    }

    /// Ignites one exact cell in a furniture object's local grid.
    pub fn ignite_object_cell(
        &mut self,
        object_id: &str,
        column: usize,
        row: usize,
        layer: usize,
    ) -> Result<SurfaceFireFrame, SurfaceFireError> {
        let node_id = self
            .nodes
            .iter()
            .find(|node| {
                matches!(
                    &node.source,
                    SurfaceFireNodeSource::Furniture { object_id: source_id }
                        if source_id == object_id
                ) && matches!(
                    node.furniture_cell,
                    Some(cell)
                        if cell.column == column && cell.row == row && cell.layer == layer
                )
            })
            .map(|node| node.id.clone())
            .ok_or_else(|| SurfaceFireError::FurnitureCellNotFound {
                object_id: object_id.to_owned(),
                column,
                row,
                layer,
            })?;
        self.ignite_node(&node_id)
    }

    pub fn ignite_floor_cell(
        &mut self,
        floor_id: &str,
        coordinate: GridCoordinate,
    ) -> Result<SurfaceFireFrame, SurfaceFireError> {
        self.ignite_node(&floor_node_id(floor_id, coordinate))
    }

    /// Applies `delta_seconds` of water without advancing time; call it before
    /// [`step`](Self::step) for the same interval.
    ///
    /// The stream is aimed at flames: its cooling is shared evenly among the
    /// burning nodes in reach, and a node goes out once its share adds up to
    /// its heat release over the knockdown time. With no flames in reach, it
    /// cools heating and extinguished nodes instead, which delays reignition.
    pub fn suppress(
        &mut self,
        suppression: &Suppression,
        delta_seconds: f64,
    ) -> Result<SuppressionOutcome, SurfaceFireError> {
        if !delta_seconds.is_finite() || delta_seconds <= 0.0 {
            return Err(SurfaceFireError::InvalidDeltaSeconds { delta_seconds });
        }
        suppression.validate()?;

        let cos_half_angle = suppression.half_angle_degrees.to_radians().cos();
        let in_reach = |node: &SurfaceFireNode| {
            let offset = (
                node.position_m.x - suppression.position_m.x,
                node.position_m.y - suppression.position_m.y,
            );
            let distance = offset.0.hypot(offset.1);
            let in_stream = match suppression.direction {
                None => true,
                Some((x, y)) => {
                    distance <= NOZZLE_SPLASH_RADIUS_M
                        || (offset.0 * x + offset.1 * y) / distance >= cos_half_angle
                }
            };
            node.room_id == suppression.room_id && distance <= suppression.reach_m && in_stream
        };
        let burning = self
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| node.state == CombustionState::Burning && in_reach(node))
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        let cooling_kj = suppression.cooling_kw * delta_seconds;
        let mut outcome = SuppressionOutcome {
            burning_reached: burning.len(),
            extinguished_node_ids: Vec::new(),
        };

        if !burning.is_empty() {
            let share_kj = cooling_kj / burning.len() as f64;
            let knockdown_seconds = self.config.suppression_knockdown_seconds;
            for index in burning {
                let node = &mut self.nodes[index];
                node.cooling_applied_kj += share_kj;
                if node.cooling_applied_kj >= current_hrr_kw(node) * knockdown_seconds {
                    node.state = CombustionState::Extinguished;
                    node.absorbed_heat_kj = 0.0;
                    node.cooling_applied_kj = 0.0;
                    outcome.extinguished_node_ids.push(node.id.clone());
                }
            }
            return Ok(outcome);
        }

        let warm = self
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| {
                matches!(
                    node.state,
                    CombustionState::Heating | CombustionState::Extinguished
                ) && in_reach(node)
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        if !warm.is_empty() {
            let share_kj = cooling_kj / warm.len() as f64;
            for index in warm {
                let node = &mut self.nodes[index];
                node.absorbed_heat_kj = (node.absorbed_heat_kj - share_kj).max(0.0);
                if node.state == CombustionState::Heating && node.absorbed_heat_kj == 0.0 {
                    node.state = CombustionState::Unignited;
                }
            }
        }
        Ok(outcome)
    }

    pub fn step(&mut self, delta_seconds: f64) -> Result<SurfaceFireFrame, SurfaceFireError> {
        if !delta_seconds.is_finite() || delta_seconds <= 0.0 {
            return Err(SurfaceFireError::InvalidDeltaSeconds { delta_seconds });
        }
        let mut incoming_heat_kw = vec![0.0; self.nodes.len()];
        for (index, node) in self.nodes.iter().enumerate() {
            if node.state != CombustionState::Burning || self.connections[index].is_empty() {
                continue;
            }
            let weight_sum = self.connections[index]
                .iter()
                .map(|connection| connection.weight)
                .sum::<f64>();
            let transferable_kw = current_hrr_kw(node) * node.material.heat_transfer_fraction;
            for connection in &self.connections[index] {
                incoming_heat_kw[connection.neighbor] +=
                    transferable_kw * connection.weight / weight_sum;
            }
        }

        let next_time = self.time_seconds + delta_seconds;
        for (index, node) in self.nodes.iter_mut().enumerate() {
            match node.state {
                CombustionState::Burning => {
                    node.fuel_remaining_kj =
                        (node.fuel_remaining_kj - current_hrr_kw(node) * delta_seconds).max(0.0);
                    if node.fuel_remaining_kj == 0.0 {
                        node.state = CombustionState::BurnedOut;
                    }
                }
                CombustionState::Unignited | CombustionState::Heating => {
                    let cooling_kw = node.material.cooling_kw_per_m2 * node.area_m2;
                    let net_heat_kw = incoming_heat_kw[index] - cooling_kw;
                    node.absorbed_heat_kj =
                        (node.absorbed_heat_kj + net_heat_kw * delta_seconds).max(0.0);
                    if node.absorbed_heat_kj >= ignition_energy(node) {
                        node.state = CombustionState::Burning;
                        node.ignition_time_seconds = Some(next_time);
                    } else if node.absorbed_heat_kj > 0.0 {
                        node.state = CombustionState::Heating;
                    } else {
                        node.state = CombustionState::Unignited;
                    }
                }
                CombustionState::Extinguished => {
                    let cooling_kw = node.material.cooling_kw_per_m2 * node.area_m2;
                    let net_heat_kw = incoming_heat_kw[index] - cooling_kw;
                    node.absorbed_heat_kj =
                        (node.absorbed_heat_kj + net_heat_kw * delta_seconds).max(0.0);
                    if node.absorbed_heat_kj >= ignition_energy(node) {
                        node.state = CombustionState::Burning;
                        node.ignition_time_seconds = Some(next_time);
                    }
                }
                CombustionState::BurnedOut => {}
            }
        }
        self.time_seconds = next_time;
        Ok(self.snapshot())
    }

    #[must_use]
    pub const fn time_seconds(&self) -> f64 {
        self.time_seconds
    }

    #[must_use]
    pub fn snapshot(&self) -> SurfaceFireFrame {
        let active_nodes = self
            .nodes
            .iter()
            .filter(|node| node.state != CombustionState::Unignited)
            .map(|node| node_frame(node, self.config.ambient_temperature_c))
            .collect::<Vec<_>>();
        let mut room_hrr = BTreeMap::<&str, f64>::new();
        for node in &self.nodes {
            *room_hrr.entry(&node.room_id).or_default() += current_hrr_kw(node);
        }
        SurfaceFireFrame {
            schema_version: SURFACE_FIRE_SCHEMA_VERSION,
            scene_id: self.scene_id.clone(),
            time_seconds: self.time_seconds,
            active_nodes,
            room_heat_release: room_hrr
                .into_iter()
                .map(|(room_id, heat_release_rate_kw)| RoomHeatRelease {
                    room_id: room_id.to_owned(),
                    heat_release_rate_kw,
                })
                .collect(),
        }
    }
}

#[derive(Debug, Error)]
pub enum SurfaceFireError {
    #[error(transparent)]
    InvalidScene(#[from] SceneValidationErrors),
    #[error(transparent)]
    Grid(#[from] GridBuildError),
    #[error("invalid surface-fire config field {field}")]
    InvalidConfig { field: &'static str },
    #[error("invalid fire material {material_id}: {message}")]
    InvalidMaterial {
        material_id: String,
        message: &'static str,
    },
    #[error("furniture fire category {category} appears more than once")]
    DuplicateFurnitureCategory { category: String },
    #[error("object {object_id} references missing asset {asset_id}")]
    AssetNotFound { object_id: String, asset_id: String },
    #[error("object {object_id} is not located in a room grid cell")]
    ObjectOutsideGrid { object_id: String },
    #[error("object {object_id} fire grid would contain {cell_count} cells, exceeding {maximum}")]
    FurnitureGridTooLarge {
        object_id: String,
        cell_count: usize,
        maximum: usize,
    },
    #[error("object {object_id} has no fire cell at ({column}, {row}, {layer})")]
    FurnitureCellNotFound {
        object_id: String,
        column: usize,
        row: usize,
        layer: usize,
    },
    #[error("surface-fire node {node_id} was not found")]
    NodeNotFound { node_id: String },
    #[error("surface-fire node {node_id} cannot ignite from state {state:?}")]
    NodeCannotIgnite {
        node_id: String,
        state: CombustionState,
    },
    #[error("delta_seconds must be finite and greater than zero, got {delta_seconds}")]
    InvalidDeltaSeconds { delta_seconds: f64 },
    #[error("invalid suppression field {field}")]
    InvalidSuppression { field: &'static str },
}

struct NewNode {
    id: String,
    source: SurfaceFireNodeSource,
    floor_id: String,
    room_id: String,
    position_m: Vec3,
    dimensions_m: Dimensions3,
    furniture_cell: Option<FurnitureFireCell>,
    area_m2: f64,
    material: FireMaterialProfile,
}

fn new_node(input: NewNode) -> SurfaceFireNode {
    let fuel_remaining_kj = input.material.fuel_energy_kj_per_m2 * input.area_m2;
    SurfaceFireNode {
        id: input.id,
        source: input.source,
        floor_id: input.floor_id,
        room_id: input.room_id,
        position_m: input.position_m,
        dimensions_m: input.dimensions_m,
        furniture_cell: input.furniture_cell,
        material: input.material,
        state: CombustionState::Unignited,
        ignition_time_seconds: None,
        absorbed_heat_kj: 0.0,
        fuel_remaining_kj,
        area_m2: input.area_m2,
        cooling_applied_kj: 0.0,
    }
}

fn connect_floor_neighbors(
    grids: &[(&tflop_world_core::Floor, SimulationGrid)],
    floor_nodes: &HashMap<(String, GridCoordinate), usize>,
    connections: &mut [Vec<HeatConnection>],
    connected_pairs: &mut HashSet<(usize, usize)>,
) {
    for (floor, grid) in grids {
        for cell in grid.cells() {
            let Some(&from) = floor_nodes.get(&(floor.id.clone(), cell.coordinate)) else {
                continue;
            };
            for coordinate in [
                GridCoordinate {
                    column: cell.coordinate.column.saturating_add(1),
                    row: cell.coordinate.row,
                },
                GridCoordinate {
                    column: cell.coordinate.column,
                    row: cell.coordinate.row.saturating_add(1),
                },
            ] {
                if let Some(&to) = floor_nodes.get(&(floor.id.clone(), coordinate)) {
                    connect(connections, connected_pairs, from, to, 1.0);
                }
            }
        }
    }
}

fn connect_furniture_to_floor(
    grids: &[(&tflop_world_core::Floor, SimulationGrid)],
    nodes: &[SurfaceFireNode],
    floor_nodes: &HashMap<(String, GridCoordinate), usize>,
    furniture_nodes: &HashMap<String, Vec<usize>>,
    connections: &mut [Vec<HeatConnection>],
    connected_pairs: &mut HashSet<(usize, usize)>,
) {
    for indices in furniture_nodes.values() {
        for &object_node in indices {
            let node = &nodes[object_node];
            if node.furniture_cell.is_none_or(|cell| cell.layer != 0) {
                continue;
            }
            let Some((floor, grid)) = grids.iter().find(|(floor, _)| floor.id == node.floor_id)
            else {
                continue;
            };
            let Some(coordinate) = grid.world_to_cell(node.position_m) else {
                continue;
            };
            if let Some(&floor_node) = floor_nodes.get(&(floor.id.clone(), coordinate)) {
                connect(connections, connected_pairs, floor_node, object_node, 1.5);
            }
        }
    }
}

fn connect_furniture_cells(
    nodes: &[SurfaceFireNode],
    furniture_nodes: &HashMap<String, Vec<usize>>,
    connections: &mut [Vec<HeatConnection>],
    connected_pairs: &mut HashSet<(usize, usize)>,
) {
    for indices in furniture_nodes.values() {
        let by_coordinate = indices
            .iter()
            .filter_map(|&index| {
                nodes[index]
                    .furniture_cell
                    .map(|cell| ((cell.column, cell.row, cell.layer), index))
            })
            .collect::<HashMap<_, _>>();
        for (&(column, row, layer), &index) in &by_coordinate {
            for neighbor_coordinate in [
                (column.saturating_add(1), row, layer),
                (column, row.saturating_add(1), layer),
            ] {
                if let Some(&neighbor) = by_coordinate.get(&neighbor_coordinate) {
                    connect(connections, connected_pairs, index, neighbor, 1.0);
                }
            }
            if let Some(&upper) = by_coordinate.get(&(column, row, layer.saturating_add(1))) {
                let key = (index.min(upper), index.max(upper));
                if connected_pairs.insert(key) {
                    // Hot gases and flame rise, so upward transfer is deliberately
                    // stronger than heat returning to the cell below.
                    connections[index].push(HeatConnection {
                        neighbor: upper,
                        weight: 3.0,
                    });
                    connections[upper].push(HeatConnection {
                        neighbor: index,
                        weight: 0.5,
                    });
                }
            }
        }
    }
}

fn connect_nearby_furniture(
    nodes: &[SurfaceFireNode],
    furniture_nodes: &HashMap<String, Vec<usize>>,
    range_m: f64,
    connections: &mut [Vec<HeatConnection>],
    connected_pairs: &mut HashSet<(usize, usize)>,
) {
    let objects = furniture_nodes.values().collect::<Vec<_>>();
    for (position, left_cells) in objects.iter().enumerate() {
        for right_cells in &objects[position + 1..] {
            let Some((&left_first, &right_first)) = left_cells.first().zip(right_cells.first())
            else {
                continue;
            };
            if nodes[left_first].floor_id != nodes[right_first].floor_id
                || nodes[left_first].room_id != nodes[right_first].room_id
            {
                continue;
            }
            connect_each_surface_cell_to_nearest(
                nodes,
                left_cells,
                right_cells,
                range_m,
                connections,
                connected_pairs,
            );
            connect_each_surface_cell_to_nearest(
                nodes,
                right_cells,
                left_cells,
                range_m,
                connections,
                connected_pairs,
            );
        }
    }
}

fn connect_each_surface_cell_to_nearest(
    nodes: &[SurfaceFireNode],
    sources: &[usize],
    targets: &[usize],
    range_m: f64,
    connections: &mut [Vec<HeatConnection>],
    connected_pairs: &mut HashSet<(usize, usize)>,
) {
    for &source in sources
        .iter()
        .filter(|&&index| is_exposed_furniture_cell(&nodes[index]))
    {
        let nearest = targets
            .iter()
            .filter(|&&index| is_exposed_furniture_cell(&nodes[index]))
            .map(|&target| (target, node_gap_m(&nodes[source], &nodes[target])))
            .min_by(|left, right| left.1.total_cmp(&right.1));
        if let Some((target, gap)) = nearest
            && gap <= range_m
        {
            connect(
                connections,
                connected_pairs,
                source,
                target,
                1.0 / (1.0 + gap),
            );
        }
    }
}

fn is_exposed_furniture_cell(node: &SurfaceFireNode) -> bool {
    node.furniture_cell.is_some_and(|cell| {
        cell.column == 0
            || cell.column + 1 == cell.columns
            || cell.row == 0
            || cell.row + 1 == cell.rows
            || cell.layer == 0
            || cell.layer + 1 == cell.layers
    })
}

fn node_gap_m(left: &SurfaceFireNode, right: &SurfaceFireNode) -> f64 {
    let horizontal_center_distance =
        (left.position_m.x - right.position_m.x).hypot(left.position_m.y - right.position_m.y);
    let left_radius = left.dimensions_m.width_m.hypot(left.dimensions_m.depth_m) / 2.0;
    let right_radius = right.dimensions_m.width_m.hypot(right.dimensions_m.depth_m) / 2.0;
    // Bounding radii keep this proximity approximation valid for yaw rotation.
    let horizontal_gap = (horizontal_center_distance - left_radius - right_radius).max(0.0);
    let z_gap = ((left.position_m.z - right.position_m.z).abs()
        - (left.dimensions_m.height_m + right.dimensions_m.height_m) / 2.0)
        .max(0.0);
    horizontal_gap.hypot(z_gap)
}

fn connect(
    connections: &mut [Vec<HeatConnection>],
    connected_pairs: &mut HashSet<(usize, usize)>,
    left: usize,
    right: usize,
    weight: f64,
) {
    if left == right {
        return;
    }
    let pair = (left.min(right), left.max(right));
    if connected_pairs.insert(pair) {
        connections[left].push(HeatConnection {
            neighbor: right,
            weight,
        });
        connections[right].push(HeatConnection {
            neighbor: left,
            weight,
        });
    }
}

fn node_frame(node: &SurfaceFireNode, ambient_temperature_c: f64) -> SurfaceFireNodeFrame {
    let ignition_energy_kj = ignition_energy(node);
    let heat_fraction = if ignition_energy_kj > 0.0 {
        (node.absorbed_heat_kj / ignition_energy_kj).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let hrr_kw = current_hrr_kw(node);
    let peak_hrr_kw = node.material.peak_hrr_kw_per_m2 * node.area_m2;
    let intensity = if peak_hrr_kw > 0.0 {
        hrr_kw / peak_hrr_kw
    } else {
        0.0
    };
    let surface_temperature_c = match node.state {
        CombustionState::Burning => node.material.ignition_temperature_c + 400.0 * intensity,
        CombustionState::BurnedOut => ambient_temperature_c,
        CombustionState::Heating | CombustionState::Unignited | CombustionState::Extinguished => {
            ambient_temperature_c
                + (node.material.ignition_temperature_c - ambient_temperature_c) * heat_fraction
        }
    };
    let initial_fuel_kj = node.material.fuel_energy_kj_per_m2 * node.area_m2;
    SurfaceFireNodeFrame {
        id: node.id.clone(),
        source: node.source.clone(),
        room_id: node.room_id.clone(),
        position_m: node.position_m,
        dimensions_m: node.dimensions_m,
        furniture_cell: node.furniture_cell,
        material_id: node.material.id.clone(),
        state: node.state,
        ignition_time_seconds: node.ignition_time_seconds,
        surface_temperature_c,
        heat_release_rate_kw: hrr_kw,
        fuel_remaining_fraction: if initial_fuel_kj > 0.0 {
            node.fuel_remaining_kj / initial_fuel_kj
        } else {
            0.0
        },
    }
}

fn current_hrr_kw(node: &SurfaceFireNode) -> f64 {
    if node.state != CombustionState::Burning {
        return 0.0;
    }
    let initial_fuel_kj = node.material.fuel_energy_kj_per_m2 * node.area_m2;
    let fuel_fraction = if initial_fuel_kj > 0.0 {
        node.fuel_remaining_kj / initial_fuel_kj
    } else {
        0.0
    };
    let burnout_taper = (fuel_fraction * 4.0).clamp(0.0, 1.0);
    node.material.peak_hrr_kw_per_m2 * node.area_m2 * burnout_taper
}

fn ignition_energy(node: &SurfaceFireNode) -> f64 {
    node.material.ignition_energy_kj_per_m2 * node.area_m2
}

/// The room an object stands in. A coarse fire cell can straddle a shared
/// wall and list both rooms; the one whose outline contains the object's
/// position wins.
fn object_room_id(
    grid: &SimulationGrid,
    floor: &Floor,
    object: &FurnitureObject,
) -> Result<String, SurfaceFireError> {
    let position = object.transform.position_m;
    let room_ids = grid
        .world_to_cell(position)
        .and_then(|coordinate| grid.cell(coordinate))
        .map(|cell| &cell.room_ids)
        .filter(|room_ids| !room_ids.is_empty())
        .ok_or_else(|| SurfaceFireError::ObjectOutsideGrid {
            object_id: object.id.clone(),
        })?;
    let containing = room_ids.iter().find(|room_id| {
        floor
            .rooms
            .iter()
            .any(|room| room.id == **room_id && room_contains(room, position))
    });
    Ok(containing.unwrap_or(&room_ids[0]).clone())
}

fn find_asset<'a>(
    assets: &'a [FurnitureAsset],
    object: &FurnitureObject,
) -> Result<&'a FurnitureAsset, SurfaceFireError> {
    assets
        .iter()
        .find(|asset| asset.id == object.asset_id)
        .ok_or_else(|| SurfaceFireError::AssetNotFound {
            object_id: object.id.clone(),
            asset_id: object.asset_id.clone(),
        })
}

fn scaled_dimensions(asset: &FurnitureAsset, object: &FurnitureObject) -> Dimensions3 {
    Dimensions3 {
        width_m: asset.dimensions_m.width_m * object.transform.scale.x,
        depth_m: asset.dimensions_m.depth_m * object.transform.scale.y,
        height_m: asset.dimensions_m.height_m * object.transform.scale.z,
    }
}

fn validate_config(config: SurfaceFireConfig) -> Result<(), SurfaceFireError> {
    if !config.cell_size_m.is_finite() || config.cell_size_m <= 0.0 {
        return Err(SurfaceFireError::InvalidConfig {
            field: "cell_size_m",
        });
    }
    if !config.ambient_temperature_c.is_finite() {
        return Err(SurfaceFireError::InvalidConfig {
            field: "ambient_temperature_c",
        });
    }
    if !config.furniture_radiation_range_m.is_finite() || config.furniture_radiation_range_m < 0.0 {
        return Err(SurfaceFireError::InvalidConfig {
            field: "furniture_radiation_range_m",
        });
    }
    if !config.suppression_knockdown_seconds.is_finite()
        || config.suppression_knockdown_seconds <= 0.0
    {
        return Err(SurfaceFireError::InvalidConfig {
            field: "suppression_knockdown_seconds",
        });
    }
    Ok(())
}

fn validate_material(material: &FireMaterialProfile) -> Result<(), SurfaceFireError> {
    if material.id.trim().is_empty() {
        return Err(SurfaceFireError::InvalidMaterial {
            material_id: material.id.clone(),
            message: "ID cannot be empty",
        });
    }
    if !material.combustible {
        return Ok(());
    }
    for (value, message) in [
        (
            material.ignition_temperature_c,
            "ignition temperature must be finite and positive",
        ),
        (
            material.ignition_energy_kj_per_m2,
            "ignition energy must be finite and positive",
        ),
        (
            material.peak_hrr_kw_per_m2,
            "peak HRR must be finite and positive",
        ),
        (
            material.fuel_energy_kj_per_m2,
            "fuel energy must be finite and positive",
        ),
    ] {
        if !value.is_finite() || value <= 0.0 {
            return Err(SurfaceFireError::InvalidMaterial {
                material_id: material.id.clone(),
                message,
            });
        }
    }
    if !material.heat_transfer_fraction.is_finite()
        || !(0.0..=1.0).contains(&material.heat_transfer_fraction)
    {
        return Err(SurfaceFireError::InvalidMaterial {
            material_id: material.id.clone(),
            message: "heat transfer fraction must be between zero and one",
        });
    }
    if !material.cooling_kw_per_m2.is_finite() || material.cooling_kw_per_m2 < 0.0 {
        return Err(SurfaceFireError::InvalidMaterial {
            material_id: material.id.clone(),
            message: "cooling rate must be finite and nonnegative",
        });
    }
    Ok(())
}

fn floor_node_id(floor_id: &str, coordinate: GridCoordinate) -> String {
    format!("floor:{floor_id}:{}:{}", coordinate.column, coordinate.row)
}

fn furniture_cell_counts(
    object_id: &str,
    dimensions: Dimensions3,
) -> Result<(usize, usize, usize), SurfaceFireError> {
    let columns = (dimensions.width_m / FURNITURE_FIRE_CELL_SIZE_M).ceil() as usize;
    let rows = (dimensions.depth_m / FURNITURE_FIRE_CELL_SIZE_M).ceil() as usize;
    let layers = (dimensions.height_m / FURNITURE_FIRE_CELL_SIZE_M).ceil() as usize;
    let cell_count = columns
        .checked_mul(rows)
        .and_then(|count| count.checked_mul(layers))
        .unwrap_or(usize::MAX);
    if cell_count > MAX_FURNITURE_FIRE_CELLS_PER_OBJECT {
        return Err(SurfaceFireError::FurnitureGridTooLarge {
            object_id: object_id.to_owned(),
            cell_count,
            maximum: MAX_FURNITURE_FIRE_CELLS_PER_OBJECT,
        });
    }
    Ok((columns, rows, layers))
}

fn furniture_yaw_sin_cos(object: &FurnitureObject) -> (f64, f64) {
    let rotation = object.transform.rotation;
    let yaw = (2.0 * (rotation.w * rotation.z + rotation.x * rotation.y))
        .atan2(1.0 - 2.0 * (rotation.y * rotation.y + rotation.z * rotation.z));
    yaw.sin_cos()
}

fn object_node_id(object_id: &str) -> String {
    format!("object:{object_id}")
}

fn object_cell_node_id(object_id: &str, cell: FurnitureFireCell) -> String {
    let is_default_ignition_cell =
        cell.column == cell.columns / 2 && cell.row == cell.rows / 2 && cell.layer == 0;
    if is_default_ignition_cell {
        format!("object:{object_id}")
    } else {
        format!(
            "object:{object_id}:cell:{}:{}:{}",
            cell.column, cell.row, cell.layer
        )
    }
}

fn material(
    id: &str,
    ignition_temperature_c: f64,
    ignition_energy_kj_per_m2: f64,
    peak_hrr_kw_per_m2: f64,
    fuel_energy_kj_per_m2: f64,
    heat_transfer_fraction: f64,
    cooling_kw_per_m2: f64,
) -> FireMaterialProfile {
    FireMaterialProfile {
        id: id.to_owned(),
        combustible: true,
        ignition_temperature_c,
        ignition_energy_kj_per_m2,
        peak_hrr_kw_per_m2,
        fuel_energy_kj_per_m2,
        heat_transfer_fraction,
        cooling_kw_per_m2,
    }
}

fn furniture_material(category: &str, material: FireMaterialProfile) -> FurnitureFireMaterial {
    FurnitureFireMaterial {
        category: category.to_owned(),
        material,
    }
}
