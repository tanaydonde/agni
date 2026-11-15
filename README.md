# agni

We spoke with our local fire department about issues they face with dealing with fire support requests.
Agni is our solution to their concerns. Firefighters can get a confident estimate of where the fire will spread
as well as the optimal route to resolve it.


Big ups the Costar and Matterport datasets. They were integral to our project
and helped acquire material data for accurate fire spread and data-rich 3D layouts, so we could plan best routes
for our firefighters.


## To run (data setup required)
```bash
cargo run -p tflop-server -- --scene data/prepared-scenes/<model-id>.json
pnpm install && pnpm --filter web dev
```
