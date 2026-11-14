/**
 * HTTP client for the Rust world server (`crates/server`).
 *
 * Request/response shapes mirror `tflop-world-core`'s agent API
 * (`docs/AGENT_TOOLS.md`). Methods never throw on HTTP or network failure:
 * they return an `EngineResult` so tools can hand the error to the model.
 */

/** Default address of `tflop-server`; override with RUST_ENGINE_URL. */
export const DEFAULT_ENGINE_URL = "http://127.0.0.1:3000";

// ---- world-core JSON types (hand-mirrored from crates/world-core) ----
// Keep in sync with model.rs and agent_api.rs. TECH_STACK.md plans ts-rs
// generation, which would replace these.

export interface Vec3 {
  x: number;
  y: number;
  z: number;
}

export interface Quaternion {
  x: number;
  y: number;
  z: number;
  w: number;
}

export type Mobility = "fixed" | "movable";

export interface FurnitureAsset {
  id: string;
  category: string;
  model_uri: string | null;
  dimensions_m: { width_m: number; depth_m: number; height_m: number };
}

export interface FurnitureObject {
  id: string;
  asset_id: string;
  transform: { position_m: Vec3; rotation: Quaternion; scale: Vec3 };
  mobility: Mobility;
}

export interface Opening {
  id: string;
  label: string | null;
  kind: "door" | "window" | "passage" | "unknown";
  width_m: number;
  height_m: number | null;
  lower_elevation_m: number | null;
  relative_center: number;
}

export interface BoundaryEdge {
  id: string;
  kind: "wall" | "invisible" | "unknown";
  start: Vec3;
  end: Vec3;
  thickness_m: number | null;
  center_line_bias: number | null;
  openings: Opening[];
}

export interface Room {
  id: string;
  label: string | null;
  classifications: { label: string; confidence: number | null }[];
  dimensions: {
    area_m2: number | null;
    width_m: number | null;
    depth_m: number | null;
    height_m: number | null;
    lower_elevation_m: number | null;
  } | null;
  boundary: BoundaryEdge[];
}

export interface Floor {
  id: string;
  label: string | null;
  elevation_m: number | null;
  rooms: Room[];
  objects: FurnitureObject[];
}

export interface Scene {
  schema_version: number;
  id: string;
  name: string | null;
  source: { provider: string; external_id: string };
  assets: FurnitureAsset[];
  floors: Floor[];
}

export interface GetSceneResponse {
  revision: number;
  scene: Scene;
}

export interface ListAssetsResponse {
  assets: FurnitureAsset[];
}

/** Matches world-core's `EditCommand` (serde tag "type", snake_case). */
export type EditCommand =
  | {
      type: "add_object";
      object_id: string;
      asset_id: string;
      floor_id: string;
      position_m: Vec3;
      rotation_z_degrees: number;
      mobility: Mobility;
    }
  | { type: "move_object"; object_id: string; position_m: Vec3 }
  | { type: "rotate_object"; object_id: string; rotation_z_degrees: number }
  | { type: "remove_object"; object_id: string };

export interface ApplyEditsRequest {
  expected_revision: number;
  edits: EditCommand[];
}

export interface EditResponse {
  revision: number;
  changed_object_ids: string[];
}

export interface ValidateLayoutResponse {
  valid: boolean;
  issues: { path: string; message: string }[];
}

/** World errors keep world-core's shape: `{ code, ...details }`. */
export interface EngineError {
  code: string;
  status?: number;
  [detail: string]: unknown;
}

/** Outcome of an engine call; never thrown. */
export type EngineResult<T> = { ok: true; value: T } | { ok: false; error: EngineError };

/**
 * Calls the `/api/world/*` routes of `tflop-server`. Errors are normalized
 * to `EngineError`:
 * - engine errors keep world-core's body plus `status` (e.g. 501
 *   `not_implemented`);
 * - other HTTP errors become `http_error`;
 * - network failures become `engine_unreachable`;
 * - a 2xx response with a non-JSON body becomes `invalid_response`.
 */
export class EngineClient {
  constructor(readonly baseUrl: string = process.env.RUST_ENGINE_URL || DEFAULT_ENGINE_URL) {}

  /** Full canonical scene plus its revision. */
  getScene() {
    return this.request<GetSceneResponse>("GET", "/api/world/scene");
  }

  /** Furniture catalog. */
  listAssets() {
    return this.request<ListAssetsResponse>("GET", "/api/world/assets");
  }

  /** Atomic batch of edits; rejected if `expected_revision` is stale. */
  applyEdits(request: ApplyEditsRequest) {
    return this.request<EditResponse>("POST", "/api/world/apply_edits", request);
  }

  /** Hard-constraint check of the current layout. */
  validateLayout() {
    return this.request<ValidateLayoutResponse>("POST", "/api/world/validate_layout");
  }

  private async request<T>(method: string, path: string, body?: unknown): Promise<EngineResult<T>> {
    let response: Response;
    try {
      response = await fetch(new URL(path, this.baseUrl), {
        method,
        headers: body === undefined ? undefined : { "content-type": "application/json" },
        body: body === undefined ? undefined : JSON.stringify(body),
      });
    } catch (error) {
      return {
        ok: false,
        error: {
          code: "engine_unreachable",
          message: `Could not reach the world server at ${this.baseUrl}: ${(error as Error).message}`,
        },
      };
    }

    const text = await response.text();
    let parsed: unknown;
    try {
      parsed = text ? JSON.parse(text) : null;
    } catch {
      parsed = undefined;
    }

    if (response.ok) {
      if (parsed === undefined) {
        return { ok: false, error: { code: "invalid_response", status: response.status, message: text.slice(0, 500) } };
      }
      return { ok: true, value: parsed as T };
    }

    if (parsed && typeof parsed === "object" && typeof (parsed as EngineError).code === "string") {
      return { ok: false, error: { ...(parsed as EngineError), status: response.status } };
    }
    return { ok: false, error: { code: "http_error", status: response.status, message: text.slice(0, 500) } };
  }
}
