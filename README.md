# tflop

## Workspace

```text
apps/
├── import-scene/       Matterport import CLI
└── web/                React + Three.js editor (Vite)
crates/
├── matterport/         Matterport API and normalization boundary
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

```bash
pnpm install
pnpm --filter web dev
```

In development, the app loads `data/prepared-scenes/<id>.json` through
`GET /api/scenes/<id>` (the route the Rust server will provide). It shows
the `MATTERPORT_MODEL_ID` from `.env` by default (only that key is read; the
credentials never reach the browser). Set `VITE_SCENE_ID` to load another
prepared scene. Run the importer below first.

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
