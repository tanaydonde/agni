// UI-only editor state: selection, the asset being dragged in from the
// palette, and an in-progress gizmo drag. The scene itself lives in the Rust
// world (see api.ts); nothing here is sent to the server until a drag ends.

import { create } from 'zustand'
import type { GizmoHandle } from './placement'
import type { Vec3 } from './types'

export interface GizmoDrag {
  objectId: string
  handle: GizmoHandle
  /** Object position and heading (degrees about Z) when the drag started. */
  start: Vec3
  startYaw: number
  /** Where the pointer ray met the floor when the drag started. */
  startHit: Vec3
}

/** Live placement of the dragged object; committed on release. */
export interface DragPreview {
  position: Vec3
  yaw: number
}

interface EditorState {
  selectedObjectId: string | null
  /** Asset being dragged in from the palette, and where it would land. */
  placing: { assetId: string; position: Vec3 | null } | null
  drag: GizmoDrag | null
  preview: DragPreview | null
  /** Gizmo handle under the cursor, for highlighting. */
  hoveredHandle: GizmoHandle | null

  select: (objectId: string | null) => void
  startPlacing: (assetId: string) => void
  setPlacingPosition: (position: Vec3 | null) => void
  stopPlacing: () => void
  startDrag: (drag: GizmoDrag) => void
  updateDrag: (preview: DragPreview) => void
  endDrag: () => void
  setHoveredHandle: (handle: GizmoHandle | null) => void
}

export const useEditorStore = create<EditorState>()((set) => ({
  selectedObjectId: null,
  placing: null,
  drag: null,
  preview: null,
  hoveredHandle: null,

  select: (selectedObjectId) => set({ selectedObjectId }),
  startPlacing: (assetId) => set({ placing: { assetId, position: null } }),
  setPlacingPosition: (position) =>
    set((state) => (state.placing ? { placing: { ...state.placing, position } } : {})),
  stopPlacing: () => set({ placing: null }),
  startDrag: (drag) => set({ drag, preview: { position: drag.start, yaw: drag.startYaw } }),
  updateDrag: (preview) => set({ preview }),
  endDrag: () => set({ drag: null, preview: null }),
  setHoveredHandle: (hoveredHandle) =>
    set((state) => (state.hoveredHandle === hoveredHandle ? {} : { hoveredHandle })),
}))
