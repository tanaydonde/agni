Demo big picture
1. Load the default, unfurnished room.
2. Ask chatbot to generate an arbitrary room for a restaurant for 20 ppl
3. Run simulation to see how it fairs against fires (ideally it should fail/be super ass)
4. We optimize with "optimize" button and run simulation again, then compare results.

COSTAR STUFF FOR LATER, but keep here for now. 

CoStar search and property selection
1. Ingest the supplied JSONL data and documented join keys.
2. Search commercial lease listings using available fields:
   - Houston or Los Angeles.
   - Location or submarket.
   - Property type and marketed use.
   - Available area.
   - Asking rent and lease terms, where supplied.
3. Normalize asking terms before comparing costs. Show when expenses or other components are unknown.
4. Join listings and properties to Matterport scan entries.
5. Confirm that the scan represents the relevant suite or space; a property-level match alone is insufficient.
6. Use property data to narrow candidates, then use geometry to test the requested layout.
CoStar determines which real spaces are candidates. Matterport determines how the requested arrangement fits inside them. If the selected space cannot meet the requirements, the agent can propose changing the brief or inspecting another candidate.

Tenant history can provide context, but must not be treated as confirmed current occupancy or occupant counts.

THIS IS WHERE WE START
3D engine and editor
1. Load one supported Matterport space.
   - Use available room boundaries, openings, and/or mesh assets.
   - Prepare the room shell ahead of the demo.
   - Automatic furniture decomposition is not a confirmed Matterport capability and is outside the initial MVP.
2. Navigate the environment.
   - Orbit, pan, and zoom.
   - Switch between perspective and top-down views.
3. Support approximately five furniture types (we can add more later).
   - Desk.
   - Chair.
   - Meeting table.
   - Sofa.
   - Storage cabinet.
4. Edit individual objects.
   - Make sure when we display objects that it is the right size (matterport gives sizes)
   - Select, move, rotate, add, and remove furniture.
   - Use transform controls, with floor-constrained movement by default.
5. Maintain structured scene state.
   - Store object IDs, assets, dimensions, positions, rotations, and fixed/movable status.
   - Manual edits and agent tools update the same state.
6. Support layout versions.
   - Preserve the baseline.
   - Create independent alternatives.
   - Switch between versions, undo changes, reset, and save.
Simulation
At a high level, the simulation should take in a 3D layout and a fire starting point and simulate N people walking through.
We time how long it takes everyone to evacuate or return how many people died due to the fire.
In the future, we can add more emergency types other than fires. 
Logically speaking, the simulation is an API call that other things can make. The rendering of the simulation is a separate matter
that we can figure out later. So, all the pedestrians moving and fire spreading will happen regardless of it being displayed on the
engine frontend. 
1. Build walkable geometry.
   - Derive the floor area from the prepared scene.
   - Treat walls, fixed obstacles, and furniture footprints as non-walkable.
   - Define confirmed exit destinations. (i think matterport defines where the doors are)
2. Model pedestrian movement.
   - Occupants navigate around obstacles and interact through the chosen pedestrian model.
   - Model preference for familiar exits explicitly rather than adding arbitrary “irrational” movement.
   - Pedestrians should collide with each other (they can't walk through each other), and also shouldn't be able to walk through furniture
   - Within the simulation, they can be little oval blobs like Primer the youtuber. 
3. Test multiple scenarios.
   - Different occupant starting positions.
   - Different fire start locations
Deep Agent
At a high level, the deep agent will have two "modes":
    - The vibe-designing mode. The user can type english prompts, such as "design me a spatious interior for a small restaurant", and the 
      agent will use whatever tools it has available provided by the engine to generate/add chairs
    - Optimize mode. The user presses a button and provided a little bit of English context. The agent will create multiple layouts and use the
      simulation API to test these layouts to see how good they do in emergencies. The agent will return the best one. It needs some extra context as well because if, for example, it's a restaurant, it should place tables in a certain way. e.g. the layout of a resutaurant vs a office space is vastly different. 

1. Interpret the user’s requirements.
   - Location and rental budget.
   - Intended use and number of occupants.
   - Required furniture and functional areas.
   - Fixed objects and user preferences.
2. Generate constrained layouts.
   - Start with a few coherent arrangement patterns.
   - Preserve required furniture and seating.
   - Maintain configured clearances and avoid overlaps.
   - Keep related objects aligned and sensibly grouped.
3. Use structured tools.
   Tool	Purpose
   get_scene()	Inspect geometry, furniture, and scene version
   create_variant()	Create an independent layout
   apply_edits()	Add, move, rotate, or remove objects
   validate_layout()	Check functional and geometric constraints
   simulate_egress()	Run the selected scenario set
   compare_variants()	Compare results and identify tradeoffs
    Note that these tools are not final/exact. We may change the names or something.
4. Iterate using simulation feedback.
   - Identify bottlenecks.
   - Propose targeted edits.
   - Revalidate and rerun.
   - Stop after a configured iteration or time budget.
5. Preserve the original objective.
   - Do not improve evacuation time by removing required seats or abandoning the requested use.
   - Report unmet requirements explicitly.
   - Describe the result as the best-performing tested layout, not a globally optimal or certified safe layout.
Minimum UI
- Search screen: User brief, filters, listing results, and scan availability. (this is the COSTAR stuff, so it's TODO)
- Chat panel: Initial requirements and follow-up edits.
- Controls: Generate, run simulation, compare versions, reset, and save.
- Results panel: Functional requirements, simulation metrics, assumptions, and unresolved issues.
MVP boundaries
- One floor per simulated space.
- A small curated set of real listings with verified scan matches.
- One prepared space for the primary demo.
- Five furniture types
- One pedestrian simulation engine.
- No structural remodeling, validated fire-spread modeling, fatality predictions
Success means a user can find a real candidate property, generate a functional arrangement, watch a fair comparison, understand the tradeoffs, and save their preferred version.
