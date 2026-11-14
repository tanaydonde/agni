# Product Requirements: Fire Preplan and Scenario Rehearsal

## Summary

tflop helps a fire officer prepare for a commercial building **before** an
emergency. For one property it combines three data sources into a visual
building brief:

| Source | What it contributes |
| --- | --- |
| CoStar | Property identity, address, building type and use, size, floors, year built, and other available building metadata. |
| Vexcel | Aerial and oblique imagery for exterior site context: building footprint, street frontage, apparatus approach, and candidate exterior entrances. |
| Matterport | Indoor scan geometry for the scanned portion of the building: rooms, walls, openings, and interior obstacles. |

An agent reads the joined record and proposes access points, known interior
routes, and potential obstacles. Every fact carries its source, capture date,
and verification status. Anything the data cannot establish is marked
**needs crew verification** rather than guessed.

A simulation then lets crews rehearse hypothetical fires in different
locations. Occupants and firefighter crews act in the known geometry, and
scenario comparisons reveal bottlenecks, inaccessible areas, and assumptions
that materially change the plan.

The product supports **preparation and training only**. Live tactical
decisions still depend on conditions observed at the incident and on the
department's procedures. The product never issues an attack order, picks a
single "optimal" entry point, or predicts casualties.

Background research and the reasoning behind these boundaries are in
`docs/FIRE_PREPLAN_RESEARCH.md`.

## User

The primary user is a **fire officer** (a company officer or preincident
planning officer) preparing for a specific commercial building. They know
fireground operations and department procedure; they do not have time to
assemble scan, imagery, and property data by hand. Secondary users are crews
running a training exercise from the officer's plan.

## Demo big picture

1. Select one commercial building with verified Matterport scan coverage.
2. Show the building brief: aerial/site view, property facts, and the bounded
   scanned interior. Unscanned areas are visibly marked as unknown.
3. The agent proposes access points, interior routes, and obstacles, and lists
   what still needs verification. The officer confirms, corrects, or rejects
   each item.
4. Run a hypothetical fire scenario: occupants evacuate and a crew performs an
   assigned search.
5. Change one thing (a different fire origin, or a blocked route) and rerun.
6. Compare the two scenarios side by side: route availability, bottlenecks,
   search coverage, timing ranges, and the assumptions each result depends on.

## 1. Property selection and data join

1. Ingest the supplied CoStar JSONL data using its documented join keys.
2. Search properties by location, property type, use, and size.
3. Join the property to its Vexcel imagery and Matterport scan entries.
4. Record what the scan covers. A scan of one suite or floor must never be
   presented as the whole building; a property-level match alone is
   insufficient.
5. Show which source supplied each fact and when it was captured.

Tenant history and listing data can provide context, but must not be treated
as confirmed current occupancy, occupant counts, or contents.

## 2. Building brief

The brief is a short, reviewable preplan card plus a 3D/2D view.

1. **Site view** from Vexcel imagery: footprint, adjacent streets, and
   candidate exterior entrances and apparatus approach.
2. **Interior view** from Matterport: the scanned floor(s), rooms, openings,
   and obstacles, with the scan boundary drawn clearly.
3. **Property facts** from CoStar: address, use, size, floors, construction
   year, and any available construction or protection fields.
4. **Agent findings**: candidate access points, known interior routes, and
   potential obstacles, each with a confidence note and source.
5. **Verification list**: facts the crew must check on a walkthrough. By
   default this includes unscanned areas, door operability and locking, exit
   discharge to the exterior, hydrants and water supply, standpipes and FDC,
   sprinkler and alarm systems, hazardous contents, and occupancy.

Every item has one of three statuses: `unverified` (agent-proposed),
`confirmed` (by a named officer, with date), or `rejected`. Only confirmed
exits and routes are treated as usable in simulations unless a scenario
explicitly assumes otherwise, and that assumption is shown in the results.

## 3. 3D view and scene editing

1. Load one supported Matterport space: room boundaries, openings, and the
   defurnished/textured mesh where available.
2. Navigate with orbit, pan, zoom, and fly controls; switch between
   perspective and top-down views.
3. Represent interior obstacles (furniture, storage, fixed equipment) as
   footprints that block movement. The existing furniture editor is reused to
   add, move, or remove obstacles so the officer can correct the model to match
   what a walkthrough found.
4. Mark exits, access points, and blocked routes directly in the view.
5. Keep structured scene state: geometry, obstacles, exits, verification
   status, and revision. Manual edits and agent tools update the same state.

## 4. Scenario rehearsal simulation

At a high level the simulation takes the building geometry, a hypothetical fire
origin, an occupant setup, and crew assignments, and steps them forward in
time. It is an API the agent and UI call; rendering is separate, and the
simulation runs whether or not it is displayed.

### Walkable geometry

- Derive walkable floor area from the prepared scene.
- Walls, fixed obstacles, and obstacle footprints are not walkable.
- Only confirmed exits are destinations unless a scenario assumes otherwise.

### Hazard

- A fire scenario places a hypothetical origin and grows a hazard zone over a
  scripted timeline that blocks or penalizes routes through it.
- Hazard zones are explicitly hypothetical. The product makes no fire-spread,
  smoke, tenability, or casualty predictions.

### Occupants

Occupants use behavior parameters and states, not personality stereotypes or
"panic":

- **States:** unaware → perceives cue → seeks information → prepares or helps
  → moves to a chosen exit → reroutes or exits.
- **Respond at different times:** sampled pre-movement delay and cue strength.
- **Favor familiar exits:** preference for the main entrance or the way they
  came in, rather than the nearest exit.
- **Move in groups:** groups travel together and wait for members.
- **Help others:** some occupants assist slower or mobility-impaired people.
- **Reroute:** when a route becomes blocked by hazard or congestion.
- Occupants collide with each other and with obstacles.

### Firefighter crews

Crews are modeled as task-driven teams, not shortest-path agents:

- A scenario begins with arrival; the crew enters through an assigned access
  point.
- Each crew has an assigned task (e.g. primary search of an area), stays
  together, and follows known routes.
- Crews report findings and **reassess** when conditions change: a route
  becomes unavailable, the hazard reaches their area, or the task completes.
- Task timing uses parameter ranges (ideally from the department's own drill
  data), not single values.

### Scenarios and comparison

- Vary fire origin, occupant starting positions and parameters, blocked or
  unavailable routes, and crew assignments.
- Use a fixed random seed per comparison and run several samples per scenario
  to show ranges rather than single numbers.
- Report: time-to-exit ranges, exit usage and bottlenecks, areas crews could
  not reach or search, task timing ranges, and occupants still inside at the
  scenario cutoff (never labeled as deaths).
- Highlight **sensitive assumptions**: inputs whose change materially alters
  the outcome, such as an unverified door or an assumed occupant count.

## 5. Preplan agent

The agent works over structured tools only; see `PREPLAN_AGENT.md`.

1. **Brief mode:** inspect the joined property record and scene, propose
   access points, routes, obstacles, and the verification list, and draft the
   preplan card.
2. **Rehearsal mode:** given the officer's goal (e.g. "what if the fire starts
   in the storage room?"), set up a small number of scenarios, run them,
   compare results, and explain bottlenecks, inaccessible areas, and which
   assumptions drove the difference.
3. When the officer corrects a fact (confirms an exit, blocks a route, adds an
   obstacle), the agent updates the brief and can rerun the same scenarios to
   show what changed.

The agent never presents a scenario result as a prediction of a real fire,
never ranks tactics for a live incident, and never marks an item confirmed on
its own.

## Minimum UI

- **Property screen:** search, select a building, and show source coverage.
- **Brief panel:** preplan card, agent findings, and the verification list
  with confirm/reject controls.
- **3D/2D view:** site context, scanned interior, exits, routes, obstacles,
  and the simulation playback.
- **Chat panel:** questions to the agent and scenario requests.
- **Scenario controls:** fire origin, occupant setup, crew assignments,
  blocked routes, run, and compare.
- **Results panel:** metrics with ranges, bottlenecks, unreachable areas,
  assumptions, and open verification items.

## MVP boundaries

- One commercial building with verified scan coverage for the primary demo.
- One floor per simulated space.
- Hypothetical hazard zones only; no validated fire or smoke modeling.
- One pedestrian/crew simulation engine.
- No live-incident mode, tactic ranking, casualty or tenability predictions,
  or code-compliance claims.
- Unscanned areas, unverified exits, and missing protection-system data are
  shown as unknowns, not filled in.

## Success

A fire officer can select a real building, see what the data does and does not
cover, confirm or correct the agent's proposed access points and routes, run
two hypothetical fire scenarios, and understand from the comparison which
bottlenecks, inaccessible areas, and assumptions matter most for their plan.
