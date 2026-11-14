# tflop

Firefighter preincident planning and scenario rehearsal. For a commercial
building, tflop combines CoStar property data, Vexcel aerial imagery, and a
Matterport interior scan into a visual building brief. An agent proposes
access points, interior routes, and obstacles, and marks what a crew still
needs to verify. A simulation lets crews rehearse hypothetical fires with
occupants and search crews, then compare scenarios for bottlenecks,
inaccessible areas, and sensitive assumptions. It is for preparation and
training, not live tactical decisions.

| Doc | Contents |
| --- | --- |
| `PRD.md` | Product requirements, user, demo, and MVP boundaries |
| `TECH_STACK.md` | Architecture, data ingestion, simulation, and build order |
| `PREPLAN_AGENT.md` | Agent modes, rules, and tool contract |
| `docs/AGENT_TOOLS.md` | World API tools implemented today |
| `docs/FIRE_PREPLAN_RESEARCH.md` | Research and reasoning behind the product boundaries |

What exists today is the Matterport import, the Rust world server, and the 3D
editor below. The furniture editor now serves to model interior obstacles;
CoStar and Vexcel ingestion, findings, the simulation, and the agent are
planned (see `TECH_STACK.md`).

## Workspace

```text
apps/
├── detect-furniture/   Furniture detection CLI (scan → measured boxes)
├── import-scene/       Matterport import CLI
└── web/                React + Three.js editor (Vite)
crates/
├── furniture-detect/   Furnished vs defurnished mesh diff into furniture boxes
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
`127.0.0.1:3000`). The fire action runs the hybrid Rust + CFAST simulation.
CFAST 7.7.7's Fortran source is vendored in `crates/cfast/vendor/` and
compiled automatically by `cargo build`. The only prerequisite is
`gfortran` (macOS: `brew install gcc`; Debian/Ubuntu: `sudo apt install
gfortran`). If it's missing, the build prints a warning and the fire action
reports `cfast_not_configured`. Install it, then run `touch
crates/cfast/build.rs` and restart the server. To use a different CFAST
executable, set `CFAST_BIN` in `.env`. Import the scene first (below), then
in two terminals:

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

## Detect furniture from the scan

The editor shows the defurnished mesh, so the real furniture is detected and
drawn back in as measured boxes (hitboxes for later simulation). Download the
furnished mesh from the MatterPak (about 140 MB) once, then run the detector:

```bash
cargo run -p import-scene -- --furnished-mesh
cargo run --release -p detect-furniture
```

This writes `data/prepared-scenes/<model-id>.detected.json`, which the editor
draws as translucent boxes (toggle with **Hide/Show scanned furniture**).

The detector (`crates/furniture-detect`) is deterministic. Surfaces in the
furnished mesh that are more than about 10 cm from any surface of the
defurnished mesh are furniture. They are voxelized, projected to a top-down
grid, split into connected pieces, and fitted with the smallest rotated
rectangle; the height is the top of each piece. Things hanging above a large
vertical gap (a chandelier over a table) are ignored, and footprints that fill
their rectangle poorly (an L-shaped sofa) are split into several boxes. Each
box gets a label (sofa, chair, table, desk, bed, storage, counter, or
unknown) from its size and room type. The editor colors it like the catalog
asset closest in size; the textured and furnished views draw it as the
furniture model with the same label that is closest in size (see below),
stretched to fill the box, and leave it out when no model has that label.
The measured box is always the object's size.

## Furniture models

The model files are not in Git (some licenses don't allow redistributing
them); download each one from its link in `assets/furniture/CREDITS.md`.
Downloaded models go, untouched, in `assets/furniture/source/`, named as in
`assets/furniture/models.json`. Give each
one an entry in `assets/furniture/models.json` with its real-world size: a
`scale` when the units are known (`0.01` for centimetres) or a `height_m`
when they aren't, plus `yaw_degrees` if its front doesn't face +Z. Then:

```bash
pnpm --filter web normalize-furniture        # all models, or name model ids
```

This writes `apps/web/public/models/furniture/<id>.glb` in metres, Y-up,
with the long side along X and the footprint centred on the origin at floor
level, and regenerates `assets/furniture/CREDITS.md` from each model's
embedded author and license.
