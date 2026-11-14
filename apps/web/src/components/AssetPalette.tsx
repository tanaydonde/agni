import type { PointerEvent } from 'react'
import { useWorld } from '../scene/api'
import { useEditorStore } from '../scene/editorStore'
import { categoryColor, categoryLabel } from '../scene/furnitureStyle'

/**
 * The furniture catalog. Press on an item and drag it into the 3D view to
 * place it on the floor (see PlacementController).
 */
export function AssetPalette() {
  const { data: world } = useWorld()
  const startPlacing = useEditorStore((state) => state.startPlacing)
  const placingAssetId = useEditorStore((state) => state.placing?.assetId)
  const assets = world?.scene.assets ?? []

  const handlePointerDown = (assetId: string) => (event: PointerEvent) => {
    if (event.button !== 0) return
    event.preventDefault() // no text selection while dragging
    // Release mouse capture so pointer events reach the 3D view.
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId)
    }
    startPlacing(assetId)
  }

  return (
    <section
      aria-label="Furniture"
      className="flex flex-none flex-col gap-2 rounded-lg border border-hairline bg-white p-3"
    >
      <header className="flex items-baseline justify-between">
        <h2 className="text-sm font-medium text-ink">Furniture</h2>
        <span className="text-xs text-muted">Drag into the scene</span>
      </header>
      {assets.length === 0 ? (
        <p className="text-xs text-muted">No furniture catalog loaded.</p>
      ) : (
        <ul className="grid grid-cols-2 gap-2">
          {assets.map((asset) => {
            const { width_m, depth_m, height_m } = asset.dimensions_m
            return (
              <li key={asset.id}>
                <button
                  type="button"
                  onPointerDown={handlePointerDown(asset.id)}
                  className={`flex w-full cursor-grab touch-none items-center gap-2 rounded-md border px-2 py-1.5 text-left select-none hover:bg-canvas active:cursor-grabbing ${
                    placingAssetId === asset.id ? 'border-accent' : 'border-control-border'
                  }`}
                >
                  <span
                    aria-hidden
                    className="size-4 flex-none rounded-sm border border-ink/20"
                    style={{ background: categoryColor(asset.category) }}
                  />
                  <span className="min-w-0">
                    <span className="block truncate text-xs font-medium text-ink">
                      {categoryLabel(asset.category)}
                    </span>
                    <span className="block font-mono text-[10px] text-muted">
                      {width_m.toFixed(2)} × {depth_m.toFixed(2)} × {height_m.toFixed(2)} m
                    </span>
                  </span>
                </button>
              </li>
            )
          })}
        </ul>
      )}
    </section>
  )
}
