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

/** Server verdict on the latest dry-run of the placement being dragged. */
export interface LiveCheck {
  /** Identifies the checked edit; see `editKey`. */
  key: string
  valid: boolean
  /** Why it's invalid, for display. */
  message: string | null
}

interface EditorState {
  selectedObjectId: string | null
  /** Asset being dragged in from the palette, and where it would land. */
  placing: { assetId: string; position: Vec3 | null } | null
  drag: GizmoDrag | null
  preview: DragPreview | null
  /** Gizmo handle under the cursor, for highlighting. */
  hoveredHandle: GizmoHandle | null
  /** Validity of the current drag or palette placement; null when unknown. */
  liveCheck: LiveCheck | null
  /** A brief message, e.g. why the world rejected the last edit. */
  notice: { id: number; text: string } | null

  select: (objectId: string | null) => void
  startPlacing: (assetId: string) => void
  setPlacingPosition: (position: Vec3 | null) => void
  stopPlacing: () => void
  startDrag: (drag: GizmoDrag) => void
  updateDrag: (preview: DragPreview) => void
  endDrag: () => void
  setHoveredHandle: (handle: GizmoHandle | null) => void
  setLiveCheck: (check: LiveCheck | null) => void
  showNotice: (text: string) => void
  dismissNotice: (id: number) => void
}

let nextNoticeId = 1

export const useEditorStore = create<EditorState>()((set) => ({
  selectedObjectId: null,
  placing: null,
  drag: null,
  preview: null,
  hoveredHandle: null,
  liveCheck: null,
  notice: null,

  select: (selectedObjectId) => set({ selectedObjectId }),
  // A new drag starts fresh: drop any leftover verdict or notice.
  startPlacing: (assetId) => set({ placing: { assetId, position: null }, liveCheck: null, notice: null }),
  setPlacingPosition: (position) =>
    set((state) => (state.placing ? { placing: { ...state.placing, position } } : {})),
  stopPlacing: () => set({ placing: null, liveCheck: null }),
  startDrag: (drag) =>
    set({ drag, preview: { position: drag.start, yaw: drag.startYaw }, liveCheck: null, notice: null }),
  updateDrag: (preview) => set({ preview }),
  endDrag: () => set({ drag: null, preview: null, liveCheck: null }),
  setHoveredHandle: (hoveredHandle) =>
    set((state) => (state.hoveredHandle === hoveredHandle ? {} : { hoveredHandle })),
  setLiveCheck: (liveCheck) => set({ liveCheck }),
  showNotice: (text) => set({ notice: { id: nextNoticeId++, text } }),
  dismissNotice: (id) => set((state) => (state.notice?.id === id ? { notice: null } : {})),
}))
