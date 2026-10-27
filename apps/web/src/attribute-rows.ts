import type { Json } from './api'
import { isStandardAttribute } from './attribute-actions'

export type UsedPlace = {
  id: string
  key: string
  label: string
  objects: number
  structureKey?: string
  structureLabel?: string
}

export type AttributeFlags = {
  required: boolean
  multiple: boolean
  languages: boolean
  searchable: boolean
  filterable: boolean
  sortable: boolean
  standard: boolean
}

export type AttributeDefinitionRow = {
  id: string
  key: string
  label: string
  kind: string
  unitFamily: string
  valueList: string
  flags: AttributeFlags
  standard: boolean
  objects: number
  types: UsedPlace[]
  groups: UsedPlace[]
}

type Usage = { objects: number; types: UsedPlace[]; groups: UsedPlace[] }

export function rowsFromCatalog(body: Json | undefined): AttributeDefinitionRow[] {
  return ((body?.attributes as Json[] | undefined) ?? []).map(rowFromCatalog)
}

export function rowsFromDefinitions(defs: Json, catalog: Json | undefined): AttributeDefinitionRow[] {
  const usage = usageById(catalog)
  const attributes = (defs.attributes as Json[] | undefined) ?? []
  return attributes.map((attribute) => {
    const id = String(attribute.id ?? '')
    const key = String(attribute.key ?? '')
    const kind = kindOf(attribute.kind)
    const standard = isStandardAttribute(id, key)
    const known = usage.get(id)
    const places = membership(defs, id, known)
    return {
      id,
      key,
      label: labelOf(attribute.labels, key),
      kind: kind.name,
      unitFamily: familyLabel(defs, kind.unitFamily),
      valueList: listLabel(defs, kind.list),
      flags: flagsOf(attribute, standard),
      standard,
      objects: known?.objects ?? 0,
      types: places.types,
      groups: places.groups,
    }
  })
}

function rowFromCatalog(row: Json): AttributeDefinitionRow {
  const id = String(row.id ?? '')
  const key = String(row.key ?? '')
  const flags = (row.flags as Json | undefined) ?? {}
  const standard = row.standard === true || isStandardAttribute(id, key)
  const used = (row.used_in as Json | undefined) ?? {}
  return {
    id,
    key,
    label: String(row.label ?? key),
    kind: String(row.kind ?? ''),
    unitFamily: placeLabel(row.unit_family),
    valueList: placeLabel(row.value_list),
    flags: {
      required: flags.required === true,
      multiple: flags.multiple === true,
      languages: flags.languages === true,
      searchable: flags.searchable === true,
      filterable: flags.filterable === true,
      sortable: flags.sortable === true,
      standard,
    },
    standard,
    objects: Number(used.objects ?? 0),
    types: places(used.types),
    groups: places(used.groups, true),
  }
}

function membership(defs: Json, id: string, known: Usage | undefined): { types: UsedPlace[]; groups: UsedPlace[] } {
  const types: UsedPlace[] = []
  const seenTypes = new Set<string>()
  for (const objectType of (defs.types as Json[] | undefined) ?? []) {
    const attached = ((objectType.attributes as unknown[] | undefined) ?? []).map(String)
    if (!attached.includes(id)) continue
    const typeId = String(objectType.id ?? objectType.key ?? '')
    seenTypes.add(typeId)
    const hit = known?.types.find((place) => place.id === typeId)
    types.push({
      id: typeId,
      key: String(objectType.key ?? ''),
      label: singularOf(objectType),
      objects: hit?.objects ?? 0,
    })
  }
  for (const place of known?.types ?? []) {
    if (seenTypes.has(place.id) || place.objects <= 0) continue
    types.push(place)
  }
  const groups: UsedPlace[] = []
  const seenGroups = new Set<string>()
  for (const structure of (defs.structures as Json[] | undefined) ?? []) {
    const structureId = String(structure.id ?? '')
    const structureKey = String(structure.key ?? '')
    const structureLabel = labelOf(structure.labels, structureKey)
    const classes = (structure.classes as Json[] | undefined) ?? []
    for (const group of (structure.groups as Json[] | undefined) ?? []) {
      const groupId = String(group.id ?? '')
      const own = ((group.attributes as unknown[] | undefined) ?? []).map(String).includes(id)
      const onClass = classes.some((item) => {
        const same = String(item.group ?? '') === groupId
        const listed = ((item.attributes as unknown[] | undefined) ?? []).map(String).includes(id)
        return same && listed
      })
      if (!own && !onClass) continue
      seenGroups.add(`${structureId}:${groupId}`)
      const hit = known?.groups.find((place) => place.id === groupId && place.structureKey === structureKey)
      groups.push({
        id: groupId,
        key: String(group.key ?? ''),
        label: labelOf(group.labels, String(group.key ?? '')),
        objects: hit?.objects ?? 0,
        structureKey,
        structureLabel,
      })
    }
  }
  for (const place of known?.groups ?? []) {
    const token = `${place.structureKey ?? ''}:${place.id}`
    if (seenGroups.has(token) || place.objects <= 0) continue
    groups.push(place)
  }
  return { types, groups }
}

function usageById(catalog: Json | undefined) {
  const map = new Map<string, Usage>()
  for (const row of rowsFromCatalog(catalog)) {
    map.set(row.id, { objects: row.objects, types: row.types, groups: row.groups })
  }
  return map
}

function kindOf(kind: unknown): { name: string; unitFamily: string; list: string } {
  if (!kind || typeof kind !== 'object') return { name: String(kind ?? ''), unitFamily: '', list: '' }
  const record = kind as Json
  return {
    name: String(record.type ?? ''),
    unitFamily: String(record.unit_family ?? ''),
    list: String(record.list ?? record.value_list ?? ''),
  }
}

function flagsOf(attribute: Json, standard: boolean): AttributeFlags {
  const index = (attribute.index as Json | undefined) ?? {}
  return {
    required: attribute.required === true,
    multiple: attribute.multiple === true,
    languages: attribute.language_dependent === true,
    searchable: index.searchable === true,
    filterable: index.filterable === true,
    sortable: index.sortable === true,
    standard,
  }
}

function familyLabel(defs: Json, id: string) {
  if (!id) return ''
  const family = ((defs.unit_families as Json[] | undefined) ?? []).find((item) => String(item.id) === id || String(item.key) === id)
  return family ? String(family.key ?? id) : id
}

function listLabel(defs: Json, key: string) {
  if (!key) return ''
  const list = ((defs.value_lists as Json[] | undefined) ?? []).find((item) => String(item.key) === key)
  return list ? labelOf(list.labels, key) : key
}

function places(value: unknown, grouped = false): UsedPlace[] {
  if (!Array.isArray(value)) return []
  return value.map((item) => {
    const row = item as Json
    return {
      id: String(row.id ?? ''),
      key: String(row.key ?? ''),
      label: String(row.label ?? row.key ?? ''),
      objects: Number(row.objects ?? 0),
      ...(grouped
        ? {
            structureKey: String(row.structure_key ?? ''),
            structureLabel: String(row.structure_label ?? ''),
          }
        : {}),
    }
  })
}

function placeLabel(value: unknown) {
  if (!value || typeof value !== 'object') return ''
  const row = value as Json
  return String(row.label ?? row.key ?? '')
}

function singularOf(objectType: Json) {
  const singular = labelOf(objectType.singular, '')
  if (singular) return singular
  return labelOf(objectType.labels, String(objectType.key ?? ''))
}

function labelOf(value: unknown, fallback: string) {
  if (!value || typeof value !== 'object') return fallback
  const labels = value as Record<string, unknown>
  const english = labels.en
  if (typeof english === 'string' && english) return english
  const first = Object.values(labels).find((item) => typeof item === 'string' && item)
  return typeof first === 'string' ? first : fallback
}
