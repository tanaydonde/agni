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

### Placing furniture

- Drag an item from the Furniture panel into the view. It follows the floor
  under the cursor and is added where you release it.
- Click the view to fly (Esc releases). Aim the crosshair at furniture and
  click to select it; click empty space to deselect.
- Press on the red (X) or green (Y) arrow and move to slide along that axis,
  or on the blue square to move freely across the floor. Del removes it.

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
