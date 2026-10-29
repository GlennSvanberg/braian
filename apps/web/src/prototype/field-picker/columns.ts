import {
  formatCellValue,
  formatColumnTitle,
  summarizeList,
  type AppliedQualifier,
  type FieldDef,
} from './qualifiers'
import { PRODUCT_FIELDS, type ProductRow, fieldById } from './mock-catalog'

/** A visible table column built from a field path + qualifiers. */
export type ProtoColumn = {
  id: string
  /** Field ids from root to leaf (e.g. articles, net_volume). */
  fieldPath: string[]
  /** Display names matching fieldPath. */
  pathNames: string[]
  qualifiers: AppliedQualifier[]
  leaf: FieldDef
  /** True when the path crossed a many-link (cardinality required). */
  viaManyLink: boolean
}

export function columnIdOf(col: Pick<ProtoColumn, 'fieldPath' | 'qualifiers'>): string {
  const q = col.qualifiers
    .map((item) => `${item.kind}:${item.optionId}${item.value ? `=${item.value}` : ''}`)
    .join('|')
  return `${col.fieldPath.join('.')}${q ? `#${q}` : ''}`
}

export function makeColumn(
  fieldPath: string[],
  qualifiers: AppliedQualifier[],
  fields: FieldDef[] = PRODUCT_FIELDS,
): ProtoColumn | null {
  let current = fields
  let viaManyLink = false
  const pathNames: string[] = []
  let leaf: FieldDef | undefined
  for (let i = 0; i < fieldPath.length; i += 1) {
    const id = fieldPath[i]!
    const field = fieldById(current, id)
    if (!field) return null
    pathNames.push(field.name)
    leaf = field
    if (field.shape === 'link' && field.link) {
      if (field.link.many) viaManyLink = true
      current = field.link.peerFields
    } else if (i < fieldPath.length - 1) {
      return null
    }
  }
  if (!leaf || leaf.shape === 'link') return null
  const draft: ProtoColumn = {
    fieldPath,
    pathNames,
    qualifiers,
    leaf,
    viaManyLink,
    id: '',
  }
  draft.id = columnIdOf(draft)
  return draft
}

export function columnTitle(col: ProtoColumn): string {
  return formatColumnTitle(col.pathNames, col.qualifiers)
}

/** Resolve the display string for one product cell. */
export function readCell(row: ProductRow, col: ProtoColumn): string {
  const leafQualifiers = col.qualifiers.filter((item) => item.kind !== 'cardinality')
  if (col.fieldPath[0] === 'articles' && col.fieldPath.length === 2) {
    const leafKey = col.fieldPath[1]!
    const card = col.qualifiers.find((item) => item.kind === 'cardinality')
    const projected = row.articles.map((article) => {
      const raw = (article as Record<string, unknown>)[leafKey]
      return {
        key: article.key,
        text: formatCellValue({ value: raw }, col.leaf, leafQualifiers),
      }
    })
    if (!card || card.optionId === 'all') {
      return summarizeList(projected.map((item) => item.text)).text
    }
    if (card.optionId === 'first') return projected[0]?.text ?? ''
    if (card.optionId === 'specific') {
      return projected.find((item) => item.key === card.value)?.text ?? ''
    }
    return ''
  }
  if (col.fieldPath[0] === 'supplier' && col.fieldPath.length === 2) {
    const leafKey = col.fieldPath[1]!
    const value = (row.supplier as Record<string, unknown>)[leafKey]
    return formatCellValue({ value }, col.leaf, leafQualifiers)
  }
  if (col.fieldPath.length === 1) {
    const key = col.fieldPath[0]!
    const value = (row as Record<string, unknown>)[key]
    return formatCellValue({ value }, col.leaf, col.qualifiers)
  }
  return ''
}

/** Default columns shown when the prototype opens. */
export function defaultColumns(): ProtoColumn[] {
  return [makeColumn(['sku'], [])!, makeColumn(['name'], [])!]
}
