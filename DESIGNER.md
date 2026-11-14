# Designer Agent

## Purpose

The Designer Agent turns a professional interior designer's natural-language
brief into several tested layout variants. It is not the geometry, fire, or
evacuation authority. The Rust world engine owns the canonical scene,
validation, simulation, scores, and saved variants.

The implementation is a **LangChain Deep Agent written in TypeScript**. It
lives in `apps/agent`, runs behind the Rust/Axum API, and uses a configured
LangChain model provider. Gemini can be that provider, but Deep Agents—not a
direct Gemini function-calling loop—is the agent harness.

## Inputs and saved state

The system has three distinct kinds of data:

| Data | Owner | Role |
| --- | --- | --- |
| Canonical scene snapshot | Rust world engine | Imported Matterport geometry, confirmed exits, fixed elements, furniture, and revision. |
| Design brief | Deep Agent state and the UI | The designer's goals, required objects, capacity, style, and preferences. |
| Variant snapshot | Rust world engine | A fork of the baseline containing only engine-applied typed commands. |

The engine may persist scene and variant snapshots as versioned JSON files,
but the agent never writes or submits raw layout JSON. It can use Deep Agents'
private in-memory/file state for a structured brief and candidate notes. The
only way to create an engine-readable proposal is `create_variant` followed by
`apply_edits`.

This keeps a text representation available for debugging and repeatability
without allowing an LLM to bypass geometry validation or overwrite the
baseline.

## Agent modes

### Design mode

Use this for prompts such as: “Design a warm, high-end restaurant for 20
people, with clear circulation and a secondary TV area.”

1. Read the scene and extract a concise design brief.
2. Ask one focused follow-up only when a missing requirement would materially
   change the layout—for example capacity, intended use, or a required
   furniture type.
3. Create up to three distinct variants: `balanced`, `design_forward`, and
   `safety_forward` when the prompt calls for it.
4. Apply a small, coherent batch of typed furniture commands to each variant.
5. Validate every variant. Repair a failed variant once using engine feedback;
   otherwise discard it and explain why.
6. Present only validated variants, with the functional tradeoffs and metrics
   returned by the engine.

Design mode does not run the expensive fire/egress simulation unless the user
asks for safety optimization or explicitly starts a simulation.

### Optimize mode

Use this when the designer selects an existing baseline or variant and asks to
improve emergency performance without abandoning the brief.

1. Read the selected scene, its brief, and the configured seeded scenario set.
2. Create at most three candidate variants from the selected starting point.
3. Validate candidates before any simulation; invalid layouts do not consume a
   simulation run.
4. Simulate every valid candidate against the same seed and scenario set.
5. Use `compare_variants` to identify actual bottlenecks, then make one
   targeted repair round per candidate and repeat validation/simulation.
6. Return the best tested valid layout—not a globally optimal or certified-safe
   layout—and preserve the original functional requirements.

The MVP budget is three candidates, one repair round, and the server's
configured simulation timeout. The UI can expose progress from the underlying
jobs.

## Constraints

Hard constraints are enforced by Rust and make a variant invalid:

- scene boundaries, walls, fixed objects, and confirmed exits cannot move;
- furniture must use catalog assets and remain inside usable space;
- movable furniture cannot overlap walls, fixed objects, or other furniture;
- doors must remain operable and exits/safety equipment unobstructed;
- configured clearances and reachable-area requirements must hold;
- required object types, seat count, capacity, and functional zones from the
  brief must remain present;
- every command must target the current scene revision and is applied atomically.

Soft constraints influence engine-generated metrics and recommendation text;
they never override hard constraints:

- style, visual balance, and room zoning;
- sightlines, window access, daylight, and conversational grouping;
- convenience of service/work circulation;
- capacity above the requested minimum; and
- evacuation time, exposure, congestion, and travel distance among layouts
  that satisfy the hard constraints.

The agent must describe engine results as simulation results under the selected
rules and seed. It must not claim building-code certification or invent a fire
compliance rule.

## Rust tool contract

The TypeScript service exposes only these structured tools to the Deep Agent.
Each tool calls the existing Axum backend; no tool accepts a raw scene file.

| Tool | Required behavior |
| --- | --- |
| `get_scene(scene_id)` | Return the current snapshot summary, catalog, constraints, exits, and revision. |
| `create_variant(scene_id, from_variant_id, name)` | Fork an immutable source snapshot; never mutate the baseline. |
| `apply_edits(variant_id, revision, commands)` | Atomically validate and apply typed `AddObject`, `MoveObject`, `RotateObject`, or `RemoveObject` commands. |
| `validate_layout(variant_id)` | Return structured hard-constraint failures and functional metrics. |
| `simulate_egress(variant_id, scenario_set_id)` | Queue a deterministic simulation using the selected fixed seed/scenario set. |
| `compare_variants(variant_ids, scenario_set_id)` | Return normalized metrics, failed constraints, and no model-authored score. |

The agent has no tool to modify room geometry, confirm exits, reset or delete
variants, change simulation rules, access Matterport, or read credentials. A
designer chooses when to save, promote, or export a variant through the UI.

## Deep Agent instructions

Use the following system instruction as the starting point:

> You are an interior-layout planning agent for professional designers. First
> inspect the current scene. Translate the user's request into a concise brief
> with requirements, hard constraints, and preferences. Use only catalog items
> and typed engine commands. Create no more than three variants and validate
> each before recommending it. When optimizing, test every valid candidate on
> the same seeded scenario set, make at most one targeted repair round, and use
> engine metrics to compare results. Never edit scene JSON, alter fixed
> geometry, or claim code certification. Preserve required capacity, objects,
> and intended use; report unmet requirements plainly.

The Deep Agent may use its planning and private state tools, but production
configuration must not give it a shell, a host filesystem, network browsing,
or general-purpose subagents. Its only external side effects are the six
engine tools above. This keeps agent behavior auditable and bounded.

## Example designer prompts

- “Create three layouts for this restaurant for 20 guests. Keep a clear route
  to every confirmed exit, leave the windows visually open, and make it feel
  warm rather than cramped.”
- “Give me a collaborative office layout for 24 people with focused desks,
  meeting space, and strong evacuation performance. Keep the natural-light
  side for desks.”
- “Optimize the current restaurant variant for fire evacuation. Keep all 20
  seats and the bar, use the existing furniture catalog, and tell me what
  design tradeoffs you made.”

## Acceptance checks

- A request for a 20-seat restaurant yields no more than three independent
  variants, each created from a preserved baseline.
- An overlap or blocked-exit error returned by Rust is never recommended as a
  valid result.
- Optimization compares only runs with the same scenario set and seed.
- The chosen recommendation includes required-capacity status, assumptions,
  and engine metrics; it does not claim certification.
