/** Pane chrome for the graph. Width is the graph pane, not the viewport. */

export const GRAPH_WIDE_MIN_PX = 720
export const GRAPH_SHEET_MAX_RATIO = 0.35
export const GRAPH_CHIP_MAX_PX = 40
export const SHEET_SWIPE_PX = 28
const EDGE_PX = 12

export type GraphDensity = 'wide' | 'narrow'
export type SheetMode = 'compact' | 'expanded' | 'chip' | 'dismissed'
export type DetailPlacement = 'side' | 'sheet' | 'chip' | 'hidden'
export type LegendLayout = 'labeled' | 'compact'

export type Box = { x: number; y: number; width: number; height: number }
export type GraphFrame = { x: number; y: number; width: number; height: number }
export type Bounds = { minX: number; minY: number; maxX: number; maxY: number }

export function graphDensity(width: number): GraphDensity {
  if (!Number.isFinite(width) || width < GRAPH_WIDE_MIN_PX) return 'narrow'
  return 'wide'
}

export function sheetMaxHeight(stageHeight: number): number {
  if (!Number.isFinite(stageHeight) || stageHeight <= 0) return 0
  return stageHeight * GRAPH_SHEET_MAX_RATIO
}

export function legendLayout(density: GraphDensity): LegendLayout {
  return density === 'narrow' ? 'compact' : 'labeled'
}

/** Wide: side card. Narrow: bottom sheet, chip, or nothing when dismissed / empty. */
export function detailPlacement(density: GraphDensity, sheet: SheetMode, hasNode: boolean): DetailPlacement {
  if (density === 'wide') return 'side'
  if (!hasNode || sheet === 'dismissed') return 'hidden'
  if (sheet === 'chip') return 'chip'
  return 'sheet'
}

export function propertiesOpen(density: GraphDensity, sheet: SheetMode): boolean {
  if (density === 'wide') return true
  return sheet === 'expanded'
}

export type InspectorChrome = {
  anchor: 'right' | 'bottom' | 'corner' | 'none'
  /** Content height, capped by maxHeightPx. Never stretch to the stage. */
  height: 'content' | 'capped'
  maxHeightPx: number
}

export function inspectorChrome(placement: DetailPlacement, stageHeight: number): InspectorChrome {
  const height = Number.isFinite(stageHeight) ? Math.max(0, stageHeight) : 0
  if (placement === 'hidden') return { anchor: 'none', height: 'content', maxHeightPx: 0 }
  if (placement === 'chip') return { anchor: 'corner', height: 'content', maxHeightPx: GRAPH_CHIP_MAX_PX }
  if (placement === 'sheet') return { anchor: 'bottom', height: 'capped', maxHeightPx: sheetMaxHeight(height) }
  return { anchor: 'right', height: 'content', maxHeightPx: Math.max(0, height - 16) }
}

/**
 * Swipe up reveals more. Swipe down collapses toward a chip.
 * A small movement is a click: compact toggles properties, a chip opens the card.
 */
export function sheetAfterGesture(sheet: SheetMode, deltaY: number): SheetMode {
  if (sheet === 'dismissed') return sheet
  if (deltaY <= -SHEET_SWIPE_PX) {
    if (sheet === 'chip') return 'compact'
    return 'expanded'
  }
  if (deltaY >= SHEET_SWIPE_PX) {
    if (sheet === 'expanded') return 'compact'
    return 'chip'
  }
  if (sheet === 'expanded') return 'compact'
  if (sheet === 'chip') return 'compact'
  return 'expanded'
}

export function nodeSummary(
  node: { caption: string; subtitle: string; kindLabel: string; properties: Array<{ label: string; value: string }> },
  labels: { type: string; number: string },
): { name: string; type: string; detail: string } {
  const typed = node.properties.find((item) => item.label === labels.type)?.value
  const number = node.properties.find((item) => item.label === labels.number)?.value ?? ''
  const type = typed || node.kindLabel
  const subtitle = node.subtitle.trim()
  const detail = number || (subtitle && subtitle !== type && subtitle !== node.caption ? subtitle : '')
  return { name: node.caption, type, detail }
}

/** Pull a narrow bottom sheet down to the 35% cap. Short chips and top legends stay put. */
export function clampSheetBoxes(stage: { width: number; height: number }, boxes: Box[], density: GraphDensity): Box[] {
  if (density !== 'narrow') return boxes
  const maxH = sheetMaxHeight(stage.height)
  if (maxH <= 0) return boxes
  return boxes.map((box) => {
    if (box.width <= 0 || box.height <= 0) return box
    const gapBottom = stage.height - (box.y + box.height)
    if (gapBottom > EDGE_PX || box.height <= maxH + 0.5) return box
    const height = maxH
    return { ...box, y: stage.height - Math.max(0, gapBottom) - height, height }
  })
}

/**
 * Free rectangle after edge-anchored chrome.
 * A corner box insets the edge it costs less area to give up,
 * so a side card shrinks width and a bottom sheet shrinks height.
 */
export function visibleGraphFrame(stage: { width: number; height: number }, chrome: Box[]): GraphFrame {
  let left = 0
  let top = 0
  let right = Math.max(0, stage.width)
  let bottom = Math.max(0, stage.height)
  for (const box of chrome) {
    if (box.width <= 0 || box.height <= 0) continue
    if (box.x + box.width <= 0 || box.y + box.height <= 0) continue
    if (box.x >= stage.width || box.y >= stage.height) continue
    const edge = anchorEdge(box, stage)
    if (edge === 'right') right = Math.min(right, box.x)
    else if (edge === 'left') left = Math.max(left, box.x + box.width)
    else if (edge === 'bottom') bottom = Math.min(bottom, box.y)
    else top = Math.max(top, box.y + box.height)
  }
  if (right < left) right = left
  if (bottom < top) bottom = top
  return { x: left, y: top, width: Math.max(1, right - left), height: Math.max(1, bottom - top) }
}

export function readGraphFrame(
  stage: { width: number; height: number },
  boxes: Box[],
  density: GraphDensity,
): GraphFrame {
  return visibleGraphFrame(stage, clampSheetBoxes(stage, boxes, density))
}

function anchorEdge(box: Box, stage: { width: number; height: number }): 'left' | 'right' | 'top' | 'bottom' {
  const gap = {
    left: Math.max(0, box.x),
    right: Math.max(0, stage.width - (box.x + box.width)),
    top: Math.max(0, box.y),
    bottom: Math.max(0, stage.height - (box.y + box.height)),
  }
  const area = {
    left: (box.x + box.width) * stage.height,
    right: (stage.width - box.x) * stage.height,
    top: (box.y + box.height) * stage.width,
    bottom: (stage.height - box.y) * stage.width,
  }
  const edges = ['left', 'right', 'top', 'bottom'] as const
  const touching = edges.filter((edge) => gap[edge] <= EDGE_PX)
  const pool = [...(touching.length > 0 ? touching : edges)]
  pool.sort((left, right) => area[left] - area[right] || gap[left] - gap[right])
  return pool[0] ?? 'bottom'
}

/** Place the camera so the bounds sit in the free frame, not under the chrome. */
export function fitCamera(input: {
  frame: GraphFrame
  bounds: Bounds
  pad?: number
  minScale?: number
  maxScale?: number
}): { k: number; x: number; y: number } {
  const pad = input.pad ?? 16
  const viewW = Math.max(1, input.frame.width - pad * 2)
  const viewH = Math.max(1, input.frame.height - pad * 2)
  const boundsW = Math.max(1, input.bounds.maxX - input.bounds.minX)
  const boundsH = Math.max(1, input.bounds.maxY - input.bounds.minY)
  const fitted = Math.min(viewW / boundsW, viewH / boundsH)
  const k = Math.min(input.maxScale ?? 1.25, Math.max(input.minScale ?? 0.35, fitted))
  const cx = (input.bounds.minX + input.bounds.maxX) / 2
  const cy = (input.bounds.minY + input.bounds.maxY) / 2
  return {
    k,
    x: input.frame.x + pad + viewW / 2 - cx * k,
    y: input.frame.y + pad + viewH / 2 - cy * k,
  }
}
