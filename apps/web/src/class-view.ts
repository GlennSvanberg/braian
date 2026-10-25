import type { Json } from './api'

export type AttributeCard = {
  id: string
  key: string
  label: string
  kind: string
  unit: string
  multiple: boolean
  languages: boolean
  required: boolean
  inherited: boolean
  from?: string
}

export type ClassRow = {
  key: string
  id: string
  structure: string
  parentKey?: string
  depth: number
}

type ClassNode = {
  id: string
  key: string
  parent?: string
  attributes: string[]
  structure: string
}

export function classRows(defs: Json | undefined): ClassRow[] {
  const classes = classesOf(defs)
  const byId = new Map(classes.map((item) => [item.id, item]))
  const children = new Map<string, ClassNode[]>()
  const roots: ClassNode[] = []
  for (const item of classes) {
    const parent = item.parent ? byId.get(item.parent) : undefined
    if (!parent || parent.structure !== item.structure) {
      roots.push(item)
      continue
    }
    const list = children.get(parent.id) ?? []
    list.push(item)
    children.set(parent.id, list)
  }
  const rows: ClassRow[] = []
  const walk = (item: ClassNode, depth: number) => {
    const parent = item.parent ? byId.get(item.parent) : undefined
    rows.push({
      key: item.key,
      id: item.id,
      structure: item.structure,
      parentKey: parent?.key,
      depth,
    })
    for (const child of children.get(item.id) ?? []) walk(child, depth + 1)
  }
  for (const root of roots) walk(root, 0)
  return rows
}

/** Own attributes first. An attribute listed on an ancestor is shown once, as inherited. */
export function attributeCards(defs: Json | undefined, classKey: string, language = 'en'): AttributeCard[] {
  if (!defs || !classKey) return []
  const attributes = (defs.attributes as Json[]) ?? []
  const families = (defs.unit_families as Json[]) ?? []
  const classes = classesOf(defs)
  const current = classes.find((item) => item.key === classKey)
  if (!current) return []
  const byId = new Map(classes.map((item) => [item.id, item]))
  const inheritedFrom = new Map<string, string>()
  let parentId = current.parent
  const guard = new Set<string>()
  while (parentId && !guard.has(parentId)) {
    guard.add(parentId)
    const parent = byId.get(parentId)
    if (!parent) break
    for (const id of parent.attributes) {
      if (!inheritedFrom.has(id)) inheritedFrom.set(id, parent.key)
    }
    parentId = parent.parent
  }
  const cards: AttributeCard[] = []
  const seen = new Set<string>()
  const push = (id: string, inherited: boolean, from?: string) => {
    const card = cardFor(id, attributes, families, language, inherited, from)
    if (!card || seen.has(card.key)) return
    seen.add(card.key)
    cards.push(card)
  }
  for (const id of current.attributes) {
    const from = inheritedFrom.get(id)
    if (from) push(id, true, from)
    else push(id, false)
  }
  for (const [id, from] of inheritedFrom) push(id, true, from)
  return cards
}

function classesOf(defs: Json | undefined): ClassNode[] {
  if (!defs) return []
  const rows: ClassNode[] = []
  for (const structure of (defs.structures as Json[]) ?? []) {
    for (const item of (structure.classes as Json[]) ?? []) {
      rows.push({
        id: String(item.id ?? ''),
        key: String(item.key ?? ''),
        parent: item.parent ? String(item.parent) : undefined,
        attributes: ((item.attributes as unknown[]) ?? []).map(String),
        structure: String(structure.key ?? ''),
      })
    }
  }
  return rows
}

function cardFor(
  id: string,
  attributes: Json[],
  families: Json[],
  language: string,
  inherited: boolean,
  from?: string,
): AttributeCard | null {
  const attribute = attributes.find((item) => String(item.id) === id || String(item.key) === id)
  if (!attribute) return null
  const kind = (attribute.kind as Json) ?? {}
  const labels = (attribute.labels as Json) ?? {}
  const label = String(labels[language] ?? labels.en ?? labels.sv ?? attribute.key ?? id)
  return {
    id: String(attribute.id ?? id),
    key: String(attribute.key ?? id),
    label,
    kind: String(kind.type ?? 'text'),
    unit: unitOf(kind, families),
    multiple: Boolean(attribute.multiple),
    languages: Boolean(attribute.language_dependent),
    required: Boolean(attribute.required),
    inherited,
    from,
  }
}

function unitOf(kind: Json, families: Json[]) {
  if (typeof kind.unit === 'string' && kind.unit) return kind.unit
  const familyId = String(kind.unit_family ?? '')
  if (!familyId) return ''
  const family = families.find((item) => String(item.id) === familyId || String(item.key) === familyId)
  return family ? String(family.key ?? '') : ''
}
