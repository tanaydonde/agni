import { Canvas } from '@react-three/fiber'
import { useMemo } from 'react'
import { HEAT_RAMP, heatColor, heatmapImage, heatOpacity } from '../fire/heatmap'
import type { FireEnsembleAnalysis } from '../fire/types'
import { FireHeatmap } from '../scene/FireHeatmap'
import { Furniture } from '../scene/Furniture'
import type { FloorGeometry } from '../scene/geometry'
import { SceneModel } from '../scene/SceneModel'
import { TopDownCamera } from '../scene/TopDownCamera'
import type { Floor, Scene } from '../scene/types'

/** The legend's ramp, fading in as the map's cells do. */
const LEGEND_GRADIENT = `linear-gradient(to right, ${HEAT_RAMP.map((_, index) => {
  const t = index / (HEAT_RAMP.length - 1)
  const [r, g, b] = heatColor(t).map((c) => Math.round(c * 255))
  return `rgba(${r}, ${g}, ${b}, ${heatOpacity(t).toFixed(2)})`
}).join(', ')})`

/** No furniture is drawn see-through here: the heat is drawn over it. */
const NOTHING_BURNING: ReadonlySet<string> = new Set()

/**
 * Where fire is likely: the grey unfurnished floor plan from straight above,
 * furniture as coloured boxes (hatched and outlined over the heat), and every
 * simulation's fire summed over time and height into one cell each, coloured
 * and faded by heat so the rooms show through. The view doesn't move.
 */
export function HeatmapView({
  scene,
  floor,
  geometry,
  meshUrl,
  ensemble,
}: {
  scene: Scene
  floor: Floor
  geometry: FloorGeometry
  meshUrl?: string
  ensemble: FireEnsembleAnalysis
}) {
  const cellSizeM = ensemble.config.cell_size_m
  const image = useMemo(() => heatmapImage(ensemble.heatmap, cellSizeM), [ensemble.heatmap, cellSizeM])
  const count = ensemble.completed_simulation_count

  return (
    <div
      role="img"
      aria-label={`Fire heatmap of the floor plan across ${count} simulations`}
      className="absolute inset-0"
    >
      <Canvas orthographic frameloop="demand" dpr={[1, 2]}>
        <hemisphereLight args={['#ffffff', '#d5dbe1', 1.6]} />
        <directionalLight position={[8, 14, 6]} intensity={1.4} />
        <SceneModel geometry={geometry} meshUrl={meshUrl} />
        <Furniture
          floor={floor}
          assets={scene.assets}
          burning={NOTHING_BURNING}
          highlightSelection={false}
          hatched
        />
        {image && <FireHeatmap image={image} floorZ={geometry.floorZ} />}
        <TopDownCamera geometry={geometry} />
      </Canvas>
      <div className="absolute right-3 bottom-3 flex w-56 flex-col gap-1.5 rounded-lg border border-hairline bg-white px-3 py-2 text-xs text-ink-secondary shadow-[0_4px_12px_rgba(26,31,54,0.08)]">
        <span className="font-medium text-ink">Time on fire</span>
        <div
          aria-hidden
          className="h-2 rounded-sm"
          style={{ background: LEGEND_GRADIENT }}
        />
        <div className="flex justify-between font-mono tabular-nums">
          <span>0 s</span>
          <span>{Math.round(image?.hottestSeconds ?? 0)} s</span>
        </div>
        <span className="text-muted">
          {!image
            ? 'Nothing burned in these simulations.'
            : `Average per cell over ${count} simulation${count === 1 ? '' : 's'}, summed over height.`}
        </span>
      </div>
    </div>
  )
}
