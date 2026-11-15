// UI-only editor state: which piece of furniture is selected. The selection
// is highlighted in the editor and is where a single fire simulation starts.
// The scene itself lives in the Rust world (see api.ts).

import { create } from 'zustand'

interface EditorState {
  selectedObjectId: string | null
  select: (objectId: string | null) => void
}

export const useEditorStore = create<EditorState>()((set) => ({
  selectedObjectId: null,
  select: (selectedObjectId) => set({ selectedObjectId }),
}))
