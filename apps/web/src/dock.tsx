import { useEffect, useRef, useState, type PointerEvent as ReactPointerEvent, type ReactNode, type RefObject } from 'react'
import { createPortal } from 'react-dom'
import { Maximize2, Minimize2, MoreHorizontal, Plus, X } from 'lucide-react'
import { ViewGlyph } from './chips'
import { Kbd } from './components/ui/kbd'
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from './components/ui/tooltip'
import { viewDisplayTitle } from './demo-labels'
import { copy } from './i18n'
import { openSharedContextMenu } from './shared-context-menu'
import {
  activate,
  applyLayoutCommand,
  dockEdge,
  findTabs,
  leaves,
  moveView,
  setDivider,
  setTabIndex,
  views,
  zoomView,
  zoneFromPoint,
  type LayoutDoc,
  type Leaf,
  type Node,
  type ViewRef,
  type Zone,
} from './layout'

export type DockView = { id: string; title: string; type: string }

type Hover =
  | { kind: 'edge'; zone: Exclude<Zone, 'center'> }
  | { kind: 'pane'; anchor: string; zone: Zone; tabIndex?: number }

type Drag = { view: string; title: string; type: string; x: number; y: number; hover: Hover | null }

const EDGE = 10
const PREVIEW_INSET = 4
/** Bottom-right of the pointer, clear of the centred zone chip. */
const GHOST_X = 18
const GHOST_Y = 22
const EDGE_SHARE = 0.3
const SPLIT_GAP = 6

export function WorkspaceDock({
  doc,
  views: viewList,
  onChange,
  renderView,
  onAddView,
}: {
  doc: LayoutDoc
  views: DockView[]
  onChange: (doc: LayoutDoc) => void
  renderView: (view: DockView) => ReactNode
  onAddView: (target: string) => void
}) {
  const rootRef = useRef<HTMLDivElement>(null)
  const docRef = useRef(doc)
  const changeRef = useRef(onChange)
  const viewsRef = useRef(viewList)
  docRef.current = doc
  changeRef.current = onChange
  viewsRef.current = viewList
  const [drag, setDrag] = useState<Drag | null>(null)
  const dragRef = useRef<Drag | null>(null)
  // View chrome menus open through the shared command registry.

  const commit = (next: LayoutDoc) => {
    docRef.current = next
    changeRef.current(next)
  }
  const run = (name: string, args: Record<string, unknown> = {}) => {
    const result = applyLayoutCommand(docRef.current, name, args, { views: asRefs(viewsRef.current) })
    if (result.handled) commit(result.doc)
  }

  useEffect(() => {
    if (!drag) return
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return
      event.preventDefault()
      event.stopPropagation()
      dragRef.current = null
      setDrag(null)
    }
    window.addEventListener('keydown', onKey, true)
    return () => window.removeEventListener('keydown', onKey, true)
  }, [drag])

  const open = views(doc.layout)
  const zoomHost = doc.zoom ? leaves(doc.layout).find((leaf) => leaf.views.includes(doc.zoom ?? '')) : undefined
  const zoomed = zoomHost && doc.zoom ? { ...zoomHost, views: [doc.zoom], active: 0 } : undefined
  const shared: Frame = {
    doc,
    viewList,
    drag,
    docRef,
    rootRef,
    dragRef,
    commit,
    run,
    onAddView,
    onMenu: ({ x, y, view }) => {
      const viewType = viewList.find((item) => item.id === view)?.type ?? ''
      openSharedContextMenu({
        context: 'dock-view',
        x,
        y,
        target: { viewId: view, viewType },
      })
    },
    onDrag: setDrag,
    renderView,
  }

  return (
    <TooltipProvider delayDuration={400} disableHoverableContent>
    <div
      ref={rootRef}
      className={drag ? 'dock-root dragging' : 'dock-root'}
      data-testid="dock"
      data-focus={doc.focus ?? ''}
      data-zoom={doc.zoom ?? ''}
      data-leaves={leaves(doc.layout)
        .map((leaf) => leaf.views.join('+'))
        .join('|')}
    >
      {open.length === 0 ? (
        <section className="pane" data-pane="empty">
          <div className="pane-tabs" />
          <div className="pane-body">{copy.empty}</div>
        </section>
      ) : zoomed ? (
        <Pane frame={shared} leaf={zoomed} />
      ) : (
        <SplitNode frame={shared} node={doc.layout} path={[]} />
      )}
      {(['left', 'right', 'top', 'bottom'] as const).map((zone) => (
        <div key={zone} className={`edge-band ${zone}`} data-testid={`edge-${zone}`} />
      ))}
      {drag?.hover?.kind === 'edge' ? (
        <div
          className="dock-preview"
          data-testid="drop-overlay"
          data-zone={drag.hover.zone}
          data-edge="true"
          style={edgePreviewStyle(drag.hover.zone)}
        >
          <span data-testid={`drop-${drag.hover.zone}`}>{zoneLabel(drag.hover.zone)}</span>
        </div>
      ) : null}
      {drag && typeof document !== 'undefined'
        ? createPortal(
            <div className="drag-ghost" data-testid="drag-ghost" style={{ left: drag.x + GHOST_X, top: drag.y + GHOST_Y }}>
              <ViewGlyph type={drag.type} />
              <span>{drag.title}</span>
            </div>,
            document.body,
          )
        : null}
      {null}
    </div>
    </TooltipProvider>
  )
}

type Frame = {
  doc: LayoutDoc
  viewList: DockView[]
  drag: Drag | null
  docRef: RefObject<LayoutDoc>
  rootRef: RefObject<HTMLDivElement | null>
  dragRef: RefObject<Drag | null>
  commit: (doc: LayoutDoc) => void
  run: (name: string, args?: Record<string, unknown>) => void
  onAddView: (target: string) => void
  onMenu: (menu: { x: number; y: number; view: string }) => void
  onDrag: (drag: Drag | null) => void
  renderView: (view: DockView) => ReactNode
}

function SplitNode({ frame, node, path }: { frame: Frame; node: Node; path: number[] }) {
  if (node.type !== 'split') {
    const leaf = leaves(node)[0]
    if (!leaf) return null
    return <Pane frame={frame} leaf={{ ...leaf, path }} />
  }
  const gaps = Math.max(0, node.children.length - 1) * 6
  return (
    <div className={`split ${node.dir}`} data-path={path.join('.') || 'root'}>
      {node.children.map((child, index) => (
        <SplitChild key={cellKey(path, index, child)} frame={frame} node={node} child={child} index={index} path={path} gaps={gaps} />
      ))}
    </div>
  )
}

function SplitChild({
  frame,
  node,
  child,
  index,
  path,
  gaps,
}: {
  frame: Frame
  node: Extract<Node, { type: 'split' }>
  child: Node
  index: number
  path: number[]
  gaps: number
}) {
  const basis = `calc((100% - ${gaps}px) * ${node.ratios[index] ?? 0})`
  return (
    <>
      {index > 0 ? (
        <Divider frame={frame} dir={node.dir} index={index - 1} path={path} count={node.children.length} />
      ) : null}
      <div className="split-cell" style={{ flex: `0 0 ${basis}` }}>
        <SplitNode frame={frame} node={child} path={[...path, index]} />
      </div>
    </>
  )
}

function Divider({
  frame,
  dir,
  index,
  path,
  count,
}: {
  frame: Frame
  dir: 'horizontal' | 'vertical'
  index: number
  path: number[]
  count: number
}) {
  const [active, setActive] = useState(false)
  const onPointerDown = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (event.button !== 0) return
    event.preventDefault()
    event.stopPropagation()
    const container = event.currentTarget.parentElement
    if (!container) return
    const horizontal = dir === 'horizontal'
    setActive(true)
    const onMove = (ev: PointerEvent) => {
      const rect = container.getBoundingClientRect()
      const span = (horizontal ? rect.width : rect.height) - Math.max(0, count - 1) * 6
      const pointer = (horizontal ? ev.clientX - rect.left : ev.clientY - rect.top) - index * 6 - 3
      const position = span > 0 ? pointer / span : 0
      const current = frame.docRef.current
      frame.commit({ ...current, layout: setDivider(current.layout, path, index, position) })
    }
    const onUp = () => {
      setActive(false)
      window.removeEventListener('pointermove', onMove)
      window.removeEventListener('pointerup', onUp)
    }
    window.addEventListener('pointermove', onMove)
    window.addEventListener('pointerup', onUp)
  }
  return (
    <div
      className={`split-divider ${dir}`}
      data-testid="split-divider"
      data-active={active ? 'true' : 'false'}
      role="separator"
      aria-orientation={dir === 'horizontal' ? 'vertical' : 'horizontal'}
      onPointerDown={onPointerDown}
    >
      <span />
    </div>
  )
}

function Pane({ frame, leaf }: { frame: Frame; leaf: Leaf }) {
  const activeId = leaf.views[leaf.active] ?? leaf.views[0]
  const focused = frame.doc.focus != null && leaf.views.includes(frame.doc.focus)
  const hover = frame.drag?.hover
  const preview = hover?.kind === 'pane' && leaf.views.includes(hover.anchor) ? hover.zone : null
  const dragged = useRef(false)
  const pendingActive = useRef<{ view: string; timer: number } | null>(null)

  const beginDrag = (event: ReactPointerEvent<HTMLButtonElement>, id: string) => {
    if (event.button !== 0) return
    if (event.target instanceof Element && event.target.closest('.tab-x')) return
    const startX = event.clientX
    const startY = event.clientY
    const meta = frame.viewList.find((view) => view.id === id)
    const title = titleOf(frame.viewList, id)
    const type = meta?.type ?? ''
    let started = false
    const move = (ev: PointerEvent) => {
      if (!started && Math.hypot(ev.clientX - startX, ev.clientY - startY) < 4) return
      started = true
      dragged.current = true
      const hoverAt = frame.rootRef.current ? hitTest(frame.rootRef.current, ev.clientX, ev.clientY, id) : null
      const next: Drag = { view: id, title, type, x: ev.clientX, y: ev.clientY, hover: hoverAt }
      frame.dragRef.current = next
      frame.onDrag(next)
    }
    const up = () => {
      window.removeEventListener('pointermove', move)
      window.removeEventListener('pointerup', up)
      const current = frame.dragRef.current
      frame.dragRef.current = null
      frame.onDrag(null)
      if (started && current) frame.commit(dropView(frame.docRef.current, current))
    }
    window.addEventListener('pointermove', move)
    window.addEventListener('pointerup', up)
  }

  return (
    <section
      className={focused ? 'pane focused' : 'pane'}
      data-pane={activeId ?? 'empty'}
      data-anchor={activeId ?? ''}
      data-views={leaf.views.join(',')}
      tabIndex={-1}
      onMouseDown={(event) => {
        if (!(event.target instanceof Element)) return
        if (event.target.closest('button, a, input, textarea, select')) return
        if (activeId && !frame.doc.zoom) frame.commit(activateDoc(frame.docRef.current, activeId))
      }}
    >
      <div className="pane-tabs">
        {leaf.views.map((id) => (
          <Tooltip key={id}>
            <TooltipTrigger asChild>
              <button
                type="button"
                data-tab={id}
                data-view-type={frame.viewList.find((view) => view.id === id)?.type ?? ''}
                className={id === activeId ? 'pane-tab active' : 'pane-tab'}
                onClick={() => {
                  if (dragged.current) {
                    dragged.current = false
                    if (pendingActive.current) window.clearTimeout(pendingActive.current.timer)
                    pendingActive.current = null
                    return
                  }
                  if (frame.docRef.current.zoom) return
                  frame.commit(activateDoc(frame.docRef.current, id))
                  if (pendingActive.current) window.clearTimeout(pendingActive.current.timer)
                  const timer = window.setTimeout(() => {
                    pendingActive.current = null
                  }, 500)
                  if (pendingActive.current) pendingActive.current.timer = timer
                }}
                onDoubleClick={(event) => {
                  event.preventDefault()
                  event.stopPropagation()
                  dragged.current = false
                  const previous = pendingActive.current?.view
                  if (pendingActive.current) window.clearTimeout(pendingActive.current.timer)
                  pendingActive.current = null
                  const current = frame.docRef.current
                  if (current.zoom === id) {
                    frame.commit(zoomView(current, id))
                    return
                  }
                  const base = previous ? activateDoc(current, previous) : current
                  frame.commit(zoomView(base, id))
                }}
                onPointerDown={(event) => {
                  if (!frame.docRef.current.zoom && !pendingActive.current) {
                    pendingActive.current = { view: leaf.views[leaf.active] ?? id, timer: 0 }
                  }
                  beginDrag(event, id)
                }}
                onMouseDown={(event) => {
                  if (event.button === 1) {
                    event.preventDefault()
                    frame.run('close_view', { view: id })
                  }
                }}
                onContextMenu={(event) => {
                  event.preventDefault()
                  frame.onMenu({ x: event.clientX, y: event.clientY, view: id })
                }}
              >
                <span>{titleOf(frame.viewList, id)}</span>
                <span
                  className="tab-x"
                  onClick={(event) => {
                    event.stopPropagation()
                    frame.run('close_view', { view: id })
                  }}
                >
                  <X size={12} aria-hidden="true" />
                </span>
              </button>
            </TooltipTrigger>
            <TooltipContent>{frame.doc.zoom === id ? copy.doubleClickRestore : copy.doubleClickMaximize}</TooltipContent>
          </Tooltip>
        ))}
        <div className="pane-chrome">
          <button type="button" className="icon-btn" aria-label={copy.addView} onClick={() => activeId && frame.onAddView(activeId)}>
            <Plus size={14} aria-hidden="true" />
          </button>
          <button
            type="button"
            className="icon-btn"
            aria-label={copy.viewMenu}
            onClick={(event) => activeId && frame.onMenu({ x: event.clientX, y: event.clientY + 8, view: activeId })}
          >
            <MoreHorizontal size={14} aria-hidden="true" />
          </button>
          <Tooltip>
            <TooltipTrigger asChild>
              <button
                type="button"
                className="icon-btn"
                data-testid="pane-maximize"
                aria-label={frame.doc.zoom ? copy.restore : copy.maximize}
                aria-keyshortcuts="Control+Alt+Z"
                onClick={() => activeId && frame.run('zoom', { view: activeId })}
              >
                {frame.doc.zoom ? <Minimize2 size={14} aria-hidden="true" /> : <Maximize2 size={14} aria-hidden="true" />}
              </button>
            </TooltipTrigger>
            <TooltipContent>
              <span>{frame.doc.zoom ? copy.restore : copy.maximize}</span>
              <Kbd>Ctrl</Kbd>
              <Kbd>Alt</Kbd>
              <Kbd>Z</Kbd>
            </TooltipContent>
          </Tooltip>
        </div>
      </div>
      <div className="pane-body">
        {activeId ? frame.renderView(frame.viewList.find((view) => view.id === activeId) ?? { id: activeId, title: activeId, type: '' }) : <p>{copy.empty}</p>}
      </div>
      {preview ? (
        <div className="dock-preview" data-testid="drop-overlay" data-zone={preview} style={previewStyle(preview)}>
          <span data-testid={`drop-${preview}`}>{zoneLabel(preview)}</span>
        </div>
      ) : null}
    </section>
  )
}


function dropView(doc: LayoutDoc, drag: Drag): LayoutDoc {
  const hover = drag.hover
  if (!hover) return doc
  let layout = doc.layout
  try {
    if (hover.kind === 'edge') {
      layout = dockEdge(layout, drag.view, hover.zone, 0.3)
    } else if (hover.tabIndex != null) {
      const pane = findTabs(layout, hover.anchor)
      if (pane?.views.includes(drag.view)) layout = setTabIndex(layout, drag.view, hover.tabIndex)
      else {
        const anchor = pane?.views.find((id) => id !== drag.view) ?? hover.anchor
        layout = setTabIndex(moveView(layout, drag.view, anchor, 'center'), drag.view, hover.tabIndex)
      }
    } else {
      const pane = findTabs(layout, hover.anchor)
      if (hover.zone !== 'center' && pane && pane.views.length === 1 && pane.views[0] === drag.view) return doc
      const anchor = pane?.views.find((id) => id !== drag.view) ?? hover.anchor
      layout = moveView(layout, drag.view, anchor, hover.zone)
    }
  } catch {
    return doc
  }
  return { ...doc, layout: activate(layout, drag.view), focus: drag.view, zoom: undefined }
}

function hitTest(root: HTMLElement, x: number, y: number, view: string): Hover | null {
  const rect = root.getBoundingClientRect()
  if (x < rect.left || y < rect.top || x > rect.right || y > rect.bottom) return null
  const edge = edgeAt(rect, x, y)
  if (edge) return { kind: 'edge', zone: edge }
  const target = document.elementFromPoint(x, y)
  const pane = target instanceof Element ? target.closest<HTMLElement>('[data-pane]') : null
  const anchor = pane?.dataset.anchor
  if (!pane || !anchor) return null
  const strip = pane.querySelector<HTMLElement>('.pane-tabs')
  if (strip && target instanceof Element && strip.contains(target)) {
    return { kind: 'pane', anchor, zone: 'center', tabIndex: insertionIndex(strip, x, view) }
  }
  const box = pane.getBoundingClientRect()
  return { kind: 'pane', anchor, zone: zoneFromPoint((x - box.left) / box.width, (y - box.top) / box.height) }
}

function edgeAt(rect: DOMRect, x: number, y: number): Exclude<Zone, 'center'> | null {
  if (x - rect.left <= EDGE) return 'left'
  if (rect.right - x <= EDGE) return 'right'
  if (y - rect.top <= EDGE) return 'top'
  if (rect.bottom - y <= EDGE) return 'bottom'
  return null
}

function insertionIndex(strip: HTMLElement, clientX: number, dragged: string) {
  const tabs = [...strip.querySelectorAll<HTMLElement>('[data-tab]')].filter((tab) => tab.dataset.tab !== dragged)
  for (let index = 0; index < tabs.length; index += 1) {
    const box = tabs[index].getBoundingClientRect()
    if (clientX < box.left + box.width / 2) return index
  }
  return tabs.length
}

function activateDoc(doc: LayoutDoc, view: string): LayoutDoc {
  if (!views(doc.layout).includes(view)) return doc
  return { ...doc, layout: activate(doc.layout, view), focus: view }
}

function titleOf(list: DockView[], id: string) {
  return viewDisplayTitle(list.find((view) => view.id === id)?.title ?? id)
}

function zoneLabel(zone: Zone) {
  if (zone === 'left') return copy.dockLeft
  if (zone === 'right') return copy.dockRight
  if (zone === 'top') return copy.dockTop
  if (zone === 'bottom') return copy.dockBottom
  return copy.addAsTab
}

function previewStyle(zone: Zone): React.CSSProperties {
  const inset = PREVIEW_INSET
  if (zone === 'left') return { left: inset, top: inset, width: `calc(50% - ${inset * 2}px)`, height: `calc(100% - ${inset * 2}px)` }
  if (zone === 'right') return { left: `calc(50% + ${inset}px)`, top: inset, width: `calc(50% - ${inset * 2}px)`, height: `calc(100% - ${inset * 2}px)` }
  if (zone === 'top') return { left: inset, top: inset, width: `calc(100% - ${inset * 2}px)`, height: `calc(50% - ${inset * 2}px)` }
  if (zone === 'bottom') return { left: inset, top: `calc(50% + ${inset}px)`, width: `calc(100% - ${inset * 2}px)`, height: `calc(50% - ${inset * 2}px)` }
  return { left: inset, top: inset, width: `calc(100% - ${inset * 2}px)`, height: `calc(100% - ${inset * 2}px)` }
}

/** Preview of the pane `dockEdge` will create: `EDGE_SHARE` of the dock, after the 6px split gap, then the same 4px inset. */
function edgePreviewStyle(zone: Exclude<Zone, 'center'>): React.CSSProperties {
  const inset = PREVIEW_INSET
  const span = `calc((100% - ${SPLIT_GAP}px) * ${EDGE_SHARE} - ${inset * 2}px)`
  const cross = `calc(100% - ${inset * 2}px)`
  const start = `calc((100% - ${SPLIT_GAP}px) * ${1 - EDGE_SHARE} + ${SPLIT_GAP}px + ${inset}px)`
  if (zone === 'left') return { left: inset, top: inset, width: span, height: cross }
  if (zone === 'right') return { left: start, top: inset, width: span, height: cross }
  if (zone === 'top') return { left: inset, top: inset, width: cross, height: span }
  return { left: inset, top: start, width: cross, height: span }
}

function asRefs(list: DockView[]): ViewRef[] {
  return list.map((view) => ({ id: view.id, title: view.title, type: view.type }))
}

function cellKey(path: number[], index: number, node: Node) {
  return `${path.join('.')}:${index}:${views(node).join(',')}`
}
