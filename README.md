# tflop

## Workspace

```text
apps/
├── import-scene/       Matterport import CLI
└── web/                React + Three.js editor (Vite)
crates/
├── matterport/         Matterport API and normalization boundary
├── server/             HTTP server exposing the world API (cargo run -p tflop-server)
└── world-core/         Canonical scene types and world rules
data/
├── fixtures/           Small sanitized test inputs
└── prepared-scenes/    Local generated scene output
```

Run all Rust checks from the repository root:

```bash
cargo check --workspace
cargo test --workspace
```

## Run the web app

The Rust world server owns the scene; the web app reads and edits it through
`/api/world/*`, which Vite proxies to `TFLOP_SERVER_ADDR` (default
`127.0.0.1:3000`). Import the scene first (below), then in two terminals:

```bash
cargo run -p tflop-server -- --scene data/prepared-scenes/<model-id>.json
pnpm install && pnpm --filter web dev
```

Edits live in the server's memory; restarting it reloads the file. The
defurnished mesh is still served by Vite from
`data/prepared-scenes/<model-id>.obj`.

### Editing and flying

The view has two modes. The cursor is only captured while flying.

- **Edit mode** (default): drag an item from the Furniture panel into the
  view; it follows the floor and is added where you release it. Click
  furniture to select it (click anything else to deselect). Drag the red (X)
  or green (Y) arrow to slide along that axis, the grey square to move freely
  across the floor, or the blue ring to rotate (hold Shift to snap to 15°).
  Del removes the selection and Esc deselects. Pinch moves toward the cursor,
  a two-finger swipe looks around, and ⌥ + drag orbits.
- **Fly mode**: right-click (or Control-click) to capture the cursor; Esc or
  another right-click returns to edit mode. Move to look, WASD/QE to fly,
  Shift for speed, Space + move to pan, ⌥ + move to orbit. Nothing can be
  selected or edited while flying.

While you drag, the world dry-runs the placement (`POST /api/world/check_edits`)
against its rules: inside a room, clear of walls and other furniture, and out
of door clearances. Furniture turns red with the reason where it can't go, and
releasing there puts it back.

Furniture always stands on the floor: the world snaps every object's
`z` to its floor's elevation and rejects tilted rotations.

## Import a Matterport scene

Copy `.env.example` to `.env` and add the Matterport Token ID, Token Secret,
and model ID. The importer also recognizes the earlier `MATTERPORT_API_KEY` and
`MATTERPORT_API_TOKEN` names for compatibility.

```bash
cargo run -p import-scene          # model from MATTERPORT_MODEL_ID
```

The command fetches structured floor geometry, converts it to the canonical
Z-up world format, validates it, and writes:

```text
data/prepared-scenes/<model-id>.json   canonical scene (walls, openings, rooms)
data/prepared-scenes/<model-id>.obj    Matterport's defurnished mesh
```

The 3D view shows the defurnished mesh (untextured, furniture removed) and
falls back to a shell built from the JSON. Pass `--skip-mesh` to import only
the JSON. The JSON stays the source of truth for walls, openings, and
simulation. Generated files are ignored by Git and contain no credentials or
signed Matterport URLs.

Imported scenes include a built-in five-item furniture catalog: chair, desk,
meeting table, sofa, and storage cabinet. These entries contain real-world
placeholder dimensions but no GLB URI yet, so renderers should display boxes
until final models are assigned.
