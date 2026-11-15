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

What exists today is the Matterport import, the CoStar property join, the
Rust world server, the fire and evacuation simulation, and the 3D editor
below. The furniture editor now serves to model interior obstacles. Vexcel
ingestion, findings, and the preplan agent are planned (see `TECH_STACK.md`).

## Workspace

```text
apps/
├── detect-furniture/   Furniture detection CLI (scan → measured boxes)
├── import-property/    CoStar join CLI (scan → <model-id>.property.json)
├── import-scene/       Matterport import CLI
└── web/                React + Three.js editor (Vite)
crates/
├── costar/             CoStar record join and derived scenario assumptions
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

### Selecting and flying

The view has two modes. The cursor is only captured while flying.

- **Edit mode** (default): click a piece of furniture to highlight it as the
  fire origin for **Run simulation** (click anything else, or press Esc, to
  deselect). Furniture can't be moved, rotated, added, or deleted from the UI.
  Pinch moves toward the cursor, a two-finger swipe looks around, and ⌥ + drag
  orbits.
- **Fly mode**: right-click (or Control-click) to capture the cursor; Esc or
  another right-click returns to edit mode. Move to look, WASD/QE to fly,
  Shift for speed, Space + move to pan, ⌥ + move to orbit. Nothing can be
  selected while flying.

The world server still accepts edits (`POST /api/world/apply_edits`, dry runs
via `check_edits`) from agents and scripts. It snaps every object's `z` to its
floor's elevation and rejects tilted rotations.

## Import a Matterport scene

Copy `.env.example` to `.env` and add the Matterport Token ID, Token Secret,
and model ID. The importer also recognizes the earlier `MATTERPORT_API_KEY` and
`MATTERPORT_API_TOKEN` names, and the hackathon's `COSTAR_MATTERPORT_KEY` and
`COSTAR_MATTERPORT_PASSWORD`, for compatibility.

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

## Join the scan to its CoStar record

Every Matterport model in the CoStar hackathon kit belongs to a commercial
property or a residential MLS listing. Unzip the kit (password from the
organizers) to `data/costar-kit/` (gitignored; override with
`COSTAR_DATA_PATH`), then:

```bash
cargo run -p import-property       # model from MATTERPORT_MODEL_ID
```

This writes `data/prepared-scenes/<model-id>.property.json`. It holds the
building facts that matter for a fire preplan, such as year built, stories,
construction, sprinklers, elevators, bedrooms, floor finish, units and current
tenants. Each fact names the CoStar field it came from. Agents, offices,
emails and phone numbers are never copied. The server loads the file next to
the scene and serves it at `GET /api/property` with derived scenario
assumptions:

| Assumption | Bungalow `zvKHR9DHNjJ` (MLS listing) | Pinemont `xKKeATjBh2m` ("After School Care") |
| --- | --- | --- |
| Occupants | 3 bedrooms × 2 = 6 sleepers | 747 sq ft scanned / 35 sq ft per person (IBC childcare) = 22 |
| Who, sent to Gemini's placer | A household asleep at night, plus room labels | Children with staff, daytime |
| Alarm | Noticed at 100 kW, 30 s to react | Noticed at 50 kW, 5 s to react |
| Need help out | 15% | 30% |
| Floor | Hardwood (listing) | Carpet (default) |
| Crew must verify | Balloon framing (1920), smoke alarms, fireplace, sprinklers, garage conflict | Scan is one room of 390 units, wood voids, sprinklers unknown, stair-only egress |

All of these are unverified assumptions. The server applies the building
context on its own: `spread` occupant placement sends it to Gemini with the
room labels. A client opts into the rest by passing the profile's values to
`/api/fire/simulate` or `/api/fire/simulate/hybrid`: `alarm_config` (custom
`detected` timings), `injured_fraction` and `floor_material` (`carpet` or
`hardwood`). The response's `gemini_placement` says whether `spread` is
available. Without a property file, everything behaves as before.

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
