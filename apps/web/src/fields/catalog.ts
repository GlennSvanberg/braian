/**
 * Build a FieldDef tree for the Fields › palette from /v1/definitions
 * and /v1/types/{id}/attribute-paths.
 *
 * Own attributes become leaves (with qualifier steps from kind).
 * Link labels become link-shaped fields that drill into the peer type.
 * Parts such as `.unit` / `.label` are not listed as separate fields —
 * they are chosen as qualifiers after the leaf is selected.
 */
import type { Json } from '../api'
import type { FieldDef, FieldValueShape } from './qualifiers'

export type DefsJson = Json

function labelOf(labels: unknown, lang = 'en'): string {
  if (!labels || typeof labels !== 'object') return ''
  const map = labels as Record<string, unknown>
  const hit = map[lang] ?? map.en ?? Object.values(map)[0]
  return hit == null ? '' : String(hit)
}

function typeByKey(defs: DefsJson, key: string): Json | undefined {
  const types = (defs.types as Json[] | undefined) ?? []
  return types.find((item) => String(item.key) === key || String(item.id) === key)
}

function attrById(defs: DefsJson, id: string): Json | undefined {
  const attrs = (defs.attributes as Json[] | undefined) ?? []
  return attrs.find((item) => String(item.id) === id || String(item.key) === id)
}

function attrByKey(defs: DefsJson, key: string): Json | undefined {
  const attrs = (defs.attributes as Json[] | undefined) ?? []
  return attrs.find((item) => String(item.key) === key)
}

function kindType(kind: unknown): string {
  if (!kind || typeof kind !== 'object') return 'text'
  return String((kind as Json).type ?? 'text')
}

function unitsForFamily(defs: DefsJson, familyToken: string | undefined): string[] {
  if (!familyToken) return []
  const families = (defs.unit_families as Json[] | undefined) ?? []
  const family = families.find(
    (item) => String(item.key) === familyToken || String(item.id) === familyToken,
  )
  if (!family) return []
  return ((family.units as Json[] | undefined) ?? []).map((unit) => String(unit.code))
}

function factorsForFamily(defs: DefsJson, familyToken: string | undefined): Record<string, number> {
  if (!familyToken) return {}
  const families = (defs.unit_families as Json[] | undefined) ?? []
  const family = families.find(
    (item) => String(item.key) === familyToken || String(item.id) === familyToken,
  )
  if (!family) return {}
  const out: Record<string, number> = {}
  for (const unit of (family.units as Json[] | undefined) ?? []) {
    const code = String(unit.code).toLowerCase()
    const factor = Number(unit.factor)
    if (code && Number.isFinite(factor)) out[code] = factor
  }
  return out
}

export function unitFactors(defs: DefsJson): Record<string, Record<string, number>> {
  const out: Record<string, Record<string, number>> = {}
  for (const family of (defs.unit_families as Json[] | undefined) ?? []) {
    const key = String(family.key)
    out[key] = factorsForFamily(defs, key)
    out[String(family.id)] = out[key]
  }
  return out
}

function shapeOf(attr: Json, defs: DefsJson): { shape: FieldValueShape; extra: Partial<FieldDef> } {
  const kind = attr.kind as Json | undefined
  const type = kindType(kind)
  const languages = ((defs.languages as string[] | undefined) ?? ['en']).map(String)
  if (attr.language_dependent) {
    return { shape: 'language', extra: { languages } }
  }
  if (type === 'number' && kind?.unit_family) {
    const family = String(kind.unit_family)
    // unit_family may be id — resolve to codes
    const families = (defs.unit_families as Json[] | undefined) ?? []
    const hit = families.find((item) => String(item.id) === family || String(item.key) === family)
    const units = unitsForFamily(defs, hit ? String(hit.key) : family)
    return { shape: 'unit', extra: { units, typeLabel: 'number' } }
  }
  if (type === 'value_list') {
    return { shape: 'value_list', extra: { valueListModes: ['label', 'code'] } }
  }
  if (type === 'range') {
    const family = kind?.unit_family != null ? String(kind.unit_family) : undefined
    const families = (defs.unit_families as Json[] | undefined) ?? []
    const hit = family
      ? families.find((item) => String(item.id) === family || String(item.key) === family)
      : undefined
    const rangeUnit = hit ? String(hit.base_unit ?? '') : ''
    return {
      shape: 'range',
      extra: { rangeParts: ['min', 'max'], rangeUnit: rangeUnit || undefined },
    }
  }
  if (type === 'boolean') return { shape: 'boolean', extra: {} }
  if (type === 'number') return { shape: 'number', extra: {} }
  return { shape: 'text', extra: {} }
}

function attributeField(defs: DefsJson, attr: Json): FieldDef {
  const { shape, extra } = shapeOf(attr, defs)
  return {
    id: String(attr.key),
    name: labelOf(attr.labels) || String(attr.key),
    typeLabel: kindType(attr.kind),
    shape,
    ...extra,
  }
}

function attributesForType(defs: DefsJson, typeObj: Json): Json[] {
  const ids = new Set<string>()
  for (const id of (typeObj.attributes as unknown[] | undefined) ?? []) {
    ids.add(String(id))
  }
  // Structure attributes
  const structures = (defs.structures as Json[] | undefined) ?? []
  const allowed = typeObj.structures as unknown[] | undefined
  for (const structure of structures) {
    if (allowed && !allowed.map(String).includes(String(structure.id))) continue
    for (const group of (structure.groups as Json[] | undefined) ?? []) {
      for (const id of (group.attributes as unknown[] | undefined) ?? []) ids.add(String(id))
      for (const cls of (group.classes as Json[] | undefined) ?? []) {
        // classes may be top-level under structure
      }
    }
    for (const cls of (structure.classes as Json[] | undefined) ?? []) {
      for (const id of (cls.attributes as unknown[] | undefined) ?? []) ids.add(String(id))
    }
  }
  const out: Json[] = []
  for (const id of ids) {
    const attr = attrById(defs, id)
    if (attr) out.push(attr)
  }
  return out
}

/**
 * Field catalog for one object type key.
 * Link fields use the path label key (e.g. `articles`) as id — same as path text before `>`.
 */
export function fieldCatalogForType(defs: DefsJson, typeKey: string): FieldDef[] {
  const typeObj = typeByKey(defs, typeKey)
  if (!typeObj) return []
  const fields: FieldDef[] = []
  const seen = new Set<string>()

  for (const attr of attributesForType(defs, typeObj)) {
    const key = String(attr.key)
    if (seen.has(key)) continue
    // Skip system bookkeeping attributes that are not useful as picker leaves
    if (['created_at', 'updated_at', 'updated_by'].includes(key)) continue
    seen.add(key)
    fields.push(attributeField(defs, attr))
  }

  const types = (defs.types as Json[] | undefined) ?? []
  for (const linkType of types) {
    const spec = linkType.link as Json | undefined
    if (!spec) continue
    const fromTypes = ((spec.from_types as unknown[]) ?? []).map(String)
    const toTypes = ((spec.to_types as unknown[]) ?? []).map(String)
    const typeId = String(typeObj.id)

    // Source side: show to_label (e.g. Articles), walk to peer types
    if (fromTypes.includes(typeId) || fromTypes.includes(typeKey)) {
      const labelKey = String((spec.to_label as Json)?.key ?? linkType.key)
      const labelName = labelOf((spec.to_label as Json)?.labels) || labelKey
      const many = String(spec.to_cardinality) === 'many' || String(spec.from_cardinality) === 'many'
      // from_cardinality caps links on the from object; many => product has many articles
      const fromMany = String(spec.from_cardinality) === 'many'
      const peerFields: FieldDef[] = []
      const peerSeen = new Set<string>()
      for (const peerId of toTypes) {
        const peer = typeByKey(defs, peerId)
        if (!peer) continue
        for (const attr of attributesForType(defs, peer)) {
          const key = String(attr.key)
          if (peerSeen.has(key) || ['created_at', 'updated_at', 'updated_by'].includes(key)) continue
          peerSeen.add(key)
          peerFields.push(attributeField(defs, attr))
        }
      }
      if (!seen.has(labelKey)) {
        seen.add(labelKey)
        fields.push({
          id: labelKey,
          name: labelName,
          typeLabel: 'link',
          shape: 'link',
          link: { many: fromMany, peerFields, peerLabel: labelName },
        })
      }
    }

    // Target side: show from_label (e.g. Product / Supplier)
    if (toTypes.includes(typeId) || toTypes.includes(typeKey)) {
      const labelKey = String((spec.from_label as Json)?.key ?? linkType.key)
      const labelName = labelOf((spec.from_label as Json)?.labels) || labelKey
      const toMany = String(spec.to_cardinality) === 'many'
      const peerFields: FieldDef[] = []
      const peerSeen = new Set<string>()
      for (const peerId of fromTypes) {
        const peer = typeByKey(defs, peerId)
        if (!peer) continue
        for (const attr of attributesForType(defs, peer)) {
          const key = String(attr.key)
          if (peerSeen.has(key) || ['created_at', 'updated_at', 'updated_by'].includes(key)) continue
          peerSeen.add(key)
          peerFields.push(attributeField(defs, attr))
        }
      }
      if (!seen.has(labelKey)) {
        seen.add(labelKey)
        fields.push({
          id: labelKey,
          name: labelName,
          typeLabel: 'link',
          shape: 'link',
          link: { many: toMany, peerFields, peerLabel: labelName },
        })
      }
    }
  }

  return fields
}

/** Resolve attribute key for a leaf FieldDef id (same as id for non-links). */
export function leafAttributeKey(fieldId: string): string {
  return fieldId
}

export { attrByKey, typeByKey, factorsForFamily, labelOf }
