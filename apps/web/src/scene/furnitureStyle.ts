// How furniture looks until real GLB models exist: a colored box per category.

const CATEGORY_COLORS: Record<string, string> = {
  chair: '#d9a46c',
  desk: '#8fb3d9',
  meeting_table: '#93c29b',
  sofa: '#c99bc4',
  storage_cabinet: '#b9ad97',
}
const DEFAULT_COLOR = '#b0b8c4'

export const SELECTED_COLOR = '#635bff' // --color-accent
export const EDGE_COLOR = '#4f566b' // --color-ink-secondary

export function categoryColor(category: string): string {
  return CATEGORY_COLORS[category] ?? DEFAULT_COLOR
}
