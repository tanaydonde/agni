/**
 * Agent-facing tools over the world server.
 *
 * The model sees a simplified view: 2D floor-plane coordinates (x, y in
 * metres, Z-up world) and rotations as yaw degrees about +Z. This module
 * converts between that view and world-core's types (Vec3 positions,
 * quaternion rotations, `EditCommand`). Each tool returns a JSON string; on
 * failure that string is `{"error": {...}}` so the model can explain it.
 */
import { tool } from "langchain";
import * as z from "zod";

import type { EngineClient, EngineResult, Opening, Quaternion, Scene, Vec3 } from "./engine.js";

/** Round to `places` decimals so tool output stays short and readable. */
const round = (value: number, places = 3) => Math.round(value * 10 ** places) / 10 ** places;

/** Yaw about +Z in degrees, in [0, 360), from a Z-up quaternion. */
export function yawDegrees(q: Quaternion): number {
  const yaw = Math.atan2(2 * (q.w * q.z + q.x * q.y), 1 - 2 * (q.y * q.y + q.z * q.z));
  return round(((yaw * 180) / Math.PI + 360) % 360, 1);
}

/** Project a 3D point onto the floor plane. */
const point = (v: Vec3) => ({ x: round(v.x), y: round(v.y) });

/**
 * Convert a door/window, stored as a position along its wall
 * (`relative_center` 0..1, `width_m`), into world-space endpoints.
 */
function openingSegment(start: Vec3, end: Vec3, opening: Opening) {
  const length = Math.hypot(end.x - start.x, end.y - start.y);
  const ux = (end.x - start.x) / length;
  const uy = (end.y - start.y) / length;
  const center = opening.relative_center * length;
  const half = opening.width_m / 2;
  return {
    id: opening.id,
    kind: opening.kind,
    label: opening.label,
    width_m: opening.width_m,
    from: point({ x: start.x + ux * (center - half), y: start.y + uy * (center - half), z: 0 }),
    to: point({ x: start.x + ux * (center + half), y: start.y + uy * (center + half), z: 0 }),
  };
}

/**
 * Compact, model-readable view of the scene: 2D (x, y) metres in the Z-up
 * world, rotations as yaw degrees, openings as world-space segments.
 */
export function summarizeScene(revision: number, scene: Scene) {
  const assets = new Map(scene.assets.map((asset) => [asset.id, asset]));
  return {
    revision,
    scene_id: scene.id,
    units: "metres; x/y are floor-plane coordinates (Z-up); yaw_deg is rotation about +Z",
    catalog: scene.assets.map((asset) => ({
      asset_id: asset.id,
      category: asset.category,
      footprint_m: { width: asset.dimensions_m.width_m, depth: asset.dimensions_m.depth_m },
      height_m: asset.dimensions_m.height_m,
    })),
    floors: scene.floors.map((floor) => ({
      floor_id: floor.id,
      label: floor.label,
      rooms: floor.rooms.map((room) => ({
        room_id: room.id,
        label: room.label,
        area_m2: room.dimensions?.area_m2 ?? null,
        walls: room.boundary.map((edge) => ({ from: point(edge.start), to: point(edge.end), kind: edge.kind })),
        openings: room.boundary.flatMap((edge) =>
          edge.openings.map((opening) => openingSegment(edge.start, edge.end, opening)),
        ),
      })),
      objects: floor.objects.map((object) => {
        const asset = assets.get(object.asset_id);
        return {
          object_id: object.id,
          asset_id: object.asset_id,
          category: asset?.category ?? "unknown",
          x: round(object.transform.position_m.x),
          y: round(object.transform.position_m.y),
          yaw_deg: yawDegrees(object.transform.rotation),
          footprint_m: asset ? { width: asset.dimensions_m.width_m, depth: asset.dimensions_m.depth_m } : null,
          mobility: object.mobility,
        };
      }),
    })),
  };
}

/** Tool results are JSON strings; errors are returned, never thrown. */
function toToolResult<T>(result: EngineResult<T>, map: (value: T) => unknown = (value) => value): string {
  return JSON.stringify(result.ok ? map(result.value) : { error: result.error });
}

/** Floor-plane position fields shared by edits. */
const xy = { x: z.number().describe("metres"), y: z.number().describe("metres") };

/**
 * One model-facing edit. The `type` values match world-core's `EditCommand`
 * tags; positions are 2D and rotation is `yaw_deg`. `set_object_transform` is
 * deliberately not exposed: yaw plus position covers floor furniture.
 */
const editSchema = z.discriminatedUnion("type", [
  z.object({
    type: z.literal("add_object"),
    object_id: z.string().describe("New unique ID you choose, e.g. 'chair-7'"),
    asset_id: z.string().describe("Catalog asset_id from get_scene/list_assets"),
    floor_id: z.string(),
    ...xy,
    yaw_deg: z.number().optional().describe("Rotation about +Z in degrees (default 0)"),
  }),
  z.object({ type: z.literal("move_object"), object_id: z.string(), ...xy }),
  z.object({ type: z.literal("rotate_object"), object_id: z.string(), yaw_deg: z.number() }),
  z.object({ type: z.literal("remove_object"), object_id: z.string() }),
]);

type AgentEdit = z.infer<typeof editSchema>;

/**
 * Translate the model-facing edit (2D, yaw) into world-core's EditCommand.
 * z is sent as 0 (floor level); see PLAN.md's open question on elevation.
 * Agent-added objects are always movable.
 */
export function toEditCommand(edit: AgentEdit) {
  const position = (x: number, y: number): Vec3 => ({ x, y, z: 0 });
  switch (edit.type) {
    case "add_object":
      return {
        type: "add_object" as const,
        object_id: edit.object_id,
        asset_id: edit.asset_id,
        floor_id: edit.floor_id,
        position_m: position(edit.x, edit.y),
        rotation_z_degrees: edit.yaw_deg ?? 0,
        mobility: "movable" as const,
      };
    case "move_object":
      return { type: "move_object" as const, object_id: edit.object_id, position_m: position(edit.x, edit.y) };
    case "rotate_object":
      return { type: "rotate_object" as const, object_id: edit.object_id, rotation_z_degrees: edit.yaw_deg };
    case "remove_object":
      return { type: "remove_object" as const, object_id: edit.object_id };
  }
}

/**
 * The four tools given to the model. Single-edit routes such as move_object
 * exist on the server but are not exposed: apply_edits covers them
 * atomically. Tool descriptions are what the model reads to decide when to
 * call each tool.
 */
export function createEngineTools(engine: EngineClient) {
  const getScene = tool(async () => toToolResult(await engine.getScene(), (v) => summarizeScene(v.revision, v.scene)), {
    name: "get_scene",
    description:
      "Read the current room: walls, doors/windows, the furniture catalog, every placed object (id, position, yaw), and the scene revision. Call this before proposing or making changes.",
    schema: z.object({}),
  });

  const listAssets = tool(async () => toToolResult(await engine.listAssets()), {
    name: "list_assets",
    description: "List the furniture catalog (asset IDs, categories, dimensions). Only these assets can be placed.",
    schema: z.object({}),
  });

  const applyEdits = tool(
    async ({ expected_revision, edits }) =>
      toToolResult(await engine.applyEdits({ expected_revision, edits: edits.map(toEditCommand) })),
    {
      name: "apply_edits",
      description:
        "Atomically add, move, rotate, or remove furniture. Pass the revision from your latest get_scene as expected_revision. The engine validates every edit; if it returns an error, report it rather than guessing.",
      schema: z.object({
        expected_revision: z.number().int().nonnegative(),
        edits: z.array(editSchema).min(1),
      }),
    },
  );

  const validateLayout = tool(async () => toToolResult(await engine.validateLayout()), {
    name: "validate_layout",
    description: "Check the current layout against the engine's hard constraints and return any issues.",
    schema: z.object({}),
  });

  return [getScene, listAssets, applyEdits, validateLayout];
}
