# Designer Agent — implementation plan

Status (2026-11-14): agent implemented with four engine tools (`get_scene`,
`list_assets`, `apply_edits`, `validate_layout`) over HTTP to `crates/server`,
which wraps the teammate's `World` API (`docs/AGENT_TOOLS.md`). Operating
contract: `DESIGNER.md`.

**Superseded below:** the variant tools/`variant_id`, the `Edit` type, and the
"Proposed engine HTTP contract". The real contract is `docs/AGENT_TOOLS.md`:
one in-memory `World`, no variants, edits keyed by `expected_revision`, angles as
`rotation_z_degrees`. Edits change that one scene; restarting the server resets
it. Next: user preferences (brief + `/memories/`), then variants and
simulation once the engine has them.

Open questions for the engine owner:

- `apply_edits` currently sends `position_m.z = 0`. Should the engine snap
  objects to the floor's elevation, or should the tool send `elevation_m`?
- Without variants, the agent's edits change the only scene. DESIGNER.md says
  never to mutate the baseline, so variants are needed before demo use.
Architecture: `TECH_STACK.md` §8–9.

## Scope of this package

`apps/agent` is a TypeScript LangChain Deep Agent. It reads the current scene
and the user's preferences, then changes the layout **only** by asking the
Rust engine to add, move, rotate, or remove furniture. The engine (owned by a
teammate) is the only thing that mutates or validates the scene; this package
never writes scene JSON.

Furniture assets are developer-supplied GLB files registered in the scene's
`assets` catalog (`FurnitureAsset`: `id`, `category`, `dimensions_m`). The
agent only references those asset IDs and places instances at coordinates.

## Stack

| Package | Version (npm latest, 2026-11-14) |
| --- | --- |
| `deepagents` | 1.16.5 |
| `langchain` | 1.6.6 |
| `@langchain/langgraph` | 1.7.4 |
| `@langchain/anthropic` | 1.6.5 |
| `zod` | latest at install |

Model: Claude through `@langchain/anthropic` (`ChatAnthropic`), configured by
`DEEP_AGENTS_MODEL` (default `claude-opus-5-5`; `claude-sonnet-5-5` is the
cheaper option) and `ANTHROPIC_API_KEY`. Model constraints that affect LangChain
config:

- do not pass `temperature`/`top_p`;
- thinking cannot be disabled on Opus 5.5, so lower the effort level instead;
- forced `tool_choice` (`any`/specific tool) returns a 400. Avoid any
  `responseFormat: toolStrategy(...)` path that forces a tool call, or verify
  it first.

Node 22, ESM (`"type": "module"`), run with `tsx` during development.

## Agent construction

```ts
const agent = createDeepAgent({
  model: new ChatAnthropic({ model: env.DEEP_AGENTS_MODEL }),
  systemPrompt: DESIGNER_PROMPT,           // required: deepagents sends no system message without one
  tools: [getScene, createVariant, applyEdits, validateLayout],
  middleware: [todoListMiddleware()],      // planning is opt-in in current deepagents
  contextSchema,                           // { sceneId, userId, variantId? }
  backend: new CompositeBackend(new StateBackend(), {
    "/memories/": new StoreBackend({ namespace: (rt) => ["prefs", rt.context.userId] }),
  }),
  memory: ["/memories/preferences.md"],    // loaded into the prompt every run
  store,                                   // InMemoryStore for the MVP
  checkpointer,                            // MemorySaver; one thread per chat session
});
```

Lockdown (DESIGNER.md: no shell, host filesystem, browsing, or general-purpose
subagents):

- Register a harness profile under `"anthropic"` with
  `generalPurposeSubagent: { enabled: false }` **and**
  `excludedTools: ["task", "execute"]`. Verified 2026-11-14: in 1.16.5 the
  subagent middleware is always installed, so `task` is still bound with zero
  subagents unless excluded. For a `ChatAnthropic` instance the profile key is
  derived from the class name (`ChatAnthropic` → `anthropic`). Covered by
  `test/agent.test.ts`.
- Use no `FilesystemBackend`/`LocalShellBackend`, so there is no `execute`
  tool and no access to the host filesystem. The remaining file tools touch only virtual
  state and the per-user `/memories/` store.

## Per-request context vs. model-chosen arguments

`sceneId` and `userId` arrive in `invoke(..., { context })` and tools read them
from `runtime.context`, so the model cannot target another scene or user.
`context.variantId` (the variant selected in the UI, or the baseline) is only
the default starting point for `create_variant`.

The model chooses `variant_id` explicitly on `get_scene`, `apply_edits`, and
`validate_layout`, so it can work on up to three variants it created. Tools
check that the variant belongs to the context scene. **The agent never edits the
baseline.** `apply_edits` rejects the baseline variant ID locally, and the
agent always forks first. The model also chooses the design arguments: asset,
object, coordinates, and yaw.

Edit scope: the user described the agent's job as **move** and **add**.
`rotate` and `remove` are kept because DESIGNER.md lists them. Either can be
dropped from the tool schema if the team prefers.

## User preferences

Two layers:

1. **Brief (this conversation).** On each design request the agent writes
   `/brief.md` in thread state: intended use, capacity, required furniture,
   which existing pieces to keep, style and soft preferences. It reads the
   brief back before editing. It asks one follow-up only when a missing fact
   (capacity, use) would materially change the layout.
2. **Durable preferences (per user, across sessions).** Stable statements
   such as "always keep windows clear" or "prefers round tables" are appended
   to `/memories/preferences.md`, which is stored per `userId` and auto-loaded into
   the prompt. The prompt says to save only explicit, durable preferences and
   never scene-specific facts. The MVP uses `InMemoryStore`, so preferences
   are lost on server restart. Swap the store later if that matters.

## Tools (agent-facing)

All tools return compact, model-readable JSON, never the full scene file.

| Tool | Model arguments | Engine call |
| --- | --- | --- |
| `get_scene` | `variant_id?` (default: context variant) | `GET /api/scenes/:sceneId?variant=` |
| `create_variant` | `name`, `from_variant_id?` (default: context variant) | `POST /api/scenes/:sceneId/variants` |
| `apply_edits` | `variant_id`, `revision`, `edits[]` | `POST /api/scenes/:sceneId/commands` |
| `validate_layout` | `variant_id` | `POST /api/scenes/:sceneId/variants/:id/validate` |

`simulate_egress` and `compare_variants` are added in optimize mode, after the
engine has simulation.

`get_scene` summary, built in TS from the engine snapshot:

- room outline polygon(s) in metres (x, y; Z-up world), area, bounds;
- doors/windows as world-space segments (derived from edge + `relative_center`
  + `width_m`), with confirmed exits marked;
- catalog: `asset_id`, `category`, footprint `width_m × depth_m`;
- furniture: `id`, `asset_id`, `category`, `x`, `y`, `yaw_deg`, `mobility`;
- `revision`.

Edits use **yaw in degrees about +Z** instead of quaternions. The tool converts
to `Quaternion { x:0, y:0, z:sin(θ/2), w:cos(θ/2) }`. Positions are the asset's
bottom-centre in metres; `z` is set to the floor elevation by the tool.

```ts
type Edit =
  | { op: "add";    asset_id: string; x: number; y: number; yaw_deg?: number }
  | { op: "move";   object_id: string; x: number; y: number }
  | { op: "rotate"; object_id: string; yaw_deg: number }
  | { op: "remove"; object_id: string };
```

Engine rejections (overlap, out of bounds, blocked door, stale revision,
fixed object) come back to the model verbatim as a structured tool result, not
as an exception. The agent then gets one repair attempt per DESIGNER.md.

## Proposed engine HTTP contract (for the Rust teammate)

A draft to agree on. It follows the TECH_STACK §9 routes and `world-core` types.

```text
GET  /api/scenes/:scene_id?variant=<id>
  200 { scene: Scene, variant_id, revision, exits: [opening_id] }

POST /api/scenes/:scene_id/variants
  { from_variant_id: string | null, name: string }
  200 { variant_id, revision }

POST /api/scenes/:scene_id/commands
  { variant_id, base_revision, commands: Command[] }      // atomic batch
  200 { ok: true, revision, created_object_ids: [string] }
  409/422 { ok: false, revision,
            errors: [{ command_index, code, message, object_ids: [string] }] }

POST /api/scenes/:scene_id/variants/:variant_id/validate
  200 { valid: bool,
        failures: [{ code, message, object_ids }],
        metrics: { object_counts_by_category, seat_count, min_clearance_m, ... } }

Command (serde tag "type"):
  AddObject    { asset_id, transform: Transform3 }        // engine assigns id
  MoveObject   { object_id, position_m: Vec3 }
  RotateObject { object_id, rotation: Quaternion }
  RemoveObject { object_id }
```

Suggested error `code`s: `stale_revision`, `unknown_asset`, `unknown_object`,
`fixed_object`, `out_of_bounds`, `overlap`, `blocks_opening`, `clearance`.

Open questions for the engine owner:

- Does the baseline have a variant ID, or is `variant=null` the baseline?
- Who assigns IDs on `AddObject`: the engine (proposed) or the client?
- Is clearance a validate-time failure or an apply-time rejection?
- Do confirmed exits live in the scene or in a separate list?

## Service surface

`POST /api/agent/turn` (Axum proxies to this service, or the web app calls it
during development): `{ scene_id, user_id, variant_id?, thread_id, message }`.
Responses stream Server-Sent Events from `agent.streamEvents(..., { version: "v3" })`:
assistant tokens, each tool call/result (so the editor can refetch the scene
after `apply_edits`), and the final message.

## Build order

1. Scaffold `apps/agent` (pnpm, ESM, tsx, Vitest) and a typed engine client.
2. **Engine for development (to be confirmed with the team):** until the
   teammate's server is running, either point at their branch or use a
   throwaway test double. The double implements the agreed contract with box
   overlap and room-bounds checks, plus a fixture room with one door and about five
   pieces. It is test scaffolding only, never a second source of truth.
3. Tools + system prompt; a CLI (`pnpm agent "..."`) that prints tool calls.
4. Spike checks with Claude:
   - print the tools bound to the model and assert that `task` and `execute`
     are absent. The harness profile is keyed by `provider:model`, so confirm
     it applies when we pass a `ChatAnthropic` instance;
   - the agent moves/adds via tools only and never invents asset IDs;
   - it repairs once after an `overlap` rejection;
   - a first run for a new user works when `/memories/preferences.md` does
     not exist yet;
   - a stated preference saved in thread A changes the layout in thread B.
     Run both threads **in one process**: `InMemoryStore`/`MemorySaver` don't
     survive separate CLI runs. A file-backed store is needed for that.
5. `POST /api/agent/turn` with SSE streaming.
6. Switch to the real engine; then optimize mode (`simulate_egress`,
   `compare_variants`).

## Known library caveats (from field notes; verify during step 1)

- Without `systemPrompt`, deepagents JS sends no system message.
- Seeding state files needs full `FileData` objects
  (`content`, `mimeType`, `created_at`, `modified_at`). Verified 2026-11-14:
  `createFileData` is **not** exported from `deepagents@1.16.5`, even though the
  docs import it. `registerHarnessProfile`, `StoreBackend`, and
  `CompositeBackend` are exported.
- An unknown `subagent_type` throws and ends the run in JS. With the
  general-purpose subagent disabled and no `task` tool this should not arise.
- Per-user `StoreBackend` namespace via `rt.context.userId` extends a documented
  `rt.context.orgId` example. Confirm it during the spike.
