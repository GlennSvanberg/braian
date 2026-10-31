import { parseColumnId } from './fields/columns'

/** True when a cell editor draft should be PATCH'ed. Esc / unchanged must return false. */
export function cellEditShouldPatch(original: string, nextText: string): boolean {
  return nextText !== original
}

/**
 * Build PATCH `values` for an inline cell edit.
 *
 * Language-dependent attributes (e.g. `name`) must be sent as
 * `{ [attrKey]: { [lang]: text } }` — the same shape the detail Editor uses —
 * even when the grid column id has no `@lang` suffix. Bare strings are rejected
 * by the API and leave the old value on screen after refresh.
 */
export function cellEditValuesBody(args: {
  attrKey: string
  nextText: string
  /** Language from a column id like `name@en` or `description@sv`. */
  columnLanguage?: string
  languageDependent: boolean
  uiLanguage: string
}): { [key: string]: unknown } {
  if (args.columnLanguage) {
    return { [args.attrKey]: { [args.columnLanguage]: args.nextText } }
  }
  if (args.languageDependent) {
    return { [args.attrKey]: { [args.uiLanguage]: args.nextText } }
  }
  return { [args.attrKey]: args.nextText }
}

/** Path / qualifier-free column key (`name@en` → `name`). */
export function columnBaseKey(columnId: string): string {
  const path = parseColumnId(columnId).path
  return (path.split('@')[0] ?? path).split('.')[0] ?? path
}

/** System / identity columns that must never open an inline editor. */
const READ_ONLY_COLUMN_BASES = new Set([
  'type',
  'version',
  'key',
  'number',
  'created',
  'created_at',
  'updated',
  'updated_at',
  'updated_by',
])

/**
 * Whether ObjectTable may start an inline edit for this column id.
 * Hops and system fields are blocked; value_list columns (e.g. Status) stay editable
 * so the Select editor can open.
 */
export function isEditableColumn(columnId: string): boolean {
  const path = parseColumnId(columnId).path
  if (path.includes('>')) return false
  const base = columnBaseKey(columnId)
  if (READ_ONLY_COLUMN_BASES.has(base)) return false
  return true
}

type AttrKind = { type?: unknown; list?: unknown }

/** Value-list / lifecycle list key for a column, or null when the cell is free text. */
export function valueListKeyForColumn(
  columnId: string,
  defs: { attributes?: Array<{ key?: unknown; kind?: unknown }> } | null | undefined,
): string | null {
  const base = columnBaseKey(columnId)
  const attributes = defs?.attributes ?? []
  const attr = attributes.find((item) => String(item.key ?? '') === base)
  const kind = (attr?.kind && typeof attr.kind === 'object' ? attr.kind : {}) as AttrKind
  const type = String(kind.type ?? '')
  if (type === 'value_list' || type === 'lifecycle') {
    return String(kind.list ?? base)
  }
  return null
}

/** True when the event target (or document focus) is inside an inline `.cell-editor` or open Select. */
export function isTextEditingTarget(event: { target: EventTarget | null }): boolean {
  const hit = (node: Element | null) =>
    Boolean(
      node
      && (node.closest('.cell-editor')
        || node.closest('.select-content')
        || node.closest('[data-radix-select-content]')),
    )
  if (event.target instanceof Element && hit(event.target)) return true
  if (typeof document !== 'undefined') {
    const active = document.activeElement
    if (active instanceof Element && hit(active)) return true
  }
  return false
}

/**
 * True while ObjectTable has an open cell editor (`editingCell`) or focus is in `.cell-editor`.
 * Call first in grid / global keydown handlers so Home/End/Ctrl+K/etc. never steal keys from the caret.
 */
export function isEditingCell(
  event: { target: EventTarget | null },
  editingCell?: { rowId: string; columnId: string } | null,
): boolean {
  return Boolean(editingCell) || isTextEditingTarget(event)
}
