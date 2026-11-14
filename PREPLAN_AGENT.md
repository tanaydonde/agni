# Preplan Agent

## Purpose

The Preplan Agent helps a fire officer turn a joined property record (CoStar
property data, Vexcel aerial imagery, and a Matterport interior scan) into a
reviewable building brief, then set up and compare hypothetical fire
scenarios for preparation and training.

It is not the geometry, verification, or simulation authority. The Rust world
engine owns the canonical scene, provenance and verification status,
simulation, metrics, and saved scenarios. The officer is the only one who can
confirm a fact.

The implementation is a **LangChain Deep Agent written in TypeScript**. It
lives in `apps/agent`, runs behind the Rust/Axum API, and uses a configured
LangChain model provider. Gemini can be that provider, but Deep Agents (not a
direct Gemini function-calling loop) is the agent harness.

## Inputs and saved state

| Data | Owner | Role |
| --- | --- | --- |
| Property record | Rust world engine | CoStar fields, Vexcel imagery references, Matterport scan coverage, and each source's capture date. |
| Canonical scene snapshot | Rust world engine | Imported geometry, openings, obstacles, exits, access points, routes, verification status, and revision. |
| Findings | Rust world engine | Agent-proposed access points, routes, obstacles, and verification items, each `unverified`, `confirmed`, or `rejected`. |
| Scenario definitions and runs | Rust world engine | Fire origin, occupant setup, crew assignments, blocked routes, seed, and resulting metrics. |
| Officer's goal and notes | Deep Agent state and the UI | What the officer wants to rehearse or check. |

The agent never writes or submits raw scene JSON. It can keep private notes in
Deep Agents' in-memory state, but the only way to change the record is through
the typed tools below, and every agent-created finding starts as
`unverified`.

## Agent modes

### Brief mode

Use this when an officer opens a building, or asks something like: "Build me a
preplan for this building."

1. Read the property record and scene. Note what each source covers and what
   it does not (unscanned floors, missing imagery angles, sparse CoStar
   fields).
2. Propose candidate **access points** from imagery and scan openings, with the
   reasoning and source for each.
3. Trace **known interior routes** from access points and openings through the
   scanned geometry, and flag dead ends, narrow passages, and routes that leave
   the scanned area.
4. Flag **potential obstacles**: storage, dense furniture, long travel
   distances, and doors whose operability is unknown.
5. Build the **verification list**: everything a crew must check in person.
   Unscanned areas, exit discharge, door locking, hydrants, standpipes and FDC,
   sprinklers and alarms, hazardous contents, and occupancy are unknown unless
   a source establishes them.
6. Draft the preplan card from the findings, keeping verified and unverified
   facts visibly separate.

Brief mode does not run simulations unless the officer asks.

### Rehearsal mode

Use this when the officer wants to explore a scenario, e.g. "What if the fire
starts in the back storage room during business hours?"

1. Read the current scene and confirmed findings. If a scenario depends on an
   unverified exit or route, say so before running it.
2. Create at most three scenarios that differ in one or two clear ways (fire
   origin, a blocked route, occupant count or familiarity, crew assignment).
3. Run every scenario with the same seed and sample count.
4. Use `compare_scenarios` to report time-to-exit ranges, exit usage and
   bottlenecks, unreachable or unsearched areas, crew task timing ranges, and
   occupants still inside at cutoff.
5. Identify the **sensitive assumptions**: inputs whose change materially
   altered the result. Suggest which of them the crew should verify first.
6. When the officer corrects a fact, rerun the same scenarios and explain what
   changed.

The MVP budget is three scenarios per request and the server's configured
simulation timeout.

## Constraints

Hard rules, enforced by Rust:

- scene geometry, scan boundaries, and source data cannot be edited by the
  agent;
- the agent cannot set any finding to `confirmed`; only the officer can, through
  the UI;
- only confirmed exits and routes are usable in a scenario unless the scenario
  explicitly lists the unverified ones it assumes;
- areas outside scan coverage are unknown and cannot be traversed in
  simulation;
- every command targets the current scene revision and is applied atomically.

Behavioral rules for the agent:

- Present results as outcomes of a hypothetical training scenario under the
  stated assumptions, never as a prediction of a real fire.
- Never recommend a single optimal entry point or a live-incident tactic, and
  never rank strategies for use at an incident.
- Never describe occupants remaining at cutoff as deaths or casualties.
- Never claim code compliance or certification.
- Report missing or stale data plainly instead of filling gaps.

## Rust tool contract

The TypeScript service exposes only these structured tools to the Deep Agent.
Each tool calls the Axum backend; no tool accepts a raw scene file.

| Tool | Required behavior |
| --- | --- |
| `get_property(property_id)` | Return CoStar fields, Vexcel imagery references, Matterport scan coverage, and capture dates. |
| `get_scene(scene_id)` | Return geometry summary, openings, obstacles, exits, access points, routes, findings with status, and revision. |
| `propose_findings(scene_id, revision, findings)` | Add or update `unverified` access points, routes, obstacles, and verification items, each with a source and rationale. |
| `apply_edits(scene_id, revision, commands)` | Atomically apply typed obstacle commands (`AddObject`, `MoveObject`, `RotateObject`, `RemoveObject`), used when the officer asks the agent to reflect a walkthrough correction. |
| `create_scenario(scene_id, spec)` | Define a scenario: fire origin and hazard timeline, occupant setup, crew assignments, blocked routes, assumed unverified items, seed, and sample count. |
| `run_scenario(scenario_id)` | Queue a deterministic simulation run. |
| `compare_scenarios(scenario_ids)` | Return engine-computed metrics with ranges, bottlenecks, unreachable areas, and assumption sensitivity; no model-authored score. |

The agent has no tool to confirm findings, edit geometry, change simulation
rules, access source APIs directly, or read credentials. The officer decides
what is confirmed, saved, and exported through the UI.

## Deep Agent instructions

Use the following system instruction as the starting point:

> You help a fire officer prepare for a commercial building before an
> emergency. Inspect the property record and scene first, and note what each
> data source covers and does not. Propose access points, interior routes,
> obstacles, and verification items with a source and rationale for each; all
> proposals are unverified until the officer confirms them. When rehearsing,
> create no more than three scenarios, run them on the same seed, and use
> engine metrics to explain bottlenecks, unreachable areas, and which
> assumptions most affect the outcome. Everything is a hypothetical training
> scenario: never predict a real fire, recommend a live tactic or single best
> entry, describe casualties, or claim code compliance. Report missing data
> plainly.

The Deep Agent may use its planning and private state tools, but production
configuration must not give it a shell, a host filesystem, network browsing,
or general-purpose subagents. Its only external side effects are the engine
tools above.

## Example officer prompts

- "Build a preplan for this building. What do we know about access from the
  rear, and what do we need to check on a walkthrough?"
- "Run a fire in the back storage room at noon with 40 occupants who mostly
  know only the front entrance. Engine 2 does a primary search of the east
  offices."
- "Now assume the rear corridor door is locked. What changes?"
- "Which of our unverified items matter most for the search plan?"

## Acceptance checks

- Every finding the agent creates is `unverified` and has a source and
  rationale.
- Areas outside scan coverage appear as unknown in the brief and are never
  traversed in simulation.
- A scenario that depends on an unverified item lists it as an assumption in
  its results.
- Compared scenarios share the same seed and sample count.
- Results report ranges, bottlenecks, unreachable areas, and assumptions; they
  contain no casualty counts, live tactical recommendations, or compliance
  claims.
