# Fire spread: where fires start and how they grow

Status: draft, 2026-11-14. Feeds the fire simulation work for the firefighting
pivot. Like [FIRE_PREPLAN_RESEARCH.md](FIRE_PREPLAN_RESEARCH.md), the output
is a **hypothetical training scenario**, not a prediction of real fire
behaviour.

## Open decision: which engine

The repo currently points at two different fire models:

- [TECH_STACK.md §7](../TECH_STACK.md) specifies a material-tagged 2.5D
  cellular model (cells heat up, ignite neighbours, move smoke through doors).
- Tanay's work in progress exports Matterport rooms to **CFAST** (NIST's
  two-zone model: an upper hot-gas layer and a lower cool layer per room,
  connected through vents). None of it is pushed yet.

Pick one before writing more fire code. Everything below works with either
one: it describes *inputs* (where the fire starts, how fast it grows), not the
solver.

## 1. Where fires start in a home

US home fires by room of origin (NFPA, 2015–2020 reports):

| Room of origin | Share of home fires | Share of home fire deaths |
|---|---|---|
| Kitchen / cooking area | ~44% | ~17% |
| Bedroom | ~6% | ~25% |
| Living room / family room / den | ~3% | ~25% |

Cooking is the leading cause of home fires. Bedroom and living-room fires are
rare but kill far more people per fire. They tend to be smoking, heating or
electrical fires on upholstered furniture or mattresses, often while people
are asleep.

**Note on scope:** FIRE_PREPLAN_RESEARCH.md targets commercial buildings. The
numbers above are for homes. If the demo is a house, update the research doc.
If it's commercial, find stats by occupancy type (restaurant, office) before
using these.

### Choosing the origin in the sim

Support two modes:

1. **Weighted random:** pick a room with probability based on the table
   above. Good for running many scenarios.
2. **Worst case, picked by hand:** a living-room sofa or a bedroom at night.
   Better for a training rehearsal, because it's the case that kills people.

Implementation:

- Each `Room` already carries Matterport's `classifications { label,
  confidence }` ([model.rs](../crates/world-core/src/model.rs)). Map labels to
  weights: kitchen high; living room and bedroom medium; laundry and garage
  low. Multiply by `confidence`.
- **Not verified yet:** which label strings a real scan returns. Import a
  scene and print them before hardcoding the map.
- Inside the room, start the fire at a specific object, not the room's
  centre. The defurnished mesh has no appliances, and the furniture catalog
  has none either. Either add ignition-source items (stove, space heater,
  dryer, outlet) to the catalog, or let the user drop the start point. The
  sofa already exists and covers the living-room case.

## 2. Do different fires need different spreading techniques?

**No. Use one spread engine and pass it a different scenario preset.** Once a
fire is burning, how it spreads depends on the fuel, the ventilation (which
doors are open) and the room geometry. The cause doesn't change that physics.
What differs between scenarios is the starting inputs:

| Scenario | Growth | Peak heat output | Special behaviour |
|---|---|---|---|
| Kitchen grease fire | fast | medium | Local to the stovetop at first. Water makes it worse. |
| Upholstered sofa (cigarette) | smoulders for minutes, then ultra-fast | high | Heavy smoke early, before any flames |
| Bed / mattress | fast to ultra-fast | high | Same as sofa |
| Space heater next to stuff | medium to fast | depends on what's nearby | |
| Electrical, inside a wall | slow | low at first | Starts hidden in the wall cavity, out of view |
| Dryer | medium | medium | Lint fire, mostly stays in the laundry room |

### Growth curves

The standard way to model growth is a **t-squared curve**: heat release rate
`Q(t) = α · t²` until it reaches the scenario's peak, then hold until the
fuel runs out. The textbook growth classes:

| Class | α (kW/s²) | Time to reach 1,055 kW (1,000 BTU/s) |
|---|---|---|
| Slow | 0.00293 | 600 s |
| Medium | 0.01172 | 300 s |
| Fast | 0.0469 | 150 s |
| Ultra-fast | 0.1876 | 75 s |

The classes are defined by the time to reach 1,055 kW, not exactly 1 MW. At
1 MW the times are about 5% shorter (584 / 292 / 146 / 73 s).

For smouldering scenarios, add a delay before the curve starts and keep
smoke output high during the delay.

These α values were checked against a secondary source (a fire-growth
calculator and training material). Check them against the SFPE Handbook or
NFPA 72 Annex B before putting them in a slide.

### Suggested type

```rust
// Sketch only; not in world-core yet. Uses world-core's Vec3 and String room IDs.
pub struct FireScenario {
    pub origin_room_id: String,    // Room.id
    pub origin_point: Vec3,        // ignition object or user-placed point
    pub growth: GrowthClass,       // Slow | Medium | Fast | UltraFast
    pub peak_hrr_kw: f64,
    pub smolder_delay_s: f64,      // 0 for flaming starts
    pub smoke_yield: f64,          // relative; high for upholstery
    pub fire_class: FireClass,     // drives suppression, not spread
}
```

## 3. Where the fire type *does* matter: suppression

For a firefighting sim, the fire class should decide which actions work. It
shouldn't change how the fire spreads.

| Class | Fuel | Note |
|---|---|---|
| A | Ordinary materials: wood, fabric, paper | Water works |
| B | Flammable liquids | Water can spread it |
| C | Live electrical | Cut the power first; water is a shock hazard |
| K | Cooking oils / grease | Water causes a flare-up. Use a lid or a class K agent |

Model this as a rule table: applying an agent the class doesn't allow
increases heat release or spreads the fire, rather than reducing it.

## Sources

- [NFPA Home Structure Fires](https://www.maine.gov/future/sites/maine.gov.dps.fmo/files/inline-files/nfpa_home_structure_fires.pdf)
- [NFPA Journal, Summer 2023](https://www.nxtbook.com/nxtbooks/nfpa/journal_2023summer/index.php?startid=80)
- [NFPA home fires started by smoking](https://www.maine.gov/dps/fmo/sites/maine.gov.dps.fmo/files/inline-files/nfpa_home_fires_started_by_smoking.pdf)
- [Fire Engineering: NFPA structure fire deaths](https://www.fireengineering.com/fire-safety/nfpa-structure-fire-deaths/)
- [Fire growth rate calculator (t-squared α values)](https://codingace.net/eng/fire_growth_rate.html)
- [Heat release rate study guide](https://open-exam-prep.com/study-guides/fire-investigator/fire-behavior-dynamics/heat-release-rate)
- [NIST CFAST](https://pages.nist.gov/cfast/), [NIST FDS](https://pages.nist.gov/fds/)
