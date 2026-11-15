// The batch's fire as a floor-plan heatmap: every simulation's burning cells,
// summed over time and height by the server (FireEnsembleAnalysis.heatmap).

import type { FireHeatmapCell } from './types'

/** Pale yellow → orange → deep red, light to dark so it also reads without hue. */
export const HEAT_RAMP = ['#ffe9a8', '#ffb347', '#ff6a1a', '#d92b1f', '#8c0f1f'] as const

/**
 * Opacity of the coolest and hottest cells. Faint heat stays see-through so
 * the rooms, walls and furniture under it read; even the hottest cell lets a
 * little of the furniture through.
 */
export const HEAT_OPACITY = { min: 0.3, max: 0.88 } as const

/**
 * The heatmap as an RGBA image, one pixel per floor cell, to stretch over the
 * floor plan. Row 0 is the lowest canonical y; column 0 the lowest x.
 */
export interface HeatmapImage {
  width: number
  height: number
  /** sRGB colour and linear alpha, 0–255, row by row. Unburned cells are clear. */
  data: Uint8Array
  /** Canonical (x, y) of the image's lowest corner, and its size, in metres. */
  originX: number
  originY: number
  widthM: number
  heightM: number
  /** Burning seconds of the hottest cell, the top of the scale. */
  hottestSeconds: number
}

/**
 * The cells as an image coloured and faded by heat: each cell's burning time
 * relative to the hottest cell picks both its colour on `HEAT_RAMP` and its
 * opacity. Null when nothing burned.
 */
export function heatmapImage(cells: FireHeatmapCell[], cellSizeM: number): HeatmapImage | null {
  const burned = cells.filter((cell) => cell.expected_burning_seconds > 0)
  if (burned.length === 0) return null
  const hottestSeconds = Math.max(...burned.map((cell) => cell.expected_burning_seconds))
  const minColumn = Math.min(...burned.map((cell) => cell.column))
  const minRow = Math.min(...burned.map((cell) => cell.row))
  const width = Math.max(...burned.map((cell) => cell.column)) - minColumn + 1
  const height = Math.max(...burned.map((cell) => cell.row)) - minRow + 1

  const data = new Uint8Array(width * height * 4)
  for (const cell of burned) {
    const intensity = cell.expected_burning_seconds / hottestSeconds
    const offset = ((cell.row - minRow) * width + (cell.column - minColumn)) * 4
    const [r, g, b] = heatColor(intensity)
    data[offset] = Math.round(r * 255)
    data[offset + 1] = Math.round(g * 255)
    data[offset + 2] = Math.round(b * 255)
    data[offset + 3] = Math.round(heatOpacity(intensity) * 255)
  }
  return {
    width,
    height,
    data,
    originX: minColumn * cellSizeM,
    originY: minRow * cellSizeM,
    widthM: width * cellSizeM,
    heightM: height * cellSizeM,
    hottestSeconds,
  }
}

/** Opacity at `intensity` (0–1), from `HEAT_OPACITY.min` to `.max`. */
export function heatOpacity(intensity: number): number {
  const t = Math.min(1, Math.max(0, intensity))
  return HEAT_OPACITY.min + (HEAT_OPACITY.max - HEAT_OPACITY.min) * t
}

/** The ramp colour at `intensity` (0–1) as linear-interpolated sRGB, each channel 0–1. */
export function heatColor(intensity: number): [number, number, number] {
  const t = Math.min(1, Math.max(0, intensity)) * (HEAT_RAMP.length - 1)
  const index = Math.min(HEAT_RAMP.length - 2, Math.floor(t))
  const from = channels(HEAT_RAMP[index])
  const to = channels(HEAT_RAMP[index + 1])
  const fraction = t - index
  return [0, 1, 2].map((c) => from[c] + (to[c] - from[c]) * fraction) as [number, number, number]
}

function channels(hex: string): [number, number, number] {
  const value = Number.parseInt(hex.slice(1), 16)
  return [(value >> 16) / 255, ((value >> 8) & 0xff) / 255, (value & 0xff) / 255]
}
