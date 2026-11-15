import { describe, expect, it } from 'vitest'
import { HEAT_OPACITY, HEAT_RAMP, heatColor, heatmapImage, heatOpacity } from './heatmap'

const cell = (column: number, row: number, expected_burning_seconds: number) => ({
  column,
  row,
  expected_burning_seconds,
  burned_fraction: 1,
})

describe('heatmapImage', () => {
  const pixel = (image: NonNullable<ReturnType<typeof heatmapImage>>, column: number, row: number) =>
    Array.from(image.data.slice((row * image.width + column) * 4, (row * image.width + column) * 4 + 4))

  it('spans the burned cells, one pixel each, from the lowest corner', () => {
    const image = heatmapImage([cell(0, 0, 10), cell(-2, 3, 2.5)], 0.25)!
    expect(image).toMatchObject({ width: 3, height: 4, originX: -0.5, originY: 0, widthM: 0.75, heightM: 1 })
    expect(image.hottestSeconds).toBe(10)
  })

  it('colours and fades each cell by its heat, leaving unburned cells clear', () => {
    const image = heatmapImage([cell(0, 0, 10), cell(-2, 3, 2.5)], 0.25)!
    const hottest = pixel(image, 2, 0)
    const cool = pixel(image, 0, 3)
    expect(hottest[3]).toBe(Math.round(HEAT_OPACITY.max * 255))
    expect(cool[3]).toBe(Math.round(heatOpacity(0.25) * 255))
    expect(cool[3]).toBeLessThan(hottest[3])
    expect(pixel(image, 1, 1)).toEqual([0, 0, 0, 0])
  })

  it('is null when no cell burned', () => {
    expect(heatmapImage([], 0.25)).toBeNull()
    expect(heatmapImage([cell(0, 0, 0)], 0.25)).toBeNull()
  })
})

describe('heatColor', () => {
  const hex = (color: [number, number, number]) =>
    `#${color.map((c) => Math.round(c * 255).toString(16).padStart(2, '0')).join('')}`

  it('runs from the first ramp colour to the last, clamped', () => {
    expect(hex(heatColor(0))).toBe(HEAT_RAMP[0])
    expect(hex(heatColor(1))).toBe(HEAT_RAMP.at(-1))
    expect(hex(heatColor(-1))).toBe(HEAT_RAMP[0])
    expect(hex(heatColor(2))).toBe(HEAT_RAMP.at(-1))
    expect(hex(heatColor(0.5))).toBe(HEAT_RAMP[2])
  })
})
