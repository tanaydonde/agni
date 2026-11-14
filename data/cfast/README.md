# CFAST feasibility scenario

`feasibility.in` is a minimal vertical-slice input for CFAST 7.7.7. It contains
one 5 m × 4 m × 3 m compartment, one exterior door, and one prescribed fire
that grows to 300 kW. It proves the external solver and output format before we
automate `world-core` conversion.

CFAST is an external NIST command-line program; its binary is intentionally not
committed. Generated result files belong under `data/cfast/output/`, which is
ignored by Git.

The scenario was verified against the official `CFAST-7.7.7` tag at commit
`b43e74e0f43c704582c5782f5446597ce729a07f`. On Apple Silicon it builds from
source with the repository's `Build/CFAST/gnu_osx/make_cfast.sh` script and
Homebrew `gfortran`.

Run a built solver against a staged copy of the input:

```bash
mkdir -p data/cfast/output
cp data/cfast/feasibility.in data/cfast/output/feasibility.in
cd data/cfast/output
/path/to/cfast7_osx feasibility.in
```

CFAST 7.7.7 writes these spreadsheet outputs:

- `*_compartments.csv`: layer temperatures and height, gas concentrations,
  optical density, fire HRR, and flame height
- `*_vents.csv`: vent mass flow and opening fraction
- `*_masses.csv`: upper/lower layer species masses
- `*_walls.csv`: ceiling, wall, and floor temperatures
- `*_devices.csv`: target, detector, and sprinkler results when configured

In the verified run, CFAST exited normally in about 0.04 seconds. At 119 s the
300 kW fire produced an upper-layer temperature of about 322 °C, a lower-layer
temperature of about 83 °C, and a smoke-layer interface about 1.27 m above the
floor. These are only feasibility values, not a validated real-building fire
scenario.

The `tflop-cfast` crate now completes the full compartment pipeline:

1. Export every `world-core` room as a positioned CFAST compartment.
2. Resolve explicit openings into room-to-room or room-to-outside connections.
3. Apply the scenario's open fraction for every resolved opening.
4. Verify and launch an external CFAST 7.7.7 executable in a temporary folder.
5. Parse per-room `*_compartments.csv` conditions and every `*_vents.csv` flow.
6. Validate and return provider-independent `SimulationResults`.

```rust
let prepared = prepare_cfast_scene(&matterport_scene)?;
for warning in &prepared.report.warnings {
    // Show or persist every inferred, adjusted, or excluded input.
}
let runner = CfastRunner::new("/path/to/cfast7")?;
let results = runner.run_all_rooms(&prepared.scene, &scenario)?;
```

The executable remains external and configurable. Every resolved opening must
have a `CfastOpeningState` with an `open_fraction` from 0 (closed) to 1 (fully
open). CFAST receives that fraction as a constant time-based vent schedule.
Solid shared walls are never exported as vents.

`prepare_cfast_scene` never mutates the canonical Matterport scene. Its CFAST
copy infers missing room dimensions from boundary geometry and measured rooms,
adjusts inconsistent opening geometry, and excludes geometry that cannot be
placed. Every such decision is returned as a typed warning in the preparation
report rather than being hidden.
