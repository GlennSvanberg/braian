import {
  forceCenter,
  forceCollide,
  forceLink,
  forceManyBody,
  forceSimulation,
  type Force,
  type ForceLink,
  type Simulation,
  type SimulationLinkDatum,
  type SimulationNodeDatum,
} from 'd3-force'
import { ChevronDown, X } from 'lucide-react'
import { useEffect, useLayoutEffect, useMemo, useRef, useState, type KeyboardEvent as ReactKeyboardEvent, type PointerEvent as ReactPointerEvent, type RefObject } from 'react'
import { api, textValue, type Json } from './api'
import { MetaChip, typeLabel } from './chips'
import {
  buildScene,
  capScene,
  expandAssortment,
  expandClass,
  groupMembers,
  isLegendHidden,
  kindLabel,
  labelPriority,
  legendEntries,
  mergeScene,
  nodeMark,
  nodeRadius,
  parseTypedId,
  searchDocuments,
  visibleLabelIds,
  type KnownObject,
  type LabelBox,
  type NodeKind,
  type Scene,
  type SceneNode,
} from './graph-scene'
import {
  detailPlacement,
  fitCamera,
  graphDensity,
  inspectorChrome,
  legendLayout,
  nodeSummary,
  propertiesOpen,
  readGraphFrame,
  sheetAfterGesture,
  type Box,
  type DetailPlacement,
  type GraphDensity,
  type SheetMode,
} from './graph-chrome'
import { openObjectLabel } from './type-labels'
import { copy, uiLanguage } from './i18n'

type SimNode = SimulationNodeDatum & SceneNode & { degree: number }
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
  const [hidden, setHidden] = useState<Set<string>>(() => new Set())
  const [full, setFull] = useState(false)
  const [markers, setMarkers] = useState<Record<string, Marker>>({})
  const [pinTick, setPinTick] = useState(0)
  const [busy, setBusy] = useState('')
  const [sheet, setSheet] = useState<SheetMode>('compact')
  const [pane, setPane] = useState<{ width: number; height: number } | null>(null)
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
    setSheet('compact')
    expandedRef.current = new Set([focusKey])
  }

  const model = useMemo(() => {
    let merged = buildScene({ graph: data, values, defs, known })
    for (const extra of extras) merged = mergeScene(merged, extra)
    return capScene(merged)
  }, [data, values, defs, known, extras])
  const scene = model.scene
  const placed = useMemo(() => {
    const nodes = scene.nodes.filter((node) => !(node.kind === 'object' && node.rawCaption))
    const ids = new Set(nodes.map((node) => node.id))
    return {
      nodes,
      edges: scene.edges.filter((edge) => ids.has(edge.source) && ids.has(edge.target)),
    }
  }, [scene])
  const pendingNames = useMemo(
    () =>
      scene.nodes
        .filter((node) => node.kind === 'object' && node.rawCaption && node.objectId)
        .map((node) => node.objectId ?? '')
        .filter(Boolean)
        .sort()
        .join(','),
    [scene],
  )
  const knownRef = useRef(known)
  knownRef.current = known
  const fitAfterRef = useRef(true)

  useEffect(() => {
    const ids = pendingNames.split(',').filter(Boolean)
    if (!ids.length) return
    let cancel = false
    const settle = (found: Record<string, KnownObject>) => {
      if (cancel) return
      setKnown((current) => {
        const next = { ...current }
        let changed = false
        for (const id of ids) {
          if (next[id]) continue
          next[id] = found[id] ?? { name: '', typeKey: '' }
          changed = true
        }
        return changed ? next : current
      })
    }
    void lookupNames(ids).then(settle).catch(() => settle({}))
    return () => {
      cancel = true
    }
  }, [pendingNames])

  const legend = useMemo(() => legendEntries(placed.nodes), [placed])
  const counts = useMemo(() => {
    const tally: Record<string, number> = {}
    for (const node of placed.nodes) {
      const id = node.kind === 'object' && node.typeKey ? `type-${node.typeKey}` : node.kind
      tally[id] = (tally[id] ?? 0) + 1
    }
    return tally
  }, [placed])

  const canvasRef = useRef<HTMLCanvasElement>(null)
  const stageRef = useRef<HTMLDivElement>(null)
  const inspectorRef = useRef<HTMLElement>(null)
  const legendRef = useRef<HTMLDivElement>(null)
  const densityRef = useRef<GraphDensity>('wide')
  const frameKeyRef = useRef('')
  const simRef = useRef<Simulation<SimNode, SimLink> | null>(null)
  const drawRef = useRef<(shouldPublish: boolean) => void>(() => {})
  const apiRef = useRef({ fit: () => {}, relayout: () => {}, refocus: () => {} })
  const hiddenRef = useRef(hidden)
  const selectedRef = useRef(selected)
  const expandRef = useRef<(node: SceneNode) => void>(() => {})
  const selectRef = useRef<(id: string) => void>(() => {})
  const pinRef = useRef(setPinTick)
  hiddenRef.current = hidden
  selectedRef.current = selected
  selectRef.current = (id: string) => {
    setSelected(id)
    setSheet((mode) => (mode === 'dismissed' || mode === 'chip' ? 'compact' : mode))
  }
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
        const relationIds = ((graph.relations as Json[]) ?? []).map((relation) => String(relation.object_id ?? '')).filter(Boolean)
        const nextKnown = await resolveNames([String(graph.id ?? node.objectId), ...relationIds], knownRef.current)
        knownRef.current = nextKnown
        setKnown(nextKnown)
        extra = buildScene({
          graph,
          values: (object.values as Json) ?? {},
          defs: defsRef.current,
          known: nextKnown,
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
              typeKey: String(doc.type_key ?? ''),
              rawCaption: !(english || fallback),
            }
          })
          const nextKnown = await resolveNames(
            hits.map((hit) => hit.id),
            knownRef.current,
          )
          knownRef.current = nextKnown
          setKnown(nextKnown)
          const named = hits.map((hit) => {
            const record = nextKnown[hit.id]
            const name = record?.name || hit.name
            const type = record?.typeKey ? typeLabel(defsRef.current, record.typeKey, uiLanguage) || hit.type : hit.type
            return { ...hit, name, type, rawCaption: !name }
          })
          extra = groupMembers(parsed.structure, parsed.key, named)
        }
      }
      fitAfterRef.current = true
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
    const paletteRef: { current: Palette | null } = { current: null }
    const markerSerial = { current: '' }
    let alive = true

    const sim = forceSimulation<SimNode, SimLink>([])
      .force(
        'link',
        forceLink<SimNode, SimLink>([])
          .id((node) => node.id)
          .distance((link) => 72 + endRadius(link.source) + endRadius(link.target))
          .strength(0.25),
      )
      .force('charge', forceManyBody<SimNode>().strength(-560).distanceMax(820))
      .force('center', forceCenter(0, 0))
      .force('collide', forceCollide<SimNode>().radius((node) => nodeRadius(node.kind, node.degree ?? 0) + 4).iterations(2))
      .force('labels', labelCollision())
      .alphaDecay(reduced ? 0.2 : 0.032)
      .velocityDecay(0.5)
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
        if (!node || isLegendHidden(hiddenRef.current, node) || node.x == null || node.y == null) continue
        const reach = nodeRadius(node.kind, node.degree ?? 0) + slop
        if ((node.x - wx) ** 2 + (node.y - wy) ** 2 <= reach ** 2) return node
      }
      return undefined
    }

    const publish = () => {
      const rect = canvas.getBoundingClientRect()
      const next: Record<string, Marker> = {}
      for (const node of sim.nodes()) {
        if (isLegendHidden(hiddenRef.current, node) || node.x == null || node.y == null) continue
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
        if (isLegendHidden(hiddenNow, source) || isLegendHidden(hiddenNow, target)) return
        const from = screenOf(source.x, source.y)
        const to = screenOf(target.x, target.y)
        const dx = to.x - from.x
        const dy = to.y - from.y
        const len = Math.hypot(dx, dy) || 1
        const bend = bendOf(link.id, len)
        const control = { x: (from.x + to.x) / 2 + (-dy / len) * bend, y: (from.y + to.y) / 2 + (dx / len) * bend }
        const targetR = nodeRadius(target.kind, target.degree ?? 0) * transform.k
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

      const placedNodes: { node: SimNode; point: { x: number; y: number }; radius: number }[] = []
      for (const node of nodes) {
        if (isLegendHidden(hiddenNow, node) || node.x == null || node.y == null) continue
        const point = screenOf(node.x, node.y)
        const radius = nodeRadius(node.kind, node.degree ?? 0) * transform.k
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
        const mark = nodeMark(node.caption)
        if (mark && radius >= 10) {
          context.fillStyle = ink(fill, colors.text, colors.bg)
          context.font = `600 ${Math.max(11, Math.min(15, radius * 0.46))}px "Inter Variable", Inter, sans-serif`
          context.fillText(mark, point.x, point.y)
        }
        context.restore()
        placedNodes.push({ node, point, radius })
      }

      context.font = '12px "Inter Variable", Inter, sans-serif'
      const boxes: LabelBox[] = placedNodes.map(({ node, point, radius }) => {
        const width = context.measureText(node.caption).width
        const active = node.id === current || highlight.has(node.id)
        return {
          id: node.id,
          priority: labelPriority(node.kind, active),
          left: point.x - width / 2 - 4,
          right: point.x + width / 2 + 4,
          top: point.y + radius + 4,
          bottom: point.y + radius + 20,
        }
      })
      const labels = visibleLabelIds(boxes, transform.k)
      for (const { node, point, radius } of placedNodes) {
        if (!labels.has(node.id) || !node.caption) continue
        const emphasized = !hover || highlight.has(node.id)
        const active = node.id === current || highlight.has(node.id)
        context.save()
        context.globalAlpha = emphasized ? 1 : 0.28
        haloText(context, node.caption, point.x, point.y + radius + 12, colors.text, colors.bg)
        if (active && node.subtitle && transform.k >= 0.6) {
          context.font = '10px "Inter Variable", Inter, sans-serif'
          context.lineWidth = 3
          context.strokeStyle = colors.bg
          context.strokeText(node.subtitle, point.x, point.y + radius + 26)
          context.fillStyle = colors.muted
          context.fillText(node.subtitle, point.x, point.y + radius + 26)
        }
        context.restore()
      }

      if (shouldPublish) publish()
    }

    const fit = () => {
      resize()
      const nodes = sim.nodes().filter((node) => !isLegendHidden(hiddenRef.current, node) && node.x != null && node.y != null)
      const width = canvas.clientWidth
      const height = canvas.clientHeight
      if (!nodes.length || width < 20 || height < 20) return
      let minX = Infinity
      let minY = Infinity
      let maxX = -Infinity
      let maxY = -Infinity
      for (const node of nodes) {
        const radius = nodeRadius(node.kind, node.degree ?? 0)
        const half = Math.min(150, Math.max(radius + 12, node.caption.length * 3.6 + 12))
        minX = Math.min(minX, node.x! - half)
        maxX = Math.max(maxX, node.x! + half)
        minY = Math.min(minY, node.y! - radius - 8)
        maxY = Math.max(maxY, node.y! + radius + 32)
      }
      const stage = { width, height }
      const frame = readGraphFrame(stage, measureChromeBoxes(canvas, [inspectorRef.current, legendRef.current]), densityRef.current)
      const camera = fitCamera({ frame, bounds: { minX, minY, maxX, maxY } })
      transform.k = camera.k
      transform.x = camera.x
      transform.y = camera.y
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
    const tally = new Map<string, number>()
    for (const edge of placed.edges) {
      tally.set(edge.source, (tally.get(edge.source) ?? 0) + 1)
      tally.set(edge.target, (tally.get(edge.target) ?? 0) + 1)
    }
    const previous = reuse ? new Map(sim.nodes().map((node) => [node.id, node])) : new Map<string, SimNode>()
    let captionsChanged = false
    const next: SimNode[] = placed.nodes.map((node, index) => {
      const degree = tally.get(node.id) ?? 0
      const old = previous.get(node.id)
      if (!old) {
        const pos = seed(node.kind, index, placed.nodes.length)
        return { ...node, degree, x: pos.x, y: pos.y, vx: 0, vy: 0 }
      }
      if (old.caption !== node.caption || old.degree !== degree) captionsChanged = true
      old.kind = node.kind
      old.caption = node.caption
      old.subtitle = node.subtitle
      old.rawCaption = node.rawCaption
      old.objectId = node.objectId
      old.properties = node.properties
      old.degree = degree
      return old
    })
    const links: SimLink[] = placed.edges.map((edge) => ({
      id: edge.id,
      source: edge.source,
      target: edge.target,
      label: edge.label,
    }))
    const oldNodes = sim.nodes()
    const sameNodes = reuse && oldNodes.length === next.length && next.every((node, index) => oldNodes[index]?.id === node.id)
    const previousLinks = sim.force<ForceLink<SimNode, SimLink>>('link')?.links() ?? []
    const sameLinks = previousLinks.length === links.length && links.every((link, index) => previousLinks[index]?.id === link.id)
    if (reuse && next.length > previous.size) fitAfterRef.current = true
    if (!sameNodes) sim.nodes(next)
    if (!sameLinks) sim.force<ForceLink<SimNode, SimLink>>('link')?.links(links)
    if (!sameNodes || !sameLinks || captionsChanged) {
      canvas.dataset.settled = 'false'
      const cold = previous.size === 0
      sim.alpha(cold ? 1 : captionsChanged && sameNodes && sameLinks ? 0.35 : 0.7).restart()
    } else drawRef.current(true)
  }, [placed, focusId])

  useEffect(() => {
    drawRef.current(true)
  }, [selected, hidden])

  useLayoutEffect(() => {
    const stage = stageRef.current
    if (!stage) return
    const apply = (width: number, height: number) => {
      const next = { width: Math.floor(width), height: Math.floor(height) }
      setPane((prev) => (prev && prev.width === next.width && prev.height === next.height ? prev : next))
    }
    apply(stage.clientWidth, stage.clientHeight)
    const observer = new ResizeObserver((entries) => {
      const rect = entries[0]?.contentRect
      apply(rect?.width ?? stage.clientWidth, rect?.height ?? stage.clientHeight)
    })
    observer.observe(stage)
    return () => observer.disconnect()
  }, [])

  const selectedNode = scene.nodes.find((node) => node.id === selected) ?? null
  const density: GraphDensity = pane == null ? 'wide' : graphDensity(pane.width)
  densityRef.current = density
  const placement = detailPlacement(density, sheet, Boolean(selectedNode))
  const showProperties = propertiesOpen(density, sheet)
  const chrome = inspectorChrome(placement, pane?.height ?? 0)
  const pinned = Boolean(
    pinTick >= 0 && simRef.current?.nodes().some((node) => node.id === selected && node.fx != null && node.fy != null),
  )
  const shown = placed.nodes.filter((node) => !isLegendHidden(hidden, node))

  useLayoutEffect(() => {
    const canvas = canvasRef.current
    if (!canvas) return
    let fitting = false
    const apply = () => {
      if (fitting) return
      const stage = { width: canvas.clientWidth, height: canvas.clientHeight }
      if (stage.width < 20 || stage.height < 20) return
      const frame = readGraphFrame(stage, measureChromeBoxes(canvas, [inspectorRef.current, legendRef.current]), densityRef.current)
      const key = `${Math.round(frame.x)}:${Math.round(frame.y)}:${Math.round(frame.width)}:${Math.round(frame.height)}`
      if (key === frameKeyRef.current) return
      frameKeyRef.current = key
      fitting = true
      apiRef.current.fit()
      fitting = false
    }
    const observer = new ResizeObserver(() => apply())
    if (inspectorRef.current) observer.observe(inspectorRef.current)
    if (legendRef.current) observer.observe(legendRef.current)
    apply()
    return () => observer.disconnect()
  }, [density, sheet, placement, full, selected])

  const revealSheet = () => setSheet((mode) => (mode === 'dismissed' || mode === 'chip' ? 'compact' : mode))

  const onKey = (event: ReactKeyboardEvent<HTMLCanvasElement>) => {
    if (event.key === 'Escape') {
      if (density === 'narrow' && sheet !== 'dismissed' && selected) {
        setSheet('dismissed')
        return
      }
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
    revealSheet()
  }

  const onStageKey = (event: ReactKeyboardEvent<HTMLDivElement>) => {
    if (event.key !== 'Escape' || event.target === canvasRef.current) return
    if (density !== 'narrow' || sheet === 'dismissed' || !selected) return
    setSheet('dismissed')
  }

  const unpin = () => {
    const node = simRef.current?.nodes().find((item) => item.id === selected)
    if (!node) return
    node.fx = null
    node.fy = null
    simRef.current?.alpha(0.3).restart()
    setPinTick((tick) => tick + 1)
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
      <div
        className="graph-stage"
        ref={stageRef}
        data-testid="graph-stage"
        data-density={pane ? density : undefined}
        data-legend={pane ? legendLayout(density) : undefined}
        onKeyDown={onStageKey}
      >
        <div className="graph-frame">
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
          <div className="graph-legend" ref={legendRef} role="group" aria-label={copy.graphLegend}>
            {legend.map((entry) => {
              const visible = !hidden.has(entry.id)
              return (
                <button
                  key={entry.id}
                  type="button"
                  className={visible ? 'legend-chip on' : 'legend-chip'}
                  aria-pressed={visible}
                  data-testid={`legend-${entry.id}`}
                  title={entry.label}
                  onClick={() =>
                    setHidden((current) => {
                      const next = new Set(current)
                      if (next.has(entry.id)) next.delete(entry.id)
                      else next.add(entry.id)
                      return next
                    })
                  }
                >
                  <i className={`dot ${entry.kind}`} data-swatch={entry.kind} />
                  <span className="legend-label">{entry.label}</span>
                  <span className="count">{counts[entry.id] ?? 0}</span>
                </button>
              )
            })}
          </div>
          <GraphInspector
            node={selectedNode}
            placement={placement}
            showProperties={showProperties}
            pinned={pinned}
            busy={busy === selectedNode?.id}
            maxHeightPx={chrome.maxHeightPx}
            inspectorRef={inspectorRef}
            onOpen={onOpen}
            onDismiss={() => setSheet('dismissed')}
            onCollapse={() => setSheet('chip')}
            onReveal={() => setSheet('compact')}
            onSwipe={(deltaY) => setSheet((mode) => sheetAfterGesture(mode, deltaY))}
            onUnpin={unpin}
          />
        </div>
      </div>
      <div className="graph-a11y" data-testid="graph-nodes">
        {shown.map((node) => (
          <button
            key={node.id}
            type="button"
            data-node-id={node.id}
            data-x={markers[node.id] ? String(markers[node.id].x) : undefined}
            data-y={markers[node.id] ? String(markers[node.id].y) : undefined}
            onClick={() => selectRef.current(node.id)}
            onDoubleClick={() => {
              selectRef.current(node.id)
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

function GraphInspector({
  node,
  placement,
  showProperties,
  pinned,
  busy,
  maxHeightPx,
  inspectorRef,
  onOpen,
  onDismiss,
  onCollapse,
  onReveal,
  onSwipe,
  onUnpin,
}: {
  node: SceneNode | null
  placement: DetailPlacement
  showProperties: boolean
  pinned: boolean
  busy: boolean
  maxHeightPx: number
  inspectorRef: RefObject<HTMLElement | null>
  onOpen: (id: string) => void
  onDismiss: () => void
  onCollapse: () => void
  onReveal: () => void
  onSwipe: (deltaY: number) => void
  onUnpin: () => void
}) {
  const drag = useRef({ y: 0, id: -1 })
  if (placement === 'hidden') return null
  const style = maxHeightPx > 0 ? { maxHeight: maxHeightPx } : undefined
  if (!node) {
    return (
      <aside ref={inspectorRef} className="graph-inspector" data-testid="graph-inspector" data-placement="side" style={style}>
        <MetaChip>{copy.graph}</MetaChip>
        <h3>{copy.graphSelect}</h3>
        <p className="hint">{copy.graphProperties}</p>
      </aside>
    )
  }
  const shownKind = node.kind === 'object' && node.typeName ? node.typeName : kindLabel(node.kind)
  const summary = nodeSummary(
    { caption: node.caption, subtitle: node.subtitle, kindLabel: shownKind, properties: node.properties },
    { type: copy.type, number: copy.number },
  )
  if (placement === 'chip') {
    return (
      <aside ref={inspectorRef} className="graph-inspector" data-testid="graph-inspector" data-placement="chip" aria-label={copy.graph} style={style}>
        <button type="button" className="graph-sheet-chip" data-testid="graph-sheet-chip" onClick={onReveal}>
          <i className={`dot ${node.kind}`} />
          <span>{summary.name}</span>
        </button>
      </aside>
    )
  }
  const onHandleDown = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (event.button !== 0) return
    drag.current = { y: event.clientY, id: event.pointerId }
    event.currentTarget.setPointerCapture(event.pointerId)
  }
  const onHandleUp = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (event.pointerId !== drag.current.id) return
    const delta = event.clientY - drag.current.y
    drag.current.id = -1
    onSwipe(delta)
  }
  return (
    <aside
      ref={inspectorRef}
      className="graph-inspector"
      data-testid="graph-inspector"
      data-placement={placement}
      aria-label={copy.graph}
      style={style}
    >
      {placement === 'sheet' ? (
        <div className="graph-sheet-sticky">
          <div
            className="graph-sheet-handle"
            data-testid="graph-sheet-handle"
            role="button"
            tabIndex={0}
            aria-label={copy.graphProperties}
            onPointerDown={onHandleDown}
            onPointerUp={onHandleUp}
            onKeyDown={(event) => {
              if (event.key !== 'Enter' && event.key !== ' ') return
              event.preventDefault()
              onSwipe(0)
            }}
          />
          <div className="graph-sheet-bar">
            <p className="graph-kicker">
              <i className={`dot ${node.kind}`} />
              <span className="graph-kicker-text">{summary.type}</span>
            </p>
            <button type="button" className="icon-btn" data-testid="graph-collapse" aria-label={copy.collapse} onClick={onCollapse}>
              <ChevronDown size={14} aria-hidden="true" />
            </button>
            <button type="button" className="icon-btn" data-testid="graph-dismiss" aria-label={copy.graphDismiss} onClick={onDismiss}>
              <X size={14} aria-hidden="true" />
            </button>
          </div>
        </div>
      ) : (
        <p className="graph-kicker">
          <i className={`dot ${node.kind}`} />
          {shownKind}
        </p>
      )}
      <h3>{placement === 'sheet' ? summary.name : node.caption}</h3>
      {placement === 'sheet' ? (
        summary.detail ? <p className="graph-number">{summary.detail}</p> : null
      ) : (
        <p className="hint">{node.subtitle}</p>
      )}
      {placement === 'sheet' && node.objectId ? (
        <button type="button" className="primary graph-action" onClick={() => onOpen(node.objectId!)}>
          {openObjectLabel(node.typeName)}
        </button>
      ) : null}
      {placement === 'sheet' && node.properties.length > 0 ? (
        <button
          type="button"
          className="graph-action"
          data-testid="graph-properties-toggle"
          aria-expanded={showProperties}
          onClick={() => onSwipe(0)}
        >
          {copy.graphProperties}
        </button>
      ) : null}
      {showProperties ? (
        <>
          {placement === 'side' ? <h4>{copy.graphProperties}</h4> : null}
          <dl data-testid="graph-properties">
            {node.properties.map((property) => (
              <div key={property.label}>
                <dt>{property.label}</dt>
                <dd>{property.value}</dd>
              </div>
            ))}
          </dl>
        </>
      ) : null}
      {placement === 'side' && node.objectId ? (
        <button type="button" className="primary graph-action" onClick={() => onOpen(node.objectId!)}>
          {openObjectLabel(node.typeName)}
        </button>
      ) : null}
      {pinned ? (
        <button type="button" className="graph-action" data-testid="graph-unpin" onClick={onUnpin}>
          {copy.unpin}
        </button>
      ) : null}
      {busy ? <p className="hint">{copy.loading}</p> : null}
    </aside>
  )
}

function measureChromeBoxes(canvas: HTMLCanvasElement, elements: Array<HTMLElement | null>): Box[] {
  const origin = canvas.getBoundingClientRect()
  const boxes: Box[] = []
  for (const element of elements) {
    if (!element) continue
    const style = getComputedStyle(element)
    if (style.display === 'none' || style.visibility === 'hidden') continue
    const rect = element.getBoundingClientRect()
    if (rect.width < 1 || rect.height < 1) continue
    boxes.push({
      x: rect.left - origin.left,
      y: rect.top - origin.top,
      width: rect.width,
      height: rect.height,
    })
  }
  return boxes
}

function seed(kind: NodeKind, index: number, total: number) {
  const ring = kind === 'object' ? 1 : kind === 'value' ? 2.1 : kind === 'class' ? 3 : kind === 'group' ? 3.6 : 4.2
  const angle = (index / Math.max(1, total)) * Math.PI * 2 + ring
  const radius = ring * 72
  return { x: Math.cos(angle) * radius, y: Math.sin(angle) * radius }
}

function spatialNeighbor(
  nodes: SimNode[],
  hidden: Set<string>,
  current: string,
  direction: 'up' | 'down' | 'left' | 'right',
) {
  const visible = nodes.filter((node) => !isLegendHidden(hidden, node) && node.x != null && node.y != null)
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

function endRadius(end: SimLink['source']) {
  if (typeof end === 'object') return nodeRadius(end.kind, end.degree ?? 0)
  return 22
}

function labelBox(node: SimNode) {
  const radius = nodeRadius(node.kind, node.degree ?? 0)
  const labelW = Math.min(280, Math.max(radius * 2, node.caption.length * 7.2 + 16))
  const half = labelW / 2
  return {
    left: (node.x ?? 0) - half,
    right: (node.x ?? 0) + half,
    top: (node.y ?? 0) - radius - 2,
    bottom: (node.y ?? 0) + radius + 22,
  }
}

function labelCollision(): Force<SimNode, SimLink> {
  let nodes: SimNode[] = []
  const force: Force<SimNode, SimLink> = (alpha: number) => {
    for (let i = 0; i < nodes.length; i += 1) {
      const a = nodes[i]
      if (!a || a.x == null || a.y == null) continue
      const boxA = labelBox(a)
      for (let j = i + 1; j < nodes.length; j += 1) {
        const b = nodes[j]
        if (!b || b.x == null || b.y == null) continue
        const boxB = labelBox(b)
        const overlapX = Math.min(boxA.right, boxB.right) - Math.max(boxA.left, boxB.left)
        const overlapY = Math.min(boxA.bottom, boxB.bottom) - Math.max(boxA.top, boxB.top)
        if (overlapX <= 0 || overlapY <= 0) continue
        const aPinned = a.fx != null
        const bPinned = b.fx != null
        if (aPinned && bPinned) continue
        const push = alpha * 0.85
        if (overlapX < overlapY) {
          const sign = (a.x ?? 0) <= (b.x ?? 0) ? -1 : 1
          const mag = overlapX * 0.5 * push
          if (!aPinned) a.vx = (a.vx ?? 0) + sign * mag * (bPinned ? 2 : 1)
          if (!bPinned) b.vx = (b.vx ?? 0) - sign * mag * (aPinned ? 2 : 1)
        } else {
          const sign = (a.y ?? 0) <= (b.y ?? 0) ? -1 : 1
          const mag = overlapY * 0.5 * push
          if (!aPinned) a.vy = (a.vy ?? 0) + sign * mag * (bPinned ? 2 : 1)
          if (!bPinned) b.vy = (b.vy ?? 0) - sign * mag * (aPinned ? 2 : 1)
        }
      }
    }
  }
  force.initialize = (next: SimNode[]) => {
    nodes = next
  }
  return force
}

async function lookupNames(ids: string[]): Promise<Record<string, KnownObject>> {
  const unique = [...new Set(ids.filter(Boolean))]
  const found: Record<string, KnownObject> = {}
  for (let index = 0; index < unique.length; index += 400) {
    const slice = unique.slice(index, index + 400)
    const body = await api<Json>('/v1/objects/names', {
      method: 'POST',
      body: JSON.stringify({ ids: slice }),
    })
    const rows = (body.objects as Json[] | undefined) ?? []
    for (const row of rows) {
      const id = String(row.id ?? '')
      if (!id) continue
      found[id] = { name: String(row.name ?? '').trim(), typeKey: String(row.type ?? '') }
    }
  }
  return found
}

async function resolveNames(ids: string[], base: Record<string, KnownObject>) {
  const missing = [...new Set(ids.filter((id) => id && !base[id]))]
  if (!missing.length) return base
  try {
    const found = await lookupNames(missing)
    const next = { ...base }
    for (const id of missing) next[id] = found[id] ?? { name: '', typeKey: '' }
    return next
  } catch {
    return base
  }
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

function haloText(context: CanvasRenderingContext2D, text: string, x: number, y: number, fill: string, plate: string) {
  context.font = '12px "Inter Variable", Inter, sans-serif'
  const width = context.measureText(text).width
  const padX = 5
  const height = 16
  context.fillStyle = plate
  roundRect(context, x - width / 2 - padX, y - height / 2, width + padX * 2, height, 4)
  context.fill()
  context.lineWidth = 3
  context.strokeStyle = plate
  context.strokeText(text, x, y)
  context.fillStyle = fill
  context.fillText(text, x, y)
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
