import { textValue, type Json } from './api'
import { optionLabel, typeLabel } from './chips'
import { copy, uiLanguage } from './i18n'

/** Five node kinds. Colours come from DESIGN.md tokens in the canvas, not from here. */
export const NODE_KINDS = ['object', 'class', 'group', 'value', 'assortment'] as const
export type NodeKind = (typeof NODE_KINDS)[number]

export const NODE_RADIUS: Record<NodeKind, number> = {
  object: 22,
  class: 16,
  group: 18,
  value: 15,
  assortment: 20,
}

/** Circle size grows with how many edges the node has, capped so hubs stay readable. */
export function nodeRadius(kind: NodeKind, degree: number) {
  const links = Number.isFinite(degree) ? degree : 0
  const extra = Math.min(14, Math.max(0, links - 1) * 2)
  return NODE_RADIUS[kind] + extra
}

/** One or two letters drawn inside the circle. Never a clipped name. */
export function nodeMark(caption: string) {
  const words = caption
    .split(/[\s/]+/)
    .map((word) => word.trim())
    .filter(Boolean)
  const first = words[0] ?? ''
  if (!first) return ''
  if (words.length === 1) {
    const chars = [...first]
    if (chars.length <= 3) return first
    return chars[0] ?? ''
  }
  const a = [...first][0] ?? ''
  const b = [...(words[1] ?? '')][0] ?? ''
  return `${a}${b}`.toLocaleUpperCase('en')
}

export function labelPriority(kind: NodeKind, active: boolean) {
  if (active) return 5
  if (kind === 'object') return 3
  if (kind === 'assortment' || kind === 'group') return 2
  if (kind === 'class') return 1
  return 0
}

export function labelVisibleAtZoom(priority: number, zoom: number) {
  if (priority >= 5) return true
  if (zoom < 0.5) return priority >= 2
  if (zoom < 0.7) return priority >= 1
  return true
}

export type LabelBox = { id: string; priority: number; left: number; top: number; right: number; bottom: number }

export function visibleLabelIds(boxes: LabelBox[], zoom: number) {
  const sorted = [...boxes].sort((left, right) => right.priority - left.priority || left.id.localeCompare(right.id))
  const kept: LabelBox[] = []
  const ids = new Set<string>()
  for (const box of sorted) {
    if (!labelVisibleAtZoom(box.priority, zoom)) continue
    if (kept.some((other) => boxesOverlap(other, box))) continue
    kept.push(box)
    ids.add(box.id)
  }
  return ids
}

function boxesOverlap(left: LabelBox, right: LabelBox) {
  return left.left < right.right && right.left < left.right && left.top < right.bottom && right.top < left.bottom
}

export const SCENE_NODE_CAP = 400

export type SceneProperty = { label: string; value: string }

export type SceneNode = {
  id: string
  kind: NodeKind
  caption: string
  subtitle: string
  /** True when the caption is still a key a later fetch can replace. */
  rawCaption: boolean
  objectId?: string
  typeKey?: string
  typeName?: string
  properties: SceneProperty[]
}

export type SceneEdge = {
  id: string
  source: string
  target: string
  label: string
}

export type Scene = { nodes: SceneNode[]; edges: SceneEdge[] }

export type KnownObject = { name: string; typeKey: string }

const STRUCTURE_NAMES: Record<string, string> = {
  friluft: copy.structureOutdoor,
  talt: copy.assortmentTents,
  sovsack: copy.structureSleepingBags,
  bygg: copy.structureBuilding,
  virke: copy.assortmentTimber,
}

/** Display name for a structure or group. Labels on the model win; known demo group keys stay as a fallback. */
export function structureCaption(key: string, defs?: Json, structureKey?: string) {
  const fromModel = captionFromDefs(defs, key, structureKey)
  if (fromModel) return fromModel
  return STRUCTURE_NAMES[key] ?? key
}

function captionFromDefs(defs: Json | undefined, key: string, structureKey?: string) {
  const structures = (defs?.structures as Json[]) ?? []
  const preferred = structureKey ? structures.filter((item) => String(item.key) === structureKey) : []
  const rest = structureKey ? structures.filter((item) => String(item.key) !== structureKey) : structures
  for (const structure of [...preferred, ...rest]) {
    if (String(structure.key) === key) {
      const label = labelText(structure.labels)
      if (label) return label
    }
    for (const group of (structure.groups as Json[]) ?? []) {
      if (String(group.key) !== key) continue
      const label = labelText(group.labels)
      if (label) return label
    }
  }
  return ''
}

function labelText(value: unknown) {
  if (!value || typeof value !== 'object') return ''
  const labels = value as Json
  const text = labels[uiLanguage] ?? labels.en ?? labels.sv
  return typeof text === 'string' ? text : ''
}

export type LegendEntry = { id: string; kind: NodeKind; label: string }

export function legendIdOf(node: { kind: NodeKind; typeKey?: string }) {
  if (node.kind === 'object' && node.typeKey) return `type-${node.typeKey}`
  return node.kind
}

export function isLegendHidden(hidden: Set<string>, node: { kind: NodeKind; typeKey?: string }) {
  return hidden.has(legendIdOf(node))
}

export function legendEntries(nodes: Array<{ kind: NodeKind; typeKey?: string; typeName?: string }>): LegendEntry[] {
  const entries: LegendEntry[] = []
  const seen = new Set<string>()
  let untyped = false
  for (const node of nodes) {
    if (node.kind !== 'object') continue
    if (!node.typeKey) {
      untyped = true
      continue
    }
    const id = `type-${node.typeKey}`
    if (seen.has(id)) continue
    seen.add(id)
    entries.push({ id, kind: 'object', label: node.typeName || node.typeKey })
  }
  if (untyped || (entries.length === 0 && nodes.some((node) => node.kind === 'object'))) {
    entries.push({ id: 'object', kind: 'object', label: copy.nodeObject })
  }
  for (const kind of ['class', 'group', 'value', 'assortment'] as const) {
    if (nodes.some((node) => node.kind === kind)) entries.push({ id: kind, kind, label: kindLabel(kind) })
  }
  return entries
}

export function kindLabel(kind: NodeKind) {
  if (kind === 'object') return copy.nodeObject
  if (kind === 'class') return copy.nodeClass
  if (kind === 'group') return copy.nodeGroup
  if (kind === 'value') return copy.nodeValue
  return copy.nodeAssortment
}

export function relationLabel(kind: string) {
  if (kind === 'article_of') return copy.articleOf
  if (kind === 'variant_of') return copy.variantOf
  if (kind === 'replaces') return copy.replacesRelation
  const words = kind.replace(/[_-]+/g, ' ').trim()
  if (!words) return copy.related
  return words.charAt(0).toUpperCase() + words.slice(1)
}

export function parseTypedId(id: string) {
  const colon = id.indexOf(':')
  const kind = colon < 0 ? id : id.slice(0, colon)
  const rest = colon < 0 ? '' : id.slice(colon + 1)
  const second = rest.indexOf(':')
  if (second < 0) return { kind, structure: rest, key: '' }
  return { kind, structure: rest.slice(0, second), key: rest.slice(second + 1) }
}

function link(source: string, label: string, target: string): SceneEdge {
  return { id: undirectedId(source, label, target), source, target, label }
}

function undirectedId(source: string, label: string, target: string) {
  const [left, right] = source < target ? [source, target] : [target, source]
  return `${left}|${label}|${right}`
}

function attributeDef(defs: Json | undefined, key: string) {
  return ((defs?.attributes as Json[]) ?? []).find((item) => String(item.key) === key || String(item.id) === key)
}

function attributeCaption(defs: Json | undefined, attribute: Json) {
  const key = String(attribute.key ?? '')
  const known = attributeDef(defs, key)
  return textValue(known?.labels, uiLanguage) || String(attribute.label ?? '') || key
}

function shorten(text: string, max = 42) {
  const clean = text.replace(/\s+/g, ' ').trim()
  if (clean.length <= max) return clean
  return `${clean.slice(0, max - 1)}…`
}

function formatWhen(value: unknown) {
  if (typeof value !== 'string' || !value) return ''
  const date = new Date(value)
  if (Number.isNaN(date.getTime())) return value
  return new Intl.DateTimeFormat('en-GB', { dateStyle: 'medium', timeStyle: 'short' }).format(date)
}

function formatValue(defs: Json | undefined, attribute: Json, raw: unknown) {
  if (raw == null || raw === '') return copy.noValue
  const known = attributeDef(defs, String(attribute.key ?? ''))
  const kind = (known?.kind as Json) ?? {}
  const kindType = String(kind.type ?? '')
  const list = String(kind.list ?? attribute.key ?? '')
  if (kindType === 'value_list') {
    const codes = Array.isArray(raw) ? raw : [raw]
    const labels = codes.map((code) => optionLabel(defs, list, textValue(code), uiLanguage)).filter(Boolean)
    return labels.join(', ') || copy.noValue
  }
  if (kindType === 'boolean' || typeof raw === 'boolean') return raw === true || raw === 'true' ? copy.yes : copy.no
  if (kindType === 'file_ref') {
    const id = textValue(raw)
    return id ? `${copy.file} ${id.slice(0, 8)}` : copy.noValue
  }
  if (kindType === 'object_ref') {
    const id = textValue(raw)
    return id ? id.slice(0, 8) : copy.noValue
  }
  const text = textValue(raw, uiLanguage)
  return text || copy.noValue
}

function putNode(nodes: Map<string, SceneNode>, node: SceneNode) {
  const existing = nodes.get(node.id)
  if (!existing) {
    nodes.set(node.id, node)
    return
  }
  if (existing.rawCaption && !node.rawCaption) {
    nodes.set(node.id, {
      ...existing,
      caption: node.caption,
      subtitle: node.subtitle || existing.subtitle,
      rawCaption: false,
      objectId: existing.objectId || node.objectId,
      typeKey: existing.typeKey || node.typeKey,
      typeName: existing.typeName || node.typeName,
      properties: node.properties.length >= existing.properties.length ? node.properties : existing.properties,
    })
  }
}

function putEdge(edges: Map<string, SceneEdge>, edge: SceneEdge) {
  if (!edges.has(edge.id)) edges.set(edge.id, edge)
}

function classNode(structure: string, classKey: string, defs?: Json): SceneNode {
  const caption = structureCaption(classKey, defs, structure)
  return {
    id: `class:${structure}:${classKey}`,
    kind: 'class',
    caption,
    subtitle: copy.nodeClass,
    rawCaption: caption === classKey,
    properties: [
      { label: copy.nodeClass, value: caption },
      { label: copy.structure, value: structureCaption(structure, defs, structure) },
    ],
  }
}

function groupNode(structure: string, groupKey: string, defs?: Json): SceneNode {
  const caption = structureCaption(groupKey, defs, structure)
  return {
    id: `group:${structure}:${groupKey}`,
    kind: 'group',
    caption,
    subtitle: copy.nodeGroup,
    rawCaption: caption === groupKey,
    properties: [
      { label: copy.nodeGroup, value: caption },
      { label: copy.structure, value: structureCaption(structure, defs, structure) },
    ],
  }
}

function sourceOrigin(source: Json, defs?: Json) {
  const structure = typeof source.structure === 'string' ? source.structure : undefined
  const inherited = Boolean(source.inherited)
  if (source.class) {
    return `${inherited ? copy.inherited : copy.fromClass}: ${structureCaption(String(source.class), defs, structure)}`
  }
  if (typeof source.group === 'string' && source.group) {
    return `${inherited ? copy.inherited : copy.fromGroup}: ${structureCaption(source.group, defs, structure)}`
  }
  return copy.fromType
}

function linkSource(
  nodes: Map<string, SceneNode>,
  edges: Map<string, SceneEdge>,
  valueId: string,
  selfId: string,
  source: Json,
  defs?: Json,
) {
  const inherited = Boolean(source.inherited)
  const structure = String(source.structure ?? 'sortiment')
  if (source.class) {
    const classKey = String(source.class)
    const classId = `class:${structure}:${classKey}`
    putNode(nodes, classNode(structure, classKey, defs))
    putEdge(edges, link(valueId, inherited ? copy.inherited : copy.fromClass, classId))
    const groupKey = typeof source.group === 'string' ? source.group : ''
    if (groupKey) {
      const groupId = `group:${structure}:${groupKey}`
      putNode(nodes, groupNode(structure, groupKey, defs))
      putNode(nodes, assortmentNode(structure, defs))
      putEdge(edges, link(classId, copy.appliesIn, groupId))
      putEdge(edges, link(groupId, copy.inAssortment, `assortment:${structure}`))
    }
    return
  }
  if (typeof source.group === 'string' && source.group) {
    const groupId = `group:${structure}:${source.group}`
    putNode(nodes, groupNode(structure, source.group, defs))
    putNode(nodes, assortmentNode(structure, defs))
    putEdge(edges, link(valueId, inherited ? copy.inherited : copy.fromGroup, groupId))
    putEdge(edges, link(groupId, copy.inAssortment, `assortment:${structure}`))
    return
  }
  putEdge(edges, link(valueId, copy.fromType, selfId))
}

function assortmentNode(structure: string, defs?: Json): SceneNode {
  const caption = structureCaption(structure, defs, structure)
  return {
    id: `assortment:${structure}`,
    kind: 'assortment',
    caption,
    subtitle: copy.nodeAssortment,
    rawCaption: caption === structure,
    properties: [{ label: copy.structure, value: caption }],
  }
}

export function buildScene(input: {
  graph: Json
  values?: Json
  defs?: Json
  known?: Record<string, KnownObject>
}): Scene {
  const nodes = new Map<string, SceneNode>()
  const edges = new Map<string, SceneEdge>()
  const objectId = String(input.graph.id ?? '')
  const key = String(input.graph.key ?? '')
  const values = input.values ?? {}
  const name = textValue(values.namn, uiLanguage)
  const selfId = `object:${objectId}`
  const typeKey = String(input.graph.type ?? '')
  const typeName = (typeKey ? typeLabel(input.defs, typeKey, uiLanguage) || typeKey : '')
  const number = textValue(values.nummer, uiLanguage) || key
  const statusCode = textValue(values.status)
  const status = statusCode ? optionLabel(input.defs, 'status', statusCode, uiLanguage) : ''
  putNode(nodes, {
    id: selfId,
    kind: 'object',
    caption: name || typeName || copy.nodeObject,
    subtitle: typeName || copy.nodeObject,
    rawCaption: false,
    objectId,
    typeKey: typeKey || undefined,
    typeName: typeName || undefined,
    properties: [
      { label: copy.type, value: typeName || typeKey || copy.nodeObject },
      { label: copy.number, value: number },
      ...(status ? [{ label: copy.status, value: status }] : []),
      { label: copy.identity, value: key || objectId },
    ],
  })

  const groups = (input.graph.groups as Json) ?? {}
  for (const [structure, groupKey] of Object.entries(groups)) {
    if (typeof groupKey !== 'string' || !groupKey) continue
    const groupId = `group:${structure}:${groupKey}`
    const assortmentId = `assortment:${structure}`
    putNode(nodes, groupNode(structure, groupKey, input.defs))
    putNode(nodes, assortmentNode(structure, input.defs))
    putEdge(edges, link(selfId, copy.placedIn, groupId))
    putEdge(edges, link(groupId, copy.inAssortment, assortmentId))
  }

  const attributes = (input.graph.attributes as Json[]) ?? []
  for (const attribute of attributes) {
    const attrKey = String(attribute.key ?? '')
    if (!attrKey) continue
    const valueId = `value:${objectId}:${attrKey}`
    const caption = attributeCaption(input.defs, attribute)
    const shown = formatValue(input.defs, attribute, values[attrKey])
    const sources = (attribute.sources as Json[]) ?? []
    const origins = sources.map((source) => sourceOrigin(source, input.defs))
    const changedBy = attribute.changed_by ? String(attribute.changed_by) : ''
    const changedAt = formatWhen(attribute.changed_at)
    putNode(nodes, {
      id: valueId,
      kind: 'value',
      caption,
      subtitle: shorten(shown, 32),
      rawCaption: caption === attrKey,
      properties: [
        { label: copy.attributes, value: caption },
        { label: copy.nodeValue, value: shown },
        ...(origins.length ? [{ label: copy.origin, value: origins.join(', ') }] : []),
        ...(changedBy ? [{ label: copy.changedBy, value: changedBy }] : []),
        ...(changedAt ? [{ label: copy.changedAt, value: changedAt }] : []),
      ],
    })
    putEdge(edges, link(selfId, copy.hasValue, valueId))
    for (const source of sources) linkSource(nodes, edges, valueId, selfId, source, input.defs)
  }

  const relations = (input.graph.relations as Json[]) ?? []
  for (const relation of relations) {
    const otherId = String(relation.object_id ?? '')
    if (!otherId) continue
    const nodeId = `object:${otherId}`
    const record = input.known?.[otherId]
    const relKey = String(relation.key ?? otherId)
    const relationType = ((input.defs?.types as Json[]) ?? []).find((item) => String(item.id) === String(relation.type_id ?? ''))
    const otherKey = record?.typeKey || String(relationType?.key ?? '')
    const typeNameForOther = otherKey ? typeLabel(input.defs, otherKey, uiLanguage) || otherKey : ''
    const caption = record?.name || typeNameForOther || copy.nodeObject
    putNode(nodes, {
      id: nodeId,
      kind: 'object',
      caption,
      subtitle: typeNameForOther || copy.nodeObject,
      rawCaption: record === undefined,
      objectId: otherId,
      typeKey: otherKey || undefined,
      typeName: typeNameForOther || undefined,
      properties: [
        { label: copy.number, value: relKey },
        { label: copy.type, value: typeNameForOther || copy.nodeObject },
        { label: copy.identity, value: relKey },
      ],
    })
    putEdge(edges, link(selfId, relationLabel(String(relation.kind ?? '')), nodeId))
  }

  return { nodes: [...nodes.values()], edges: [...edges.values()] }
}

function prefersIncoming(existing: SceneNode, incoming: SceneNode) {
  if (existing.rawCaption && !incoming.rawCaption) return true
  const incomingNamed = !incoming.rawCaption && incoming.caption !== copy.nodeObject
  const existingNamed = !existing.rawCaption && existing.caption !== copy.nodeObject
  return incomingNamed && !existingNamed
}

export function mergeScene(base: Scene, extra: Scene): Scene {
  const nodes = new Map(base.nodes.map((node) => [node.id, node]))
  for (const node of extra.nodes) {
    const existing = nodes.get(node.id)
    if (!existing) {
      nodes.set(node.id, node)
      continue
    }
    const upgrade = prefersIncoming(existing, node)
    nodes.set(node.id, {
      ...existing,
      caption: upgrade ? node.caption : existing.caption,
      subtitle: upgrade && node.subtitle ? node.subtitle : existing.subtitle,
      rawCaption: upgrade ? node.rawCaption : existing.rawCaption,
      objectId: existing.objectId || node.objectId,
      typeKey: upgrade ? node.typeKey || existing.typeKey : existing.typeKey || node.typeKey,
      typeName: upgrade ? node.typeName || existing.typeName : existing.typeName || node.typeName,
      properties: node.properties.length > existing.properties.length ? node.properties : existing.properties,
    })
  }
  const edges = new Map<string, SceneEdge>()
  for (const item of [...base.edges, ...extra.edges]) {
    const id = undirectedId(item.source, item.label, item.target)
    if (!edges.has(id)) edges.set(id, { ...item, id })
  }
  return { nodes: [...nodes.values()], edges: [...edges.values()] }
}

export function capScene(scene: Scene, limit = SCENE_NODE_CAP) {
  if (scene.nodes.length <= limit) return { scene, capped: false }
  const nodes = scene.nodes.slice(0, limit)
  const ids = new Set(nodes.map((node) => node.id))
  return {
    scene: {
      nodes,
      edges: scene.edges.filter((edge) => ids.has(edge.source) && ids.has(edge.target)),
    },
    capped: true,
  }
}

function structuresOf(defs: Json | undefined) {
  return (defs?.structures as Json[]) ?? []
}

export function classAttributes(defs: Json | undefined, structure: string, classKey: string) {
  const found = structuresOf(defs).find((item) => String(item.key) === structure)
  const classes = (found?.classes as Json[]) ?? []
  const klass = classes.find((item) => String(item.key) === classKey)
  if (!klass) return []
  const attributes = (defs?.attributes as Json[]) ?? []
  return ((klass.attributes as string[]) ?? [])
    .map((id) => attributes.find((item) => String(item.id) === id))
    .filter((item): item is Json => Boolean(item))
}

export function expandClass(defs: Json | undefined, structure: string, classKey: string): Scene {
  const found = structuresOf(defs).find((item) => String(item.key) === structure)
  const classes = (found?.classes as Json[]) ?? []
  const klass = classes.find((item) => String(item.key) === classKey)
  const nodes: SceneNode[] = [classNode(structure, classKey, defs)]
  const edges: SceneEdge[] = []
  const classId = `class:${structure}:${classKey}`
  if (klass?.parent) {
    const parent = classes.find((item) => String(item.id) === String(klass.parent))
    if (parent) {
      const parentKey = String(parent.key ?? '')
      if (parentKey) {
        nodes.push(classNode(structure, parentKey, defs))
        edges.push(link(classId, copy.parent, `class:${structure}:${parentKey}`))
      }
    }
  }
  for (const attribute of classAttributes(defs, structure, classKey)) {
    const key = String(attribute.key ?? '')
    const caption = textValue(attribute.labels, uiLanguage) || key
    const id = `value:def:${structure}:${classKey}:${key}`
    nodes.push({
      id,
      kind: 'value',
      caption,
      subtitle: copy.definedHere,
      rawCaption: caption === key,
      properties: [
        { label: copy.attributes, value: caption },
        { label: copy.nodeClass, value: structureCaption(classKey, defs, structure) },
      ],
    })
    edges.push(link(id, copy.fromClass, classId))
  }
  return { nodes, edges }
}

export type MemberHit = { id: string; key: string; name: string; type: string; typeKey?: string; rawCaption?: boolean }

export function groupMembers(structure: string, group: string, hits: MemberHit[]): Scene {
  const groupId = `group:${structure}:${group}`
  const nodes: SceneNode[] = []
  const edges: SceneEdge[] = []
  for (const hit of hits) {
    if (!hit.id) continue
    const id = `object:${hit.id}`
    const typeName = hit.type
    nodes.push({
      id,
      kind: 'object',
      caption: hit.name || typeName || copy.nodeObject,
      subtitle: typeName || copy.nodeObject,
      rawCaption: hit.rawCaption ?? !hit.name,
      objectId: hit.id,
      typeKey: hit.typeKey || undefined,
      typeName: typeName || undefined,
      properties: [
        { label: copy.number, value: hit.key || hit.id },
        { label: copy.type, value: hit.type || copy.nodeObject },
        { label: copy.nodeGroup, value: structureCaption(group) },
      ],
    })
    edges.push(link(id, copy.placedIn, groupId))
  }
  return { nodes, edges }
}

export function expandAssortment(defs: Json | undefined, structure: string): Scene {
  const found = structuresOf(defs).find((item) => String(item.key) === structure)
  const groups = (found?.groups as Json[]) ?? []
  const byId = new Map(groups.map((group) => [String(group.id), String(group.key ?? '')]))
  const nodes: SceneNode[] = [assortmentNode(structure, defs)]
  const edges: SceneEdge[] = []
  for (const group of groups) {
    const key = String(group.key ?? '')
    if (!key) continue
    const id = `group:${structure}:${key}`
    nodes.push(groupNode(structure, key, defs))
    edges.push(link(id, copy.inAssortment, `assortment:${structure}`))
    const parentKey = byId.get(String(group.parent ?? ''))
    if (parentKey) edges.push(link(id, copy.parent, `group:${structure}:${parentKey}`))
  }
  return { nodes, edges }
}

export function searchDocuments(body: Json): Json[] {
  return ((body.hits as Json[]) ?? []).map((hit) => {
    const document = hit.document
    if (document && typeof document === 'object') {
      const doc = document as Json
      if (!doc.id && hit.id) return { ...doc, id: hit.id }
      return doc
    }
    return hit
  })
}
