import {
  EMPTY_CELL,
  formatCellValue,
  formatColumnTitle,
  summarizeList,
  type AppliedQualifier,
  type FieldDef,
} from './qualifiers'
import { MOCK_PRODUCTS, PRODUCT_FIELDS, type ProductRow, fieldById } from './mock-catalog'

/** A visible table column built from a field path + qualifiers. */
export type ProtoColumn = {
  id: string
  fieldPath: string[]
  pathNames: string[]
  qualifiers: AppliedQualifier[]
  leaf: FieldDef
  viaManyLink: boolean
}

export type CellDisplay = { text: string; title: string }

export type ArticlePickOption = {
  id: string
  label: string
  subtitle: string
}

export function columnIdOf(col: Pick<ProtoColumn, 'fieldPath' | 'qualifiers'>): string {
  const q = col.qualifiers
    .map((item) => `${item.kind}:${item.optionId}${item.value ? `=${item.value}` : ''}`)
    .join('|')
  return `${col.fieldPath.join('.')}${q ? `#${q}` : ''}`
}

export function columnPathKey(col: Pick<ProtoColumn, 'fieldPath'>): string {
  return col.fieldPath.join('.')
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

/** Reasonable pixel width so path headers stay readable while aligned with cells. */
export function columnWidthPx(col: ProtoColumn): number {
  const title = columnTitle(col)
  return Math.max(200, Math.min(280, Math.round(title.length * 7.2)))
}

/**
 * Articles for Specific… after a leaf field is chosen.
 * Label is `GTIN · key`; subtitle is product name(s). Only articles that have the field.
 */
export function articleOptionsForField(leafId: string): ArticlePickOption[] {
  const byKey = new Map<string, ArticlePickOption>()
  for (const product of MOCK_PRODUCTS) {
    for (const article of product.articles) {
      const raw = (article as Record<string, unknown>)[leafId]
      if (raw == null) continue
      const existing = byKey.get(article.key)
      if (existing) {
        if (!existing.subtitle.split(', ').includes(product.name)) {
          existing.subtitle = `${existing.subtitle}, ${product.name}`
        }
        continue
      }
      byKey.set(article.key, {
        id: article.key,
        label: `${article.gtin} · ${article.key}`,
        subtitle: product.name,
      })
    }
  }
  return [...byKey.values()].sort((a, b) => a.id.localeCompare(b.id))
}

function withEmpty(text: string, title = ''): CellDisplay {
  if (!text) return { text: EMPTY_CELL, title: '' }
  return { text, title: title || text }
}

/** Resolve display text + tooltip title for one product cell. */
export function readCellDetail(row: ProductRow, col: ProtoColumn): CellDisplay {
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
      const summary = summarizeList(projected.map((item) => item.text).filter(Boolean))
      return withEmpty(summary.text, summary.title)
    }
    if (card.optionId === 'first') return withEmpty(projected[0]?.text ?? '')
    if (card.optionId === 'specific') {
      return withEmpty(projected.find((item) => item.key === card.value)?.text ?? '')
    }
    return withEmpty('')
  }
  if (col.fieldPath[0] === 'supplier' && col.fieldPath.length === 2) {
    const leafKey = col.fieldPath[1]!
    const value = (row.supplier as Record<string, unknown>)[leafKey]
    return withEmpty(formatCellValue({ value }, col.leaf, leafQualifiers))
  }
  if (col.fieldPath.length === 1) {
    const key = col.fieldPath[0]!
    const value = (row as Record<string, unknown>)[key]
    return withEmpty(formatCellValue({ value }, col.leaf, col.qualifiers))
  }
  return withEmpty('')
}

export function readCell(row: ProductRow, col: ProtoColumn): string {
  return readCellDetail(row, col).text
}

export function defaultColumns(): ProtoColumn[] {
  return [makeColumn(['sku'], [])!, makeColumn(['name'], [])!]
}

export function withReplacedQualifier(column: ProtoColumn, next: AppliedQualifier): ProtoColumn {
  const others = column.qualifiers.filter((item) => item.kind !== next.kind)
  const qualifiers = [...others, next]
  qualifiers.sort((a, b) => {
    const order = (k: AppliedQualifier['kind']) =>
      k === 'cardinality' ? 0 : k === 'language' || k === 'unit' || k === 'value_list' || k === 'range' ? 1 : 2
    return order(a.kind) - order(b.kind)
  })
  return (
    makeColumn(column.fieldPath, qualifiers) ?? {
      ...column,
      qualifiers,
      id: columnIdOf({ fieldPath: column.fieldPath, qualifiers }),
    }
  )
}
