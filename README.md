# tflop

## Workspace

```text
apps/
└── import-scene/       Matterport import CLI
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

## Import a Matterport scene

Copy `.env.example` to `.env` and add the Matterport Token ID and Token Secret.
The importer also recognizes the earlier `MATTERPORT_API_KEY` and
`MATTERPORT_API_TOKEN` names for compatibility.

```bash
cargo run -p import-scene -- --model-id zvKHR9DHNjJ
```

The command fetches structured floor geometry, converts it to the canonical
Z-up world format, validates it, and writes:

```text
data/prepared-scenes/zvKHR9DHNjJ.json
```

Generated scenes are ignored by Git. They do not contain credentials or signed
Matterport asset URLs.

Imported scenes include a built-in five-item furniture catalog: chair, desk,
meeting table, sofa, and storage cabinet. These entries contain real-world
placeholder dimensions but no GLB URI yet, so renderers should display boxes
until final models are assigned.
