import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'
import {
  GRAPH_SHEET_MAX_RATIO,
  GRAPH_WIDE_MIN_PX,
  SHEET_SWIPE_PX,
  clampSheetBoxes,
  detailPlacement,
  fitCamera,
  graphDensity,
  inspectorChrome,
  legendLayout,
  nodeSummary,
  propertiesOpen,
  readGraphFrame,
  sheetAfterGesture,
  sheetMaxHeight,
  visibleGraphFrame,
} from './graph-chrome'

const bounds = { minX: -100, minY: -80, maxX: 100, maxY: 80 }

describe('graph pane breakpoint', () => {
  it('treats panes under 720px as narrow and the rest as wide', () => {
    expect(GRAPH_WIDE_MIN_PX).toBe(720)
    expect(graphDensity(0)).toBe('narrow')
    expect(graphDensity(370)).toBe('narrow')
    expect(graphDensity(719)).toBe('narrow')
    expect(graphDensity(719.9)).toBe('narrow')
    expect(graphDensity(720)).toBe('wide')
    expect(graphDensity(1200)).toBe('wide')
    expect(graphDensity(Number.NaN)).toBe('narrow')
  })

  it('keeps the container query on the same breakpoint and sheet cap', () => {
    const css = readFileSync(new URL('./styles.css', import.meta.url), 'utf8')
    expect(css).toContain('container-name: graph')
    expect(css).toContain('container-type: inline-size')
    expect(css).toContain('width < 720px')
    expect(css).toContain('max-height: 35%')
    expect(css).toContain('flex-wrap: nowrap')
    expect(css).toMatch(/\.graph-inspector \{[^}]*height: auto/)
    expect(css).not.toMatch(/\.graph-inspector \{[^}]*bottom: 8px/)
    expect(GRAPH_SHEET_MAX_RATIO).toBe(0.35)
  })

  it('sizes the wide side card to content and caps a narrow sheet at 35%', () => {
    expect(detailPlacement('wide', 'compact', true)).toBe('side')
    expect(detailPlacement('wide', 'dismissed', true)).toBe('side')
    expect(detailPlacement('wide', 'chip', false)).toBe('side')
    const side = inspectorChrome('side', 640)
    expect(side).toEqual({ anchor: 'right', height: 'content', maxHeightPx: 624 })
    expect(side.maxHeightPx).toBeLessThan(640)

    expect(detailPlacement('narrow', 'compact', true)).toBe('sheet')
    expect(detailPlacement('narrow', 'expanded', true)).toBe('sheet')
    expect(detailPlacement('narrow', 'chip', true)).toBe('chip')
    expect(detailPlacement('narrow', 'dismissed', true)).toBe('hidden')
    expect(detailPlacement('narrow', 'compact', false)).toBe('hidden')

    const stage = 500
    const sheet = inspectorChrome('sheet', stage)
    expect(sheet.height).toBe('capped')
    expect(sheet.maxHeightPx).toBeCloseTo(sheetMaxHeight(stage))
    expect(sheet.maxHeightPx / stage).toBeLessThanOrEqual(0.35)
    expect(inspectorChrome('chip', stage).maxHeightPx).toBe(40)
    expect(inspectorChrome('hidden', stage).anchor).toBe('none')
  })

  it('collapses the legend to one compact row in a narrow pane', () => {
    expect(legendLayout('wide')).toBe('labeled')
    expect(legendLayout('narrow')).toBe('compact')
  })

  it('shows properties on the side card and only after expand on the sheet', () => {
    expect(propertiesOpen('wide', 'compact')).toBe(true)
    expect(propertiesOpen('narrow', 'compact')).toBe(false)
    expect(propertiesOpen('narrow', 'expanded')).toBe(true)
    expect(propertiesOpen('narrow', 'chip')).toBe(false)
  })
})

describe('graph sheet gestures', () => {
  it('expands on swipe up or click and collapses toward a chip on swipe down', () => {
    expect(sheetAfterGesture('compact', 0)).toBe('expanded')
    expect(sheetAfterGesture('compact', -(SHEET_SWIPE_PX - 1))).toBe('expanded')
    expect(sheetAfterGesture('compact', -SHEET_SWIPE_PX)).toBe('expanded')
    expect(sheetAfterGesture('compact', SHEET_SWIPE_PX)).toBe('chip')
    expect(sheetAfterGesture('expanded', SHEET_SWIPE_PX)).toBe('compact')
    expect(sheetAfterGesture('expanded', 0)).toBe('compact')
    expect(sheetAfterGesture('chip', 0)).toBe('compact')
    expect(sheetAfterGesture('chip', -SHEET_SWIPE_PX)).toBe('compact')
    expect(sheetAfterGesture('dismissed', -40)).toBe('dismissed')
    expect(sheetAfterGesture('dismissed', 0)).toBe('dismissed')
  })
})

describe('graph detail summary', () => {
  it('shows name, type, and number for an object', () => {
    expect(
      nodeSummary(
        {
          caption: '2-person tent',
          subtitle: 'Article',
          kindLabel: 'Object',
          properties: [
            { label: 'Type', value: 'Article' },
            { label: 'Number', value: 'ART-TALT-2' },
          ],
        },
        { type: 'Type', number: 'Number' },
      ),
    ).toEqual({ name: '2-person tent', type: 'Article', detail: 'ART-TALT-2' })
  })

  it('uses the value text when a node has no number', () => {
    expect(
      nodeSummary(
        {
          caption: 'Colour',
          subtitle: 'Green',
          kindLabel: 'Value',
          properties: [{ label: 'Value', value: 'Green' }],
        },
        { type: 'Type', number: 'Number' },
      ),
    ).toEqual({ name: 'Colour', type: 'Value', detail: 'Green' })
  })
})

describe('visible graph frame', () => {
  it('insets a short side card from the right and the legend from the bottom', () => {
    const stage = { width: 1000, height: 600 }
    const frame = visibleGraphFrame(stage, [
      { x: 752, y: 8, width: 240, height: 160 },
      { x: 8, y: 552, width: 480, height: 40 },
    ])
    expect(frame).toEqual({ x: 0, y: 0, width: 752, height: 552 })
  })

  it('insets a narrow pane for a top legend and a bottom sheet', () => {
    const stage = { width: 370, height: 640 }
    const frame = visibleGraphFrame(stage, [
      { x: 8, y: 8, width: 180, height: 32 },
      { x: 82, y: 460, width: 280, height: 172 },
    ])
    expect(frame.y).toBe(40)
    expect(frame.height).toBe(420)
    expect(frame.width).toBe(370)
    const covered = stage.height - (frame.y + frame.height)
    expect(covered).toBeLessThanOrEqual(stage.height * GRAPH_SHEET_MAX_RATIO + 12)
  })

  it('clamps a tall narrow sheet to 35% before fitting', () => {
    const stage = { width: 370, height: 600 }
    const boxes = clampSheetBoxes(
      stage,
      [
        { x: 8, y: 8, width: 180, height: 32 },
        { x: 82, y: 88, width: 280, height: 504 },
      ],
      'narrow',
    )
    const sheet = boxes[1]
    expect(sheet?.height).toBeCloseTo(210)
    expect(sheet!.height / stage.height).toBeLessThanOrEqual(0.35)
    const frame = readGraphFrame(stage, boxes, 'narrow')
    const covered = stage.height - (frame.y + frame.height)
    expect(covered).toBeLessThanOrEqual(stage.height * 0.35 + EDGE_SLACK)
  })

  it('leaves most of the pane free when details are a chip or dismissed', () => {
    const stage = { width: 370, height: 500 }
    const chip = visibleGraphFrame(stage, [
      { x: 8, y: 8, width: 160, height: 28 },
      { x: 220, y: 464, width: 140, height: 28 },
    ])
    expect(chip.height).toBeGreaterThan(stage.height * 0.8)
    const dismissed = visibleGraphFrame(stage, [{ x: 8, y: 8, width: 160, height: 28 }])
    expect(dismissed.height).toBeGreaterThan(chip.height)
  })

  it('does not clamp the wide side card', () => {
    const stage = { width: 960, height: 600 }
    const card = { x: 712, y: 8, width: 240, height: 400 }
    expect(clampSheetBoxes(stage, [card], 'wide')[0]).toEqual(card)
  })

  it('moves Fit into the free area when the sheet covers the bottom', () => {
    const stage = { width: 370, height: 640 }
    const open = readGraphFrame(stage, [{ x: 8, y: 8, width: 180, height: 32 }], 'narrow')
    const covered = readGraphFrame(
      stage,
      [
        { x: 8, y: 8, width: 180, height: 32 },
        { x: 82, y: 400, width: 280, height: 232 },
      ],
      'narrow',
    )
    const before = fitCamera({ frame: open, bounds })
    const after = fitCamera({ frame: covered, bounds })
    expect(after.y).toBeLessThan(before.y)
    const centerY = after.y + ((bounds.minY + bounds.maxY) / 2) * after.k
    expect(centerY).toBeGreaterThan(covered.y)
    expect(centerY).toBeLessThan(covered.y + covered.height)
  })

  it('moves Fit left of a wide side card', () => {
    const stage = { width: 960, height: 600 }
    const clear = fitCamera({ frame: visibleGraphFrame(stage, []), bounds })
    const beside = fitCamera({
      frame: visibleGraphFrame(stage, [{ x: 712, y: 8, width: 240, height: 180 }]),
      bounds,
    })
    expect(beside.x).toBeLessThan(clear.x)
    const centerX = beside.x
    expect(centerX).toBeLessThan(712)
  })

  it('centers the bounds in the padded free frame', () => {
    const frame = { x: 0, y: 40, width: 370, height: 300 }
    const camera = fitCamera({ frame, bounds: { minX: -50, minY: -20, maxX: 50, maxY: 20 } })
    expect(camera.x).toBeCloseTo(frame.x + 16 + (frame.width - 32) / 2)
    expect(camera.y).toBeCloseTo(frame.y + 16 + (frame.height - 32) / 2)
  })
})

const EDGE_SLACK = 12
