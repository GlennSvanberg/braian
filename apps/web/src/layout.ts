/**
 * Tiling layout. The JSON shape matches P360 Client v4 (`layout.rs`):
 * a split carries `dir`, `ratios` (sum 1) and `children`, a tab group
 * carries `views` and a numeric `active` index.
 *
 * `horizontal` is side by side. `vertical` is stacked.
 */

export type Zone = 'left' | 'right' | 'top' | 'bottom' | 'center'
export type Dir = 'horizontal' | 'vertical'

export type SplitNode = {
  type: 'split'
  dir: Dir
  ratios: number[]
  children: Node[]
}

export type TabsNode = {
  type: 'tabs'
  views: string[]
  active: number
}

export type Node = SplitNode | TabsNode

export type LayoutDoc = {
  id: string
  title: string
  layout: Node
  focus?: string
  zoom?: string
  createdBy?: string
}

export type Place = {
  target?: string
  zone?: Zone
  dock?: 'edge'
  share?: number
  tabIndex?: number
}

export type Rect = { x: number; y: number; w: number; h: number }

export type Leaf = {
  views: string[]
  active: number
  rect: Rect
  path: number[]
}

export type LayoutErrorCode = 'missing-view' | 'missing-target' | 'target-removed'

export class LayoutError extends Error {
  readonly code: LayoutErrorCode

  constructor(code: LayoutErrorCode, message: string) {
    super(message)
    this.name = 'LayoutError'
    this.code = code
  }
}

const DIVIDER_MIN = 0.04
const RESIZE_MIN = 0.05
const NEIGHBOUR_TOL = 1e-3

export function tabGroup(ids: string[], active = 0): TabsNode {
  return { type: 'tabs', views: ids, active }
}

export function splitPair(dir: Dir, ratio: number, first: Node, second: Node): SplitNode {
  const firstRatio = roundRatio(ratio)
  return { type: 'split', dir, ratios: [firstRatio, roundRatio(1 - firstRatio)], children: [first, second] }
}

export function isSplit(node: Node): node is SplitNode {
  return node.type === 'split'
}

export function views(node: Node): string[] {
  if (node.type === 'tabs') return [...node.views]
  return node.children.flatMap(views)
}

export function leaves(node: Node): Leaf[] {
  const out: Leaf[] = []
  walkLeaves(node, { x: 0, y: 0, w: 1, h: 1 }, [], out)
  return out
}

export function normalize(node: Node): Node {
  if (node.type === 'tabs') {
    const list = node.views.filter((id) => id.length > 0)
    const active = list.length === 0 ? 0 : clamp(Math.trunc(node.active) || 0, 0, list.length - 1)
    return { type: 'tabs', views: list, active }
  }
  const kept: Array<{ child: Node; ratio: number }> = []
  node.children.forEach((child, index) => {
    const next = normalize(child)
    if (views(next).length === 0) return
    const ratio = node.ratios[index]
    kept.push({ child: next, ratio: typeof ratio === 'number' && ratio > 0 && Number.isFinite(ratio) ? ratio : 0 })
  })
  if (kept.length === 0) return { type: 'tabs', views: [], active: 0 }
  if (kept.length === 1) return kept[0].child
  const sum = kept.reduce((total, item) => total + item.ratio, 0)
  const ratios = sumToOne(sum > 0 ? kept.map((item) => item.ratio / sum) : kept.map(() => 1 / kept.length))
  return { type: 'split', dir: node.dir, ratios, children: kept.map((item) => item.child) }
}

export function remove(node: Node, view: string): Node {
  return normalize(removeFrom(node, view))
}

export function activate(node: Node, view: string): Node {
  if (node.type === 'tabs') {
    const index = node.views.indexOf(view)
    if (index < 0) return node
    return { ...node, active: index }
  }
  return { ...node, children: node.children.map((child) => activate(child, view)) }
}

/** Centre adds `view` as the active tab. An edge replaces the target pane with a fresh 0.5 split and does not flatten into a same-direction parent. */
export function insert(node: Node, target: string, view: string, zone: Zone): Node {
  const path = findPath(node, target)
  if (!path) {
    if (views(node).length === 0) return { type: 'tabs', views: [view], active: 0 }
    throw new LayoutError('missing-target', 'target is not in the layout')
  }
  const pane = nodeAt(node, path)
  if (!pane || pane.type !== 'tabs') throw new LayoutError('missing-target', 'target is not in the layout')
  if (zone === 'center') return replaceAt(node, path, addTab(pane, view))
  const rest = removeViewFromPane(pane, view)
  const fresh: TabsNode = { type: 'tabs', views: [view], active: 0 }
  const dir: Dir = zone === 'left' || zone === 'right' ? 'horizontal' : 'vertical'
  const lead = zone === 'left' || zone === 'top'
  const split: SplitNode = {
    type: 'split',
    dir,
    ratios: [0.5, 0.5],
    children: lead ? [fresh, rest] : [rest, fresh],
  }
  return normalize(replaceAt(node, path, split))
}

/**
 * Remove `view`, then insert it beside `target`.
 * Dropping the view onto its own tab group is a no-op.
 * Removing the view must leave `target` in the tree.
 */
export function moveView(node: Node, view: string, target: string, zone: Zone): Node {
  if (!views(node).includes(view)) throw new LayoutError('missing-view', 'view is not in the layout')
  const pane = findTabs(node, target)
  if (zone === 'center' && pane?.views.includes(view)) return node
  const without = remove(node, view)
  if (!views(without).includes(target)) throw new LayoutError('target-removed', 'removal removed target')
  return insert(without, target, view, zone)
}

/** Wrap the whole tree in a new root split. `share` is clamped to 0.1–0.9 and defaults to 0.3. */
export function dockEdge(node: Node, view: string, zone: Zone, share = 0.3): Node {
  const portion = roundRatio(clamp(Number.isFinite(share) ? share : 0.3, 0.1, 0.9))
  const rest = remove(node, view)
  const fresh: TabsNode = { type: 'tabs', views: [view], active: 0 }
  if (views(rest).length === 0) return fresh
  if (zone === 'center') {
    const anchor = views(rest)[0]
    return anchor ? activate(insert(rest, anchor, view, 'center'), view) : fresh
  }
  const other = roundRatio(1 - portion)
  const dir: Dir = zone === 'left' || zone === 'right' ? 'horizontal' : 'vertical'
  const lead = zone === 'left' || zone === 'top'
  return {
    type: 'split',
    dir,
    ratios: lead ? [portion, other] : [other, portion],
    children: lead ? [fresh, rest] : [rest, fresh],
  }
}

/** Move boundary `index` to absolute fraction `position` inside that split. Only the two neighbours change, and each stays at least 0.04. */
export function setDivider(node: Node, path: number[], index: number, position: number): Node {
  const split = nodeAt(node, path)
  if (!split || split.type !== 'split') return node
  if (index < 0 || index >= split.ratios.length - 1) return node
  const ratios = split.ratios.slice()
  const pair = ratios[index] + ratios[index + 1]
  const before = ratios.slice(0, index).reduce((total, ratio) => total + ratio, 0)
  let left = position - before
  if (!(pair > DIVIDER_MIN * 2)) left = pair / 2
  else left = clamp(left, DIVIDER_MIN, pair - DIVIDER_MIN)
  ratios[index] = roundRatio(left)
  ratios[index + 1] = roundRatio(pair - ratios[index])
  return replaceAt(node, path, { ...split, ratios })
}

/** Grow or shrink the nearest split of `dir` that contains `view`. Each side stays at least 0.05. */
export function resize(node: Node, view: string, dir: Dir, delta: number): Node {
  const located = nearestSplit(node, view, dir)
  if (!located) return node
  const split = nodeAt(node, located.path)
  if (!split || split.type !== 'split') return node
  const other = located.childIndex + 1 < split.children.length ? located.childIndex + 1 : located.childIndex - 1
  if (other < 0) return node
  const ratios = split.ratios.slice()
  const pair = ratios[located.childIndex] + ratios[other]
  const floor = Math.min(RESIZE_MIN, pair / 2)
  const next = roundRatio(clamp(ratios[located.childIndex] + delta, floor, pair - floor))
  ratios[located.childIndex] = next
  ratios[other] = roundRatio(pair - next)
  return replaceAt(node, located.path, { ...split, ratios })
}

/** The visible pane fully on `zone`'s side of `view` (tolerance 1e-3) whose centre is nearest. Returns that pane's active view. */
export function neighbour(node: Node, view: string, zone: Zone): string | null {
  if (zone === 'center') return null
  const all = leaves(node).filter((leaf) => leaf.views.length > 0)
  const self = all.find((leaf) => leaf.views.includes(view))
  if (!self) return null
  const selfCentre = centre(self.rect)
  let best: { id: string; score: number; ortho: number; y: number; x: number } | null = null
  for (const leaf of all) {
    if (leaf === self) continue
    if (!fullyOnSide(self.rect, leaf.rect, zone)) continue
    const point = centre(leaf.rect)
    const dx = point.x - selfCentre.x
    const dy = point.y - selfCentre.y
    const score = dx * dx + dy * dy
    const ortho = zone === 'left' || zone === 'right' ? Math.abs(dy) : Math.abs(dx)
    const id = leaf.views[leaf.active] ?? leaf.views[0]
    if (!id) continue
    const candidate = { id, score, ortho, y: point.y, x: point.x }
    if (
      !best ||
      candidate.score < best.score - 1e-9 ||
      (Math.abs(candidate.score - best.score) <= 1e-9 &&
        (candidate.ortho < best.ortho - 1e-9 ||
          (Math.abs(candidate.ortho - best.ortho) <= 1e-9 && (candidate.y < best.y || (candidate.y === best.y && candidate.x < best.x)))))
    ) {
      best = candidate
    }
  }
  return best?.id ?? null
}

/**
 * Alone in its pane with a neighbour: swap.
 * Sharing the pane with a neighbour: join that neighbour as a tab.
 * Otherwise dock against the workspace edge at share 0.5.
 */
export function moveDir(node: Node, view: string, zone: Zone): Node {
  if (zone === 'center') return node
  const pane = findTabs(node, view)
  if (!pane) return node
  const next = neighbour(node, view, zone)
  const alone = pane.views.length === 1
  if (alone && next) return swap(node, view, next)
  if (!alone && next) return moveView(node, view, next, 'center')
  return dockEdge(node, view, zone, 0.5)
}

export function swap(node: Node, a: string, b: string): Node {
  if (a === b) return node
  if (!views(node).includes(a) || !views(node).includes(b)) return node
  return swapIds(node, a, b)
}

export function setTabIndex(node: Node, view: string, index: number): Node {
  if (node.type === 'tabs') {
    if (!node.views.includes(view)) return node
    const current = node.views[node.active] ?? view
    const rest = node.views.filter((id) => id !== view)
    const at = clamp(Math.trunc(index) || 0, 0, rest.length)
    rest.splice(at, 0, view)
    const active = Math.max(0, rest.indexOf(current))
    return { type: 'tabs', views: rest, active }
  }
  return { ...node, children: node.children.map((child) => setTabIndex(child, view, index)) }
}

export function placeView(doc: LayoutDoc, view: string, place: Place = {}): LayoutDoc {
  let layout = doc.layout
  if (place.dock === 'edge') {
    const zone = place.zone && place.zone !== 'center' ? place.zone : 'right'
    layout = dockEdge(layout, view, zone, place.share ?? 0.3)
  } else {
    const zone = place.zone ?? 'right'
    const open = views(layout)
    const others = open.filter((id) => id !== view)
    const focus = doc.focus && others.includes(doc.focus) ? doc.focus : undefined
    const given = place.target && (place.target === view || open.includes(place.target)) ? place.target : undefined
    const target = given ?? focus ?? others[0]
    if (!target) layout = { type: 'tabs', views: [view], active: 0 }
    else if (open.includes(view)) layout = moveView(layout, view, target, zone)
    else layout = insert(layout, target, view, zone)
  }
  if (typeof place.tabIndex === 'number') layout = setTabIndex(layout, view, place.tabIndex)
  layout = activate(layout, view)
  return { ...doc, layout, focus: view }
}

export function ensureInLayout(doc: LayoutDoc, view: string, target?: string): LayoutDoc {
  const open = views(doc.layout)
  if (open.includes(view)) return { ...doc, layout: activate(doc.layout, view), focus: view }
  if (open.length === 0) return { ...doc, layout: { type: 'tabs', views: [view], active: 0 }, focus: view }
  const anchor =
    (target && open.includes(target) ? target : undefined) ??
    (doc.focus && open.includes(doc.focus) ? doc.focus : undefined) ??
    open[open.length - 1]
  return { ...doc, layout: activate(insert(doc.layout, anchor, view, 'right'), view), focus: view }
}

export function closeView(doc: LayoutDoc, view: string): LayoutDoc {
  const layout = remove(doc.layout, view)
  const open = views(layout)
  const focus = doc.focus && open.includes(doc.focus) ? doc.focus : open[0]
  const zoom = doc.zoom && doc.zoom !== view && open.includes(doc.zoom) ? doc.zoom : undefined
  return { ...doc, layout, focus, zoom }
}

export function focusView(doc: LayoutDoc, view: string): LayoutDoc {
  if (!views(doc.layout).includes(view)) return doc
  return { ...doc, layout: activate(doc.layout, view), focus: view }
}

export function focusDirection(doc: LayoutDoc, zone: Zone): LayoutDoc {
  const current = doc.focus && views(doc.layout).includes(doc.focus) ? doc.focus : views(doc.layout)[0]
  if (!current) return doc
  const next = neighbour(doc.layout, current, zone)
  if (!next) return doc
  return focusView(doc, next)
}

export function zoomView(doc: LayoutDoc, view: string): LayoutDoc {
  if (!views(doc.layout).includes(view)) return doc
  if (doc.zoom === view) return { ...doc, zoom: undefined }
  return { ...doc, zoom: view }
}

export function resetLayout(doc: LayoutDoc, canonical?: Node): LayoutDoc {
  const layout = canonical ? migrateLayout(canonical) : tabGroup(views(doc.layout), 0)
  const open = views(layout)
  return { ...doc, layout, focus: open[0], zoom: undefined }
}

export function zoneFromPoint(fx: number, fy: number): Zone {
  if (fx < 0.22) return 'left'
  if (fx > 0.78) return 'right'
  if (fy < 0.25) return 'top'
  if (fy > 0.75) return 'bottom'
  return 'center'
}

export function zoneFromKey(key: string): Zone | null {
  if (key === 'ArrowLeft' || key === 'left') return 'left'
  if (key === 'ArrowRight' || key === 'right') return 'right'
  if (key === 'ArrowUp' || key === 'up' || key === 'top') return 'top'
  if (key === 'ArrowDown' || key === 'down' || key === 'bottom') return 'bottom'
  return null
}

export function migrateLayout(value: unknown): Node {
  return normalize(readNode(value) ?? { type: 'tabs', views: [], active: 0 })
}

export type ViewRef = { id: string; title: string; type: string }

export type CommandResult = {
  doc: LayoutDoc
  handled: boolean
  summary?: string
}

const layoutCommandNames = new Set([
  'open_view',
  'move_view',
  'split',
  'resize',
  'swap',
  'focus',
  'move_view_left',
  'move_view_right',
  'move_view_up',
  'move_view_down',
  'zoom',
  'close_view',
  'get_layout',
  'reset_layout',
  'ui.open_view',
  'ui.place',
  'ui.link',
])

export function applyLayoutCommand(
  doc: LayoutDoc,
  name: string,
  args: Record<string, unknown>,
  ctx: { views: ViewRef[]; canonical?: Node; place?: Place } = { views: [] },
): CommandResult {
  if (!layoutCommandNames.has(name)) return { doc, handled: false }
  const catalog = ctx.views
  if (name === 'get_layout') {
    return { doc, handled: true, summary: JSON.stringify(layoutSnapshot(doc)) }
  }
  if (name === 'ui.link') {
    const id = resolveView(args.view ?? args.id ?? args.title, catalog)
    if (id && views(doc.layout).includes(id)) return { doc: focusView(doc, id), handled: true }
    return { doc, handled: true }
  }
  if (name === 'reset_layout') return { doc: resetLayout(doc, ctx.canonical), handled: true }
  if (name === 'move_view_left' || name === 'move_view_right' || name === 'move_view_up' || name === 'move_view_down') {
    const zone = name.endsWith('left') ? 'left' : name.endsWith('right') ? 'right' : name.endsWith('up') ? 'top' : 'bottom'
    const view = resolveView(args.view, catalog) ?? focused(doc)
    if (!view) return { doc, handled: true }
    return { doc: { ...doc, layout: moveDir(doc.layout, view, zone), focus: view }, handled: true }
  }
  if (name === 'close_view') {
    const view = resolveView(args.view ?? args.id ?? args.title, catalog) ?? focused(doc)
    if (!view) return { doc, handled: true }
    return { doc: closeView(doc, view), handled: true }
  }
  if (name === 'zoom') {
    const explicit = resolveView(args.view ?? args.id, catalog)
    if (doc.zoom && !explicit) return { doc: { ...doc, zoom: undefined }, handled: true }
    const view = explicit ?? focused(doc)
    if (!view) return { doc, handled: true }
    return { doc: zoomView(doc, view), handled: true }
  }
  if (name === 'focus') {
    const direction = typeof args.direction === 'string' ? zoneFromKey(args.direction) : null
    if (direction) return { doc: focusDirection(doc, direction), handled: true }
    const view = resolveView(args.view ?? args.id ?? args.title, catalog)
    if (!view) return { doc, handled: true }
    return { doc: focusView(doc, view), handled: true }
  }
  if (name === 'swap') {
    const a = resolveView(args.view ?? args.a, catalog) ?? focused(doc)
    const b = resolveView(args.with ?? args.b ?? args.target, catalog)
    if (!a || !b) return { doc, handled: true }
    return { doc: { ...doc, layout: swap(doc.layout, a, b) }, handled: true }
  }
  if (name === 'resize') {
    const view = resolveView(args.view, catalog) ?? focused(doc)
    if (!view) return { doc, handled: true }
    const direction = typeof args.direction === 'string' ? zoneFromKey(args.direction) : null
    const dir: Dir =
      args.dir === 'horizontal' || args.dir === 'vertical'
        ? args.dir
        : direction === 'left' || direction === 'right'
          ? 'horizontal'
          : 'vertical'
    const sign = direction === 'left' || direction === 'top' ? -1 : 1
    const delta = typeof args.delta === 'number' && Number.isFinite(args.delta) ? args.delta : 0.05 * sign
    return { doc: { ...doc, layout: resize(doc.layout, view, dir, delta) }, handled: true }
  }
  if (name === 'split') {
    const view = resolveView(args.view, catalog) ?? focused(doc)
    const zone = legacyZone(args.zone) ?? 'right'
    if (!view || zone === 'center') return { doc, handled: true }
    return { doc: splitOff(doc, view, zone), handled: true }
  }
  if (name === 'move_view') {
    const view = resolveView(args.view ?? args.id, catalog) ?? focused(doc)
    const target = resolveView(args.target, catalog)
    const zone = legacyZone(args.zone) ?? 'right'
    if (!view || !target) return { doc, handled: true }
    return { doc: { ...placeView(doc, view, { target, zone }), focus: view }, handled: true }
  }
  const token = args.view ?? args.entity ?? args.id ?? args.title
  const view = resolveView(token, catalog)
  if (!view) return { doc, handled: true }
  if (name === 'ui.open_view' && args.zone == null && args.place == null && !ctx.place) {
    return { doc: ensureInLayout(doc, view, typeof args.target === 'string' ? args.target : undefined), handled: true }
  }
  const fromArgs = placeFrom(args)
  const place: Place = {
    target: fromArgs.target ?? ctx.place?.target,
    zone: fromArgs.zone ?? ctx.place?.zone,
    dock: fromArgs.dock ?? ctx.place?.dock,
    share: fromArgs.share ?? ctx.place?.share,
    tabIndex: fromArgs.tabIndex ?? ctx.place?.tabIndex,
  }
  if ((name === 'open_view' || name === 'ui.place') && !place.zone && place.dock !== 'edge') place.zone = 'right'
  return { doc: placeView(doc, view, place), handled: true }
}

export function layoutSnapshot(doc: LayoutDoc) {
  return {
    workspace: doc.id,
    layout: doc.layout,
    focus: doc.focus ?? null,
    zoom: doc.zoom ?? null,
    views: views(doc.layout),
  }
}

export function findTabs(node: Node, view: string): TabsNode | null {
  if (node.type === 'tabs') return node.views.includes(view) ? node : null
  for (const child of node.children) {
    const found = findTabs(child, view)
    if (found) return found
  }
  return null
}

function splitOff(doc: LayoutDoc, view: string, zone: Zone): LayoutDoc {
  const pane = findTabs(doc.layout, view)
  if (!pane) return doc
  if (pane.views.length > 1) {
    const anchor = pane.views.find((id) => id !== view)
    if (!anchor) return doc
    const layout = activate(moveView(doc.layout, view, anchor, zone), view)
    return { ...doc, layout, focus: view }
  }
  return { ...doc, layout: activate(dockEdge(doc.layout, view, zone, 0.5), view), focus: view }
}

function focused(doc: LayoutDoc) {
  return doc.focus && views(doc.layout).includes(doc.focus) ? doc.focus : views(doc.layout)[0]
}

function resolveView(token: unknown, catalog: ViewRef[]): string | undefined {
  if (typeof token !== 'string' || token.length === 0) return undefined
  return catalog.find((view) => view.id === token || view.title === token || view.type === token)?.id
}

function placeFrom(args: Record<string, unknown>): Place {
  const raw = args.place
  if (raw && typeof raw === 'object' && !Array.isArray(raw)) {
    const record = raw as Record<string, unknown>
    return {
      target: typeof record.target === 'string' ? record.target : undefined,
      zone: legacyZone(record.zone),
      dock: record.dock === 'edge' || record.dock === true ? 'edge' : undefined,
      share: typeof record.share === 'number' ? record.share : undefined,
      tabIndex: typeof record.tabIndex === 'number' ? record.tabIndex : undefined,
    }
  }
  return {
    target: typeof args.target === 'string' ? args.target : undefined,
    zone: legacyZone(args.zone),
    dock: args.dock === 'edge' || args.dock === true ? 'edge' : undefined,
    share: typeof args.share === 'number' ? args.share : undefined,
    tabIndex: typeof args.tabIndex === 'number' ? args.tabIndex : undefined,
  }
}

function legacyZone(value: unknown): Zone | undefined {
  if (value === 'tab' || value === 'new-tab' || value === 'new tab') return 'center'
  if (value === 'left' || value === 'right' || value === 'top' || value === 'bottom' || value === 'center') return value
  return undefined
}

function walkLeaves(node: Node, rect: Rect, path: number[], out: Leaf[]) {
  if (node.type === 'tabs') {
    out.push({ views: node.views, active: node.active, rect, path })
    return
  }
  let offset = 0
  node.children.forEach((child, index) => {
    const ratio = node.ratios[index] ?? 0
    const childRect =
      node.dir === 'horizontal'
        ? { x: rect.x + offset * rect.w, y: rect.y, w: ratio * rect.w, h: rect.h }
        : { x: rect.x, y: rect.y + offset * rect.h, w: rect.w, h: ratio * rect.h }
    walkLeaves(child, childRect, [...path, index], out)
    offset += ratio
  })
}

function removeFrom(node: Node, view: string): Node {
  if (node.type === 'tabs') {
    if (!node.views.includes(view)) return node
    const current = node.views[node.active]
    const list = node.views.filter((id) => id !== view)
    const active = list.length === 0 ? 0 : Math.max(0, list.indexOf(current ?? ''))
    return { type: 'tabs', views: list, active }
  }
  return { ...node, children: node.children.map((child) => removeFrom(child, view)) }
}

function addTab(pane: TabsNode, view: string): TabsNode {
  if (pane.views.includes(view)) return { ...pane, active: pane.views.indexOf(view) }
  const list = [...pane.views, view]
  return { type: 'tabs', views: list, active: list.length - 1 }
}

function removeViewFromPane(pane: TabsNode, view: string): TabsNode {
  if (!pane.views.includes(view)) return pane
  const current = pane.views[pane.active]
  const list = pane.views.filter((id) => id !== view)
  return { type: 'tabs', views: list, active: list.length === 0 ? 0 : Math.max(0, list.indexOf(current ?? '')) }
}

function findPath(node: Node, view: string, path: number[] = []): number[] | null {
  if (node.type === 'tabs') return node.views.includes(view) ? path : null
  for (let index = 0; index < node.children.length; index += 1) {
    const found = findPath(node.children[index], view, [...path, index])
    if (found) return found
  }
  return null
}

function nodeAt(node: Node, path: number[]): Node | null {
  let current = node
  for (const index of path) {
    if (current.type !== 'split') return null
    const child = current.children[index]
    if (!child) return null
    current = child
  }
  return current
}

function replaceAt(node: Node, path: number[], next: Node): Node {
  if (path.length === 0) return next
  if (node.type !== 'split') return node
  const [head, ...rest] = path
  return {
    ...node,
    children: node.children.map((child, index) => (index === head ? replaceAt(child, rest, next) : child)),
  }
}

function nearestSplit(node: Node, view: string, dir: Dir): { path: number[]; childIndex: number } | null {
  const found: Array<{ path: number[]; childIndex: number; dir: Dir }> = []
  const visit = (current: Node, path: number[]): boolean => {
    if (current.type === 'tabs') return current.views.includes(view)
    for (let index = 0; index < current.children.length; index += 1) {
      if (visit(current.children[index], [...path, index])) {
        found.push({ path, childIndex: index, dir: current.dir })
        return true
      }
    }
    return false
  }
  if (!visit(node, [])) return null
  const match = found.find((item) => item.dir === dir)
  return match ? { path: match.path, childIndex: match.childIndex } : null
}

function fullyOnSide(self: Rect, other: Rect, zone: Zone) {
  if (zone === 'left') return other.x + other.w <= self.x + NEIGHBOUR_TOL
  if (zone === 'right') return other.x >= self.x + self.w - NEIGHBOUR_TOL
  if (zone === 'top') return other.y + other.h <= self.y + NEIGHBOUR_TOL
  if (zone === 'bottom') return other.y >= self.y + self.h - NEIGHBOUR_TOL
  return false
}

function centre(rect: Rect) {
  return { x: rect.x + rect.w / 2, y: rect.y + rect.h / 2 }
}

function swapIds(node: Node, a: string, b: string): Node {
  if (node.type === 'tabs') {
    const previous = node.views[node.active]
    const list = node.views.map((id) => (id === a ? b : id === b ? a : id))
    let active = previous ? list.indexOf(previous) : 0
    if (active < 0) {
      const arrived = previous === a ? b : previous === b ? a : list[0]
      active = Math.max(0, list.indexOf(arrived ?? ''))
    }
    return { type: 'tabs', views: list, active }
  }
  return { ...node, children: node.children.map((child) => swapIds(child, a, b)) }
}

function readNode(value: unknown): Node | null {
  if (!value || typeof value !== 'object') return null
  const record = value as Record<string, unknown>
  if (record.type === 'tabs' && Array.isArray(record.views)) {
    const list = record.views.filter((item): item is string => typeof item === 'string')
    const active = typeof record.active === 'number' ? record.active : typeof record.active === 'string' ? list.indexOf(record.active) : 0
    return { type: 'tabs', views: list, active: list.length === 0 ? 0 : Math.max(0, active) }
  }
  if (record.type === 'split' && (record.dir === 'horizontal' || record.dir === 'vertical') && Array.isArray(record.children)) {
    const children = record.children.map(readNode).filter((child): child is Node => child !== null)
    const ratios = Array.isArray(record.ratios) ? record.ratios.filter((item): item is number => typeof item === 'number') : []
    return { type: 'split', dir: record.dir, ratios, children }
  }
  if (Array.isArray(record.tabs)) {
    const list = record.tabs.filter((item): item is string => typeof item === 'string')
    const activeId = typeof record.active === 'string' ? record.active : list[0]
    const active = list.length === 0 ? 0 : Math.max(0, list.indexOf(activeId ?? ''))
    return { type: 'tabs', views: list, active }
  }
  if ((record.direction === 'row' || record.direction === 'column') && record.first && record.second) {
    const first = readNode(record.first)
    const second = readNode(record.second)
    if (!first || !second) return null
    const ratio = typeof record.ratio === 'number' ? record.ratio : 0.5
    const dir: Dir = record.direction === 'column' ? 'vertical' : 'horizontal'
    return { type: 'split', dir, ratios: [ratio, 1 - ratio], children: [first, second] }
  }
  return null
}

function clamp(value: number, min: number, max: number) {
  return Math.min(max, Math.max(min, value))
}

function roundRatio(value: number) {
  return Math.round(value * 1e6) / 1e6
}

function sumToOne(values: number[]): number[] {
  if (values.length === 0) return values
  const head = values.slice(0, -1).map(roundRatio)
  const used = head.reduce((total, value) => total + value, 0)
  return [...head, roundRatio(1 - used)]
}
