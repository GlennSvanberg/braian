import {
  forceCenter,
  forceCollide,
  forceLink,
  forceManyBody,
  forceSimulation,
  type ForceLink,
  type Simulation,
  type SimulationLinkDatum,
  type SimulationNodeDatum,
} from 'd3-force'
import { useEffect, useMemo, useRef, useState, type KeyboardEvent as ReactKeyboardEvent } from 'react'
import { api, textValue, type Json } from './api'
import { MetaChip, typeLabel } from './chips'
import {
  NODE_KINDS,
  NODE_RADIUS,
  buildScene,
  capScene,
  expandAssortment,
  expandClass,
  groupMembers,
  kindLabel,
  mergeScene,
  parseTypedId,
  searchDocuments,
  type KnownObject,
  type NodeKind,
  type Scene,
  type SceneNode,
} from './graph-scene'
import { copy, uiLanguage } from './i18n'

type SimNode = SimulationNodeDatum & SceneNode
type SimLink = SimulationLinkDatum<SimNode> & { id: string; label: string }
type Marker = { x: number; y: number }
type Palette = {
  fill: Record<NodeKind, string>
  text: string
  text2: string
  muted: string
  bg: string
  border: string
  raised: string
}

export function GraphMap({
  data,
  values,
  defs,
  onOpen,
}: {
  data: Json
  values?: Json
  defs?: Json
  onOpen: (id: string) => void
}) {
  const focusId = String(data.id ?? '')
  const focusKey = `object:${focusId}`
  const [seen, setSeen] = useState(focusId)
  const [extras, setExtras] = useState<Scene[]>([])
  const [known, setKnown] = useState<Record<string, KnownObject>>({})
  const [selected, setSelected] = useState(focusKey)
  const [hidden, setHidden] = useState<Set<NodeKind>>(() => new Set())
  const [full, setFull] = useState(false)
  const [markers, setMarkers] = useState<Record<string, Marker>>({})
  const [pinTick, setPinTick] = useState(0)
  const [busy, setBusy] = useState('')
  const expandedRef = useRef(new Set<string>([focusKey]))
  const inflightRef = useRef(new Set<string>())
  const defsRef = useRef(defs)
  defsRef.current = defs

  if (seen !== focusId) {
    setSeen(focusId)
    setExtras([])
    setKnown({})
    setSelected(focusKey)
    setHidden(new Set())
    setFull(false)
    expandedRef.current = new Set([focusKey])
  }

  const model = useMemo(() => {
    let merged = buildScene({ graph: data, values, defs, known })
    for (const extra of extras) merged = mergeScene(merged, extra)
    return capScene(merged)
  }, [data, values, defs, known, extras])
  const scene = model.scene

  useEffect(() => {
    const relations = (data.relations as Json[]) ?? []
    let cancel = false
    void (async () => {
      const found: Record<string, KnownObject> = {}
      await Promise.all(
        relations.map(async (relation) => {
          const id = String(relation.object_id ?? '')
          if (!id) return
          try {
            const object = await api<Json>(`/v1/objects/${id}`)
            const name = textValue((object.values as Json | undefined)?.namn, uiLanguage)
            if (!name) return
            found[id] = { name, typeKey: String(object.type ?? '') }
          } catch {
            // The number stays on the node until the object can be read.
          }
        }),
      )
      if (!cancel) setKnown(found)
    })()
    return () => {
      cancel = true
    }
  }, [focusId, data.relations])

  const counts = useMemo(() => {
    const tally = Object.fromEntries(NODE_KINDS.map((kind) => [kind, 0])) as Record<NodeKind, number>
    for (const node of scene.nodes) tally[node.kind] += 1
    return tally
  }, [scene])

  const canvasRef = useRef<HTMLCanvasElement>(null)
  const simRef = useRef<Simulation<SimNode, SimLink> | null>(null)
  const drawRef = useRef<(shouldPublish: boolean) => void>(() => {})
  const apiRef = useRef({ fit: () => {}, relayout: () => {}, refocus: () => {} })
  const hiddenRef = useRef(hidden)
  const selectedRef = useRef(selected)
  const expandRef = useRef<(node: SceneNode) => void>(() => {})
  const selectRef = useRef(setSelected)
  const pinRef = useRef(setPinTick)
  hiddenRef.current = hidden
  selectedRef.current = selected
  selectRef.current = setSelected
  pinRef.current = setPinTick

  const expand = async (node: SceneNode) => {
    if (!node || expandedRef.current.has(node.id) || inflightRef.current.has(node.id)) return
    if (node.kind === 'value') {
      expandedRef.current.add(node.id)
      return
    }
    expandedRef.current.add(node.id)
    inflightRef.current.add(node.id)
    setBusy(node.id)
    try {
      let extra: Scene = { nodes: [], edges: [] }
      if (node.kind === 'object' && node.objectId) {
        const [graph, object] = await Promise.all([
          api<Json>(`/v1/objects/${node.objectId}/graph`),
          api<Json>(`/v1/objects/${node.objectId}`),
        ])
        extra = buildScene({
          graph,
          values: (object.values as Json) ?? {},
          defs: defsRef.current,
        })
      } else if (node.kind === 'class' || node.kind === 'group' || node.kind === 'assortment') {
        const parsed = parseTypedId(node.id)
        if (node.kind === 'class') extra = expandClass(defsRef.current, parsed.structure, parsed.key)
        if (node.kind === 'assortment') extra = expandAssortment(defsRef.current, parsed.structure)
        if (node.kind === 'group') {
          const params = new URLSearchParams({
            filter: `groups:=${parsed.structure}:${parsed.key}`,
            per_page: '40',
          })
          const body = await api<Json>(`/v1/search?${params}`)
          const hits = searchDocuments(body).map((doc) => {
            const english = textValue(doc.a_namn__en, 'en')
            const fallback = textValue(doc.a_namn__sv, 'sv')
            return {
              id: String(doc.id ?? ''),
              key: String(doc.key ?? ''),
              name: english || fallback,
              type: typeLabel(defsRef.current, String(doc.type_key ?? ''), uiLanguage),
              rawCaption: !english,
            }
          })
          extra = groupMembers(parsed.structure, parsed.key, hits)
        }
      }
      if (extra.nodes.length || extra.edges.length) setExtras((current) => [...current, extra])
    } catch {
      expandedRef.current.delete(node.id)
    } finally {
      inflightRef.current.delete(node.id)
      setBusy('')
    }
  }
  expandRef.current = (node) => {
    void expand(node)
  }

  useEffect(() => {
    const canvas = canvasRef.current
    if (!canvas) return
    const context = canvas.getContext('2d')
    if (!context) return
    const reduced = window.matchMedia('(prefers-reduced-motion: reduce)').matches
    const transform = { x: 0, y: 0, k: 1 }
    const hoverRef = { id: '' }
    const movedRef = { current: false }
    const fitAfterRef = { current: true }
    const paletteRef: { current: Palette | null } = { current: null }
    const markerSerial = { current: '' }
    let alive = true

    const sim = forceSimulation<SimNode, SimLink>([])
      .force('link', forceLink<SimNode, SimLink>([]).id((node) => node.id).distance(148).strength(0.38))
      .force('charge', forceManyBody<SimNode>().strength(-280).distanceMax(640))
      .force('center', forceCenter(0, 0))
      .force('collide', forceCollide<SimNode>().radius((node) => NODE_RADIUS[node.kind] + 12).iterations(2))
      .alphaDecay(reduced ? 0.18 : 0.03)
      .velocityDecay(0.45)
      .stop()
    simRef.current = sim

    const readPalette = (): Palette => {
      if (paletteRef.current) return paletteRef.current
      const root = getComputedStyle(document.documentElement)
      const token = (name: string) => root.getPropertyValue(name).trim()
      const swatch = (kind: NodeKind) => {
        const el = document.querySelector<HTMLElement>(`[data-swatch='${kind}']`)
        return el ? getComputedStyle(el).backgroundColor : token('--accent')
      }
      paletteRef.current = {
        fill: {
          object: swatch('object'),
          class: swatch('class'),
          group: swatch('group'),
          value: swatch('value'),
          assortment: swatch('assortment'),
        },
        text: token('--text'),
        text2: token('--text-2'),
        muted: token('--muted'),
        bg: token('--bg'),
        border: token('--border'),
        raised: token('--raised'),
      }
      return paletteRef.current
    }

    const resize = () => {
      const width = canvas.clientWidth
      const height = canvas.clientHeight
      const dpr = Math.min(window.devicePixelRatio || 1, 2)
      const nextW = Math.max(1, Math.floor(width * dpr))
      const nextH = Math.max(1, Math.floor(height * dpr))
      if (canvas.width !== nextW || canvas.height !== nextH) {
        canvas.width = nextW
        canvas.height = nextH
      }
      if (!movedRef.current && transform.x === 0 && transform.y === 0 && width > 0) {
        transform.x = width / 2
        transform.y = height / 2
      }
    }

    const screenOf = (x: number, y: number) => ({
      x: transform.x + x * transform.k,
      y: transform.y + y * transform.k,
    })

    const hitTest = (ox: number, oy: number) => {
      const wx = (ox - transform.x) / transform.k
      const wy = (oy - transform.y) / transform.k
      const nodes = sim.nodes()
      const slop = 6 / transform.k
      for (let index = nodes.length - 1; index >= 0; index -= 1) {
        const node = nodes[index]
        if (!node || hiddenRef.current.has(node.kind) || node.x == null || node.y == null) continue
        const reach = NODE_RADIUS[node.kind] + slop
        if ((node.x - wx) ** 2 + (node.y - wy) ** 2 <= reach ** 2) return node
      }
      return undefined
    }

    const publish = () => {
      const rect = canvas.getBoundingClientRect()
      const next: Record<string, Marker> = {}
      for (const node of sim.nodes()) {
        if (hiddenRef.current.has(node.kind) || node.x == null || node.y == null) continue
        const point = screenOf(node.x, node.y)
        const marker = { x: Math.round(rect.left + point.x), y: Math.round(rect.top + point.y) }
        next[node.id] = marker
        const button = document.querySelector<HTMLElement>(`[data-node-id="${CSS.escape(node.id)}"]`)
        if (button) {
          button.setAttribute('data-x', String(marker.x))
          button.setAttribute('data-y', String(marker.y))
        }
      }
      const serial = JSON.stringify(next)
      if (serial === markerSerial.current) return
      markerSerial.current = serial
      if (alive) setMarkers(next)
    }

    const draw = (shouldPublish: boolean) => {
      resize()
      const width = canvas.clientWidth
      const height = canvas.clientHeight
      const dpr = Math.min(window.devicePixelRatio || 1, 2)
      const colors = readPalette()
      const nodes = sim.nodes()
      const links = sim.force<ForceLink<SimNode, SimLink>>('link')?.links() ?? []
      const hiddenNow = hiddenRef.current
      const hover = hoverRef.id
      const current = selectedRef.current
      const highlight = new Set<string>()
      if (hover) {
        highlight.add(hover)
        for (const link of links) {
          const source = endId(link.source)
          const target = endId(link.target)
          if (source === hover) highlight.add(target)
          if (target === hover) highlight.add(source)
        }
      }
      const captionIds = new Set<string>()
      const captionSeed = hover || current
      let degree = 0
      if (captionSeed) {
        captionIds.add(captionSeed)
        for (const link of links) {
          const source = endId(link.source)
          const target = endId(link.target)
          if (source === captionSeed || target === captionSeed) {
            captionIds.add(source)
            captionIds.add(target)
            degree += 1
          }
        }
      }
      const showEdgeLabels = degree > 0 && degree <= 5

      context.setTransform(dpr, 0, 0, dpr, 0, 0)
      context.clearRect(0, 0, width, height)
      context.fillStyle = colors.bg
      context.fillRect(0, 0, width, height)
      context.lineJoin = 'round'
      context.lineCap = 'round'
      context.textAlign = 'center'
      context.textBaseline = 'middle'

      const paintLink = (link: SimLink, emphasized: boolean) => {
        const source = endNode(link.source)
        const target = endNode(link.target)
        if (!source || !target || source.x == null || source.y == null || target.x == null || target.y == null) return
        if (hiddenNow.has(source.kind) || hiddenNow.has(target.kind)) return
        const from = screenOf(source.x, source.y)
        const to = screenOf(target.x, target.y)
        const dx = to.x - from.x
        const dy = to.y - from.y
        const len = Math.hypot(dx, dy) || 1
        const bend = bendOf(link.id, len)
        const control = { x: (from.x + to.x) / 2 + (-dy / len) * bend, y: (from.y + to.y) / 2 + (dx / len) * bend }
        const targetR = NODE_RADIUS[target.kind] * transform.k
        const tip = pointAtDistance(from, control, to, targetR + 2, true)
        context.save()
        context.globalAlpha = !hover || emphasized ? 0.95 : 0.12
        context.strokeStyle = emphasized ? colors.text2 : colors.border
        context.fillStyle = emphasized ? colors.text2 : colors.border
        context.lineWidth = emphasized ? 1.8 : 1.25
        context.beginPath()
        context.moveTo(from.x, from.y)
        context.quadraticCurveTo(control.x, control.y, to.x, to.y)
        context.stroke()
        const tangent = curveTangent(from, control, to, tip.t)
        arrow(context, tip.point.x, tip.point.y, Math.atan2(tangent.y, tangent.x))
        const showLabel = showEdgeLabels && captionIds.has(source.id) && captionIds.has(target.id)
        if (showLabel && transform.k >= 0.55) {
          const mid = curvePoint(from, control, to, 0.68)
          context.font = '11px "Inter Variable", Inter, sans-serif'
          const widthText = context.measureText(link.label).width
          const boxW = widthText + 12
          const boxH = 16
          context.globalAlpha = !hover || emphasized ? 1 : 0.16
          context.fillStyle = colors.raised
          context.strokeStyle = colors.border
          context.lineWidth = 1
          roundRect(context, mid.x - boxW / 2, mid.y - boxH / 2, boxW, boxH, 8)
          context.fill()
          context.stroke()
          context.fillStyle = colors.text2
          context.fillText(link.label, mid.x, mid.y + 0.5)
        }
        context.restore()
      }

      for (const link of links) {
        const source = endId(link.source)
        const target = endId(link.target)
        if (hover && (highlight.has(source) || highlight.has(target))) continue
        paintLink(link, false)
      }
      if (hover) {
        for (const link of links) {
          const source = endId(link.source)
          const target = endId(link.target)
          if (highlight.has(source) || highlight.has(target)) paintLink(link, true)
        }
      }

      const visibleCount = nodes.reduce((count, node) => count + (hiddenNow.has(node.kind) ? 0 : 1), 0)
      const roomy = visibleCount <= 36 && transform.k >= 0.75
      for (const node of nodes) {
        if (hiddenNow.has(node.kind) || node.x == null || node.y == null) continue
        const point = screenOf(node.x, node.y)
        const radius = NODE_RADIUS[node.kind] * transform.k
        const emphasized = !hover || highlight.has(node.id)
        const fill = colors.fill[node.kind]
        context.save()
        context.globalAlpha = emphasized ? 1 : 0.14
        context.beginPath()
        context.arc(point.x, point.y, radius, 0, Math.PI * 2)
        context.fillStyle = fill
        context.fill()
        context.lineWidth = node.id === current ? 3 : hover === node.id ? 2 : 2
        context.strokeStyle = node.id === current || hover === node.id ? colors.text : colors.bg
        context.stroke()
        context.font = `600 ${radius >= 22 ? 12 : 11}px "Inter Variable", Inter, sans-serif`
        const lines = radius >= 14 ? insideLines(context, node.caption, radius * 1.75) : []
        const insideFits = lines.length === 1 && lines[0] === node.caption
        if (lines.length) {
          context.fillStyle = ink(fill, colors.text, colors.bg)
          if (lines.length === 1) context.fillText(lines[0] ?? '', point.x, point.y)
          else {
            context.fillText(lines[0] ?? '', point.x, point.y - 6)
            context.fillText(lines[1] ?? '', point.x, point.y + 7)
          }
        }
        const showCaption = (roomy || captionIds.has(node.id)) && transform.k >= 0.6
        if (showCaption && !insideFits) {
          context.globalAlpha = emphasized ? 1 : 0.35
          context.fillStyle = colors.text
          context.font = '12px "Inter Variable", Inter, sans-serif'
          context.fillText(fitText(context, node.caption, roomy ? 112 : 160), point.x, point.y + radius + 13)
        }
        if (captionIds.has(node.id) && node.subtitle && transform.k >= 0.6) {
          context.globalAlpha = emphasized ? 1 : 0.35
          context.fillStyle = colors.muted
          context.font = '10px "Inter Variable", Inter, sans-serif'
          const under = !insideFits ? radius + 26 : radius + 14
          context.fillText(fitText(context, node.subtitle, 150), point.x, point.y + under)
        }
        context.restore()
      }

      if (shouldPublish) publish()
    }

    const fit = () => {
      resize()
      const nodes = sim.nodes().filter((node) => !hiddenRef.current.has(node.kind) && node.x != null && node.y != null)
      const width = canvas.clientWidth
      const height = canvas.clientHeight
      if (!nodes.length || width < 20 || height < 20) return
      let minX = Infinity
      let minY = Infinity
      let maxX = -Infinity
      let maxY = -Infinity
      for (const node of nodes) {
        const pad = NODE_RADIUS[node.kind] + 36
        minX = Math.min(minX, node.x! - pad)
        maxX = Math.max(maxX, node.x! + pad)
        minY = Math.min(minY, node.y! - pad)
        maxY = Math.max(maxY, node.y! + pad)
      }
      const boundsW = Math.max(1, maxX - minX)
      const boundsH = Math.max(1, maxY - minY)
      const padRight = width >= 720 ? 280 : Math.round(width * 0.42)
      const viewW = Math.max(140, width - padRight - 28)
      const viewH = Math.max(140, height - 84)
      const fitted = Math.min(viewW / boundsW, viewH / boundsH)
      const focus = nodes.find((node) => node.id === selectedRef.current) ?? nodes.find((node) => node.kind === 'object') ?? nodes[0]
      if (fitted >= 0.72 && fitted <= 1.4) {
        const k = Math.min(1.2, fitted)
        transform.k = k
        transform.x = 20 + viewW / 2 - ((minX + maxX) / 2) * k
        transform.y = (height - 16) / 2 - ((minY + maxY) / 2) * k
      } else if (focus?.x != null && focus.y != null) {
        transform.k = 1
        transform.x = 20 + viewW / 2 - focus.x
        transform.y = (height - 16) / 2 - focus.y
      } else {
        const k = Math.min(1.2, Math.max(0.5, fitted))
        transform.k = k
        transform.x = 20 + viewW / 2 - ((minX + maxX) / 2) * k
        transform.y = (height - 16) / 2 - ((minY + maxY) / 2) * k
      }
      draw(true)
    }

    const relayout = () => {
      const nodes = sim.nodes()
      nodes.forEach((node, index) => {
        node.fx = null
        node.fy = null
        const pos = seed(node.kind, index, nodes.length)
        node.x = pos.x
        node.y = pos.y
        node.vx = 0
        node.vy = 0
      })
      movedRef.current = false
      fitAfterRef.current = true
      canvas.dataset.settled = 'false'
      sim.alpha(1).restart()
      pinRef.current((tick) => tick + 1)
    }

    apiRef.current = {
      fit,
      relayout,
      refocus: () => {
        movedRef.current = false
        fitAfterRef.current = true
      },
    }
    drawRef.current = draw

    sim.on('tick', () => {
      if (alive) draw(false)
    })
    sim.on('end', () => {
      if (!alive || sim.nodes().length === 0) return
      if (fitAfterRef.current) fit()
      else draw(true)
      canvas.dataset.settled = 'true'
    })

    let mode: 'none' | 'pan' | 'node' = 'none'
    let dragNode: SimNode | undefined
    let lastX = 0
    let lastY = 0
    let startX = 0
    let startY = 0
    let dragged = false

    const onPointerDown = (event: PointerEvent) => {
      canvas.focus()
      const hit = hitTest(event.offsetX, event.offsetY)
      lastX = event.offsetX
      lastY = event.offsetY
      startX = event.offsetX
      startY = event.offsetY
      dragged = false
      if (hit) {
        mode = 'node'
        dragNode = hit
        hit.fx = hit.x
        hit.fy = hit.y
        sim.alphaTarget(0.25).restart()
      } else {
        mode = 'pan'
        canvas.classList.add('panning')
      }
      if (typeof event.pointerId === 'number') canvas.setPointerCapture(event.pointerId)
    }
    const onPointerMove = (event: PointerEvent) => {
      const hit = hitTest(event.offsetX, event.offsetY)
      const nextHover = hit?.id ?? ''
      if (hoverRef.id !== nextHover) {
        hoverRef.id = nextHover
        canvas.style.cursor = nextHover ? 'pointer' : mode === 'pan' ? 'grabbing' : 'grab'
        draw(false)
      }
      if (mode === 'none') return
      const dx = event.offsetX - lastX
      const dy = event.offsetY - lastY
      lastX = event.offsetX
      lastY = event.offsetY
      if (Math.hypot(event.offsetX - startX, event.offsetY - startY) > 3) dragged = true
      if (mode === 'pan') {
        transform.x += dx
        transform.y += dy
        movedRef.current = true
        fitAfterRef.current = false
        draw(false)
      } else if (dragNode && dragNode.x != null && dragNode.y != null) {
        dragNode.fx = (dragNode.fx ?? dragNode.x) + dx / transform.k
        dragNode.fy = (dragNode.fy ?? dragNode.y) + dy / transform.k
        movedRef.current = true
        fitAfterRef.current = false
        draw(false)
      }
    }
    const onPointerUp = (event: PointerEvent) => {
      if (mode === 'node' && dragNode) {
        const travel = Math.hypot(event.offsetX - startX, event.offsetY - startY)
        if (!dragged || travel <= 3) {
          dragNode.fx = null
          dragNode.fy = null
        }
        selectRef.current(dragNode.id)
        sim.alphaTarget(0)
        pinRef.current((tick) => tick + 1)
        draw(true)
      }
      mode = 'none'
      dragNode = undefined
      canvas.classList.remove('panning')
      if (typeof event.pointerId === 'number' && canvas.hasPointerCapture(event.pointerId)) canvas.releasePointerCapture(event.pointerId)
    }
    const onPointerLeave = () => {
      if (mode !== 'none') return
      if (!hoverRef.id) return
      hoverRef.id = ''
      canvas.style.cursor = 'grab'
      draw(false)
    }
    const onWheel = (event: WheelEvent) => {
      event.preventDefault()
      const prev = transform.k
      const next = Math.min(2.5, Math.max(0.25, prev * (event.deltaY < 0 ? 1.12 : 1 / 1.12)))
      const wx = (event.offsetX - transform.x) / prev
      const wy = (event.offsetY - transform.y) / prev
      transform.k = next
      transform.x = event.offsetX - wx * next
      transform.y = event.offsetY - wy * next
      movedRef.current = true
      fitAfterRef.current = false
      draw(false)
    }
    const onDouble = (event: MouseEvent) => {
      if (dragged) return
      const hit = hitTest(event.offsetX, event.offsetY)
      if (!hit) return
      selectRef.current(hit.id)
      expandRef.current(hit)
    }

    let sawPointer = 0
    const pointerRecent = () => performance.now() - sawPointer < 80
    const rememberPointer = () => {
      sawPointer = performance.now()
    }
    const onMouseDown = (event: MouseEvent) => {
      if (pointerRecent()) return
      onPointerDown(event as PointerEvent)
    }
    const onMouseMove = (event: MouseEvent) => {
      if (pointerRecent()) return
      onPointerMove(event as PointerEvent)
    }
    const onMouseUp = (event: MouseEvent) => {
      if (pointerRecent()) return
      onPointerUp(event as PointerEvent)
    }
    const onPointerDownWrapped = (event: PointerEvent) => {
      rememberPointer()
      onPointerDown(event)
    }
    const onPointerMoveWrapped = (event: PointerEvent) => {
      rememberPointer()
      onPointerMove(event)
    }
    const onPointerUpWrapped = (event: PointerEvent) => {
      rememberPointer()
      onPointerUp(event)
    }
    canvas.addEventListener('pointerdown', onPointerDownWrapped)
    canvas.addEventListener('pointermove', onPointerMoveWrapped)
    canvas.addEventListener('pointerup', onPointerUpWrapped)
    canvas.addEventListener('pointerleave', onPointerLeave)
    canvas.addEventListener('mousedown', onMouseDown)
    canvas.addEventListener('mousemove', onMouseMove)
    canvas.addEventListener('mouseup', onMouseUp)
    canvas.addEventListener('wheel', onWheel, { passive: false })
    canvas.addEventListener('dblclick', onDouble)
    let lastW = 0
    let lastH = 0
    const observer = new ResizeObserver(() => {
      const w = canvas.clientWidth
      const h = canvas.clientHeight
      const jumped = Math.abs(w - lastW) > 48 || Math.abs(h - lastH) > 48
      lastW = w
      lastH = h
      if (jumped && w > 40 && h > 40 && sim.nodes().length > 0) fit()
      else draw(false)
    })
    observer.observe(canvas)
    const theme = new MutationObserver(() => {
      paletteRef.current = null
      draw(false)
    })
    theme.observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme'] })
    draw(false)

    return () => {
      alive = false
      sim.on('tick', null)
      sim.on('end', null)
      sim.stop()
      observer.disconnect()
      theme.disconnect()
      canvas.removeEventListener('pointerdown', onPointerDownWrapped)
      canvas.removeEventListener('pointermove', onPointerMoveWrapped)
      canvas.removeEventListener('pointerup', onPointerUpWrapped)
      canvas.removeEventListener('pointerleave', onPointerLeave)
      canvas.removeEventListener('mousedown', onMouseDown)
      canvas.removeEventListener('mousemove', onMouseMove)
      canvas.removeEventListener('mouseup', onMouseUp)
      canvas.removeEventListener('wheel', onWheel)
      canvas.removeEventListener('dblclick', onDouble)
      if (simRef.current === sim) simRef.current = null
    }
  }, [])

  const layoutFocus = useRef('')
  useEffect(() => {
    const sim = simRef.current
    const canvas = canvasRef.current
    if (!sim || !canvas) return
    const reuse = layoutFocus.current === focusId
    layoutFocus.current = focusId
    if (!reuse) apiRef.current.refocus()
    const previous = reuse ? new Map(sim.nodes().map((node) => [node.id, node])) : new Map<string, SimNode>()
    const next: SimNode[] = scene.nodes.map((node, index) => {
      const old = previous.get(node.id)
      if (!old) {
        const pos = seed(node.kind, index, scene.nodes.length)
        return { ...node, x: pos.x, y: pos.y, vx: 0, vy: 0 }
      }
      old.kind = node.kind
      old.caption = node.caption
      old.subtitle = node.subtitle
      old.rawCaption = node.rawCaption
      old.objectId = node.objectId
      old.properties = node.properties
      return old
    })
    const links: SimLink[] = scene.edges.map((edge) => ({
      id: edge.id,
      source: edge.source,
      target: edge.target,
      label: edge.label,
    }))
    const oldNodes = sim.nodes()
    const sameNodes = reuse && oldNodes.length === next.length && next.every((node, index) => oldNodes[index]?.id === node.id)
    const previousLinks = sim.force<ForceLink<SimNode, SimLink>>('link')?.links() ?? []
    const sameLinks = previousLinks.length === links.length && links.every((link, index) => previousLinks[index]?.id === link.id)
    if (!sameNodes) sim.nodes(next)
    if (!sameLinks) sim.force<ForceLink<SimNode, SimLink>>('link')?.links(links)
    if (!sameNodes || !sameLinks) {
      canvas.dataset.settled = 'false'
      sim.alpha(previous.size === 0 ? 1 : 0.7).restart()
    } else drawRef.current(true)
  }, [scene, focusId])

  useEffect(() => {
    drawRef.current(true)
  }, [selected, hidden])

  const selectedNode = scene.nodes.find((node) => node.id === selected) ?? null
  const pinned = Boolean(
    pinTick >= 0 && simRef.current?.nodes().some((node) => node.id === selected && node.fx != null && node.fy != null),
  )
  const shown = scene.nodes.filter((node) => !hidden.has(node.kind))

  const onKey = (event: ReactKeyboardEvent<HTMLCanvasElement>) => {
    if (event.key === 'Escape') {
      setSelected('')
      return
    }
    if (event.key === 'Enter' && selectedNode) {
      event.preventDefault()
      if (selectedNode.objectId && expandedRef.current.has(selectedNode.id)) onOpen(selectedNode.objectId)
      else void expand(selectedNode)
      return
    }
    if (!['ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight'].includes(event.key)) return
    event.preventDefault()
    const direction = event.key === 'ArrowUp' ? 'up' : event.key === 'ArrowDown' ? 'down' : event.key === 'ArrowLeft' ? 'left' : 'right'
    const nodes = simRef.current?.nodes() ?? []
    setSelected((current) => spatialNeighbor(nodes, hidden, current, direction))
  }

  return (
    <div className={full ? 'graph-wrap graph-full' : 'graph-wrap'} data-testid={full ? 'graph-full' : undefined}>
      <div className="toolbar">
        <button type="button" data-testid="graph-fit" onClick={() => apiRef.current.fit()}>
          {copy.fitGraph}
        </button>
        <button type="button" data-testid="graph-relayout" onClick={() => apiRef.current.relayout()}>
          {copy.relayout}
        </button>
        <button type="button" onClick={() => setFull((value) => !value)}>
          {full ? copy.closeFull : copy.openFull}
        </button>
        {model.capped ? <span className="hint">{copy.graphCapped}</span> : null}
      </div>
      <div className="graph-stage">
        <div className="graph-canvas-wrap">
          <canvas
            ref={canvasRef}
            className="graph"
            data-testid="graph"
            role="img"
            aria-label={copy.graph}
            tabIndex={0}
            onKeyDown={onKey}
          />
        </div>
        <div className="graph-legend" role="group" aria-label={copy.graphLegend}>
          {NODE_KINDS.map((kind) => {
            const visible = !hidden.has(kind)
            return (
              <button
                key={kind}
                type="button"
                className={visible ? 'legend-chip on' : 'legend-chip'}
                aria-pressed={visible}
                data-testid={`legend-${kind}`}
                onClick={() =>
                  setHidden((current) => {
                    const next = new Set(current)
                    if (next.has(kind)) next.delete(kind)
                    else next.add(kind)
                    return next
                  })
                }
              >
                <i className={`dot ${kind}`} data-swatch={kind} />
                {kindLabel(kind)}
                <span className="count">{counts[kind]}</span>
              </button>
            )
          })}
        </div>
        <aside className="graph-inspector" data-testid="graph-inspector">
          {selectedNode ? (
            <>
              <p className="graph-kicker">
                <i className={`dot ${selectedNode.kind}`} />
                {kindLabel(selectedNode.kind)}
              </p>
              <h3>{selectedNode.caption}</h3>
              <p className="hint">{selectedNode.subtitle}</p>
              <h4>{copy.graphProperties}</h4>
              <dl>
                {selectedNode.properties.map((property) => (
                  <div key={property.label}>
                    <dt>{property.label}</dt>
                    <dd>{property.value}</dd>
                  </div>
                ))}
              </dl>
              {selectedNode.objectId ? (
                <button type="button" className="primary" onClick={() => onOpen(selectedNode.objectId!)}>
                  {copy.openObject}
                </button>
              ) : null}
              {pinned ? (
                <button
                  type="button"
                  data-testid="graph-unpin"
                  onClick={() => {
                    const node = simRef.current?.nodes().find((item) => item.id === selected)
                    if (!node) return
                    node.fx = null
                    node.fy = null
                    simRef.current?.alpha(0.3).restart()
                    setPinTick((tick) => tick + 1)
                  }}
                >
                  {copy.unpin}
                </button>
              ) : null}
              {busy === selectedNode.id ? <p className="hint">{copy.loading}</p> : null}
            </>
          ) : (
            <>
              <MetaChip>{copy.graph}</MetaChip>
              <h3>{copy.graphSelect}</h3>
              <p className="hint">{copy.graphProperties}</p>
            </>
          )}
        </aside>
      </div>
      <div className="graph-a11y" data-testid="graph-nodes">
        {shown.map((node) => (
          <button
            key={node.id}
            type="button"
            data-node-id={node.id}
            data-x={markers[node.id] ? String(markers[node.id].x) : undefined}
            data-y={markers[node.id] ? String(markers[node.id].y) : undefined}
            onClick={() => setSelected(node.id)}
            onDoubleClick={() => {
              setSelected(node.id)
              void expand(node)
            }}
          >
            {node.caption}
          </button>
        ))}
      </div>
      <div className="graph-a11y" data-testid="attribute-provenance">
        <p>{copy.attributeProvenance}</p>
        {scene.nodes
          .filter((node) => node.kind === 'value')
          .map((node) => (
            <p key={node.id}>
              <b>{node.caption}</b> {node.properties.find((property) => property.label === copy.origin)?.value ?? ''}
            </p>
          ))}
      </div>
    </div>
  )
}

function seed(kind: NodeKind, index: number, total: number) {
  const ring = kind === 'object' ? 1 : kind === 'value' ? 2.1 : kind === 'class' ? 3 : kind === 'group' ? 3.6 : 4.2
  const angle = (index / Math.max(1, total)) * Math.PI * 2 + ring
  const radius = ring * 72
  return { x: Math.cos(angle) * radius, y: Math.sin(angle) * radius }
}

function spatialNeighbor(
  nodes: SimNode[],
  hidden: Set<NodeKind>,
  current: string,
  direction: 'up' | 'down' | 'left' | 'right',
) {
  const visible = nodes.filter((node) => !hidden.has(node.kind) && node.x != null && node.y != null)
  const self = visible.find((node) => node.id === current) ?? visible[0]
  if (!self) return current
  let best = self.id
  let bestScore = Infinity
  for (const node of visible) {
    if (node.id === self.id) continue
    const dx = (node.x ?? 0) - (self.x ?? 0)
    const dy = (node.y ?? 0) - (self.y ?? 0)
    const forward = direction === 'right' ? dx : direction === 'left' ? -dx : direction === 'down' ? dy : -dy
    if (forward < 8) continue
    const lateral = direction === 'left' || direction === 'right' ? Math.abs(dy) : Math.abs(dx)
    const score = lateral * 2.2 + forward
    if (score < bestScore) {
      bestScore = score
      best = node.id
    }
  }
  return best
}

function endId(end: SimLink['source']) {
  return typeof end === 'object' ? end.id : String(end)
}

function endNode(end: SimLink['source']) {
  return typeof end === 'object' ? end : undefined
}

function bendOf(id: string, len: number) {
  let hash = 0
  for (let index = 0; index < id.length; index += 1) hash = (hash * 33 + id.charCodeAt(index)) >>> 0
  const sign = hash % 2 === 0 ? 1 : -1
  return sign * Math.min(72, Math.max(18, len * 0.18))
}

function curvePoint(from: { x: number; y: number }, control: { x: number; y: number }, to: { x: number; y: number }, t: number) {
  const u = 1 - t
  return {
    x: u * u * from.x + 2 * u * t * control.x + t * t * to.x,
    y: u * u * from.y + 2 * u * t * control.y + t * t * to.y,
  }
}

function curveTangent(
  from: { x: number; y: number },
  control: { x: number; y: number },
  to: { x: number; y: number },
  t: number,
) {
  return {
    x: 2 * (1 - t) * (control.x - from.x) + 2 * t * (to.x - control.x),
    y: 2 * (1 - t) * (control.y - from.y) + 2 * t * (to.y - control.y),
  }
}

function pointAtDistance(
  from: { x: number; y: number },
  control: { x: number; y: number },
  to: { x: number; y: number },
  radius: number,
  fromEnd: boolean,
) {
  let low = 0
  let high = 1
  for (let step = 0; step < 16; step += 1) {
    const mid = (low + high) / 2
    const point = curvePoint(from, control, to, mid)
    const origin = fromEnd ? to : from
    const dist = Math.hypot(point.x - origin.x, point.y - origin.y)
    if (fromEnd) {
      if (dist > radius) low = mid
      else high = mid
    } else if (dist < radius) low = mid
    else high = mid
  }
  const t = fromEnd ? high : low
  return { t, point: curvePoint(from, control, to, t) }
}

function arrow(context: CanvasRenderingContext2D, x: number, y: number, angle: number) {
  const size = 8
  context.beginPath()
  context.moveTo(x, y)
  context.lineTo(x - size * Math.cos(angle - 0.45), y - size * Math.sin(angle - 0.45))
  context.lineTo(x - size * Math.cos(angle + 0.45), y - size * Math.sin(angle + 0.45))
  context.closePath()
  context.fill()
}

function roundRect(context: CanvasRenderingContext2D, x: number, y: number, width: number, height: number, radius: number) {
  context.beginPath()
  context.roundRect(x, y, width, height, radius)
}

function insideLines(context: CanvasRenderingContext2D, text: string, maxWidth: number) {
  if (!text) return []
  if (context.measureText(text).width <= maxWidth) return [text]
  const space = text.indexOf(' ')
  if (space > 0) {
    const first = fitText(context, text.slice(0, space), maxWidth)
    const second = fitText(context, text.slice(space + 1), maxWidth)
    if (first && second && !first.endsWith('…') && !second.endsWith('…')) return [first, second]
  }
  const fitted = fitText(context, text, maxWidth)
  return fitted ? [fitted] : []
}

function fitText(context: CanvasRenderingContext2D, text: string, maxWidth: number) {
  if (!text) return ''
  if (context.measureText(text).width <= maxWidth) return text
  let next = text
  while (next.length > 1 && context.measureText(`${next}…`).width > maxWidth) next = next.slice(0, -1)
  return next.length < 2 ? '' : `${next}…`
}

function luminance(color: string) {
  const hex = color.trim()
  if (hex.startsWith('#')) {
    const body = hex.length >= 7 ? hex.slice(1, 7) : hex.slice(1)
    const red = Number.parseInt(body.slice(0, 2), 16)
    const green = Number.parseInt(body.slice(2, 4), 16)
    const blue = Number.parseInt(body.slice(4, 6), 16)
    if ([red, green, blue].some((channel) => Number.isNaN(channel))) return 0
    return (0.2126 * red + 0.7152 * green + 0.0722 * blue) / 255
  }
  const match = color.match(/[\d.]+/g)
  if (!match || match.length < 3) return 0
  const [red, green, blue] = match.slice(0, 3).map(Number)
  return (0.2126 * red + 0.7152 * green + 0.0722 * blue) / 255
}

function ink(fill: string, text: string, bg: string) {
  const filled = luminance(fill)
  return Math.abs(filled - luminance(text)) >= Math.abs(filled - luminance(bg)) ? text : bg
}
