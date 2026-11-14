// UI-only editor state: selection, the asset being dragged in from the
// palette, and an in-progress gizmo drag. The scene itself lives in the Rust
// world (see api.ts); nothing here is sent to the server until a drag ends.

import { create } from 'zustand'
import type { MoveHandle } from './placement'
import type { Vec3 } from './types'

export interface GizmoDrag {
  objectId: string
  handle: MoveHandle
  /** Object position when the drag started. */
  start: Vec3
  /** Where the pointer ray met the floor when the drag started. */
  startHit: Vec3
}

interface EditorState {
  selectedObjectId: string | null
  /** Asset being dragged in from the palette, and where it would land. */
  placing: { assetId: string; position: Vec3 | null } | null
  drag: GizmoDrag | null
  /** Live position of the dragged object; committed on release. */
  dragPosition: Vec3 | null
  /** Gizmo handle under the crosshair, for highlighting. */
  hoveredHandle: MoveHandle | null
  /**
   * While the pointer is locked the cursor is hidden; during a gizmo drag the
   * crosshair follows this virtual cursor, in pixels from the view center.
   */
  cursorOffset: { x: number; y: number }

  select: (objectId: string | null) => void
  startPlacing: (assetId: string) => void
  setPlacingPosition: (position: Vec3 | null) => void
  stopPlacing: () => void
  startDrag: (drag: GizmoDrag) => void
  updateDrag: (position: Vec3, cursorOffset: { x: number; y: number }) => void
  endDrag: () => void
  setHoveredHandle: (handle: MoveHandle | null) => void
}

export const useEditorStore = create<EditorState>()((set) => ({
  selectedObjectId: null,
  placing: null,
  drag: null,
  dragPosition: null,
  hoveredHandle: null,
  cursorOffset: { x: 0, y: 0 },

  select: (selectedObjectId) => set({ selectedObjectId }),
  startPlacing: (assetId) => set({ placing: { assetId, position: null } }),
  setPlacingPosition: (position) =>
    set((state) => (state.placing ? { placing: { ...state.placing, position } } : {})),
  stopPlacing: () => set({ placing: null }),
  startDrag: (drag) => set({ drag, dragPosition: drag.start, cursorOffset: { x: 0, y: 0 } }),
  updateDrag: (dragPosition, cursorOffset) => set({ dragPosition, cursorOffset }),
  endDrag: () => set({ drag: null, dragPosition: null, cursorOffset: { x: 0, y: 0 } }),
  setHoveredHandle: (hoveredHandle) =>
    set((state) => (state.hoveredHandle === hoveredHandle ? {} : { hoveredHandle })),
}))
