// @vitest-environment jsdom
import { describe, expect, it } from 'vitest'
import {
  cellEditShouldPatch,
  cellEditValuesBody,
  columnBaseKey,
  isEditableColumn,
  isEditingCell,
  isTextEditingTarget,
  resolveCellEditInitial,
  valueListKeyForColumn,
} from './cell-edit'

describe('cellEditShouldPatch', () => {
  it('returns false for identical strings (Enter/blur must not PATCH)', () => {
    expect(cellEditShouldPatch('hello', 'hello')).toBe(false)
  })

  it('returns false when Esc restores the original after a draft clear', () => {
    const original = 'seed text'
    const draftCleared = ''
    expect(cellEditShouldPatch(original, draftCleared)).toBe(true)
    // cancel path commits original again → no PATCH
    expect(cellEditShouldPatch(original, original)).toBe(false)
  })

  it('returns true only when the value actually changed', () => {
    expect(cellEditShouldPatch('a', 'b')).toBe(true)
    expect(cellEditShouldPatch('', 'x')).toBe(true)
  })
})

describe('cellEditValuesBody', () => {
  it('wraps language_dependent attrs without @lang using uiLanguage (name fix)', () => {
    expect(
      cellEditValuesBody({
        attrKey: 'name',
        nextText: 'Aged cheddar',
        languageDependent: true,
        uiLanguage: 'en',
      }),
    ).toEqual({ name: { en: 'Aged cheddar' } })
  })

  it('keeps an explicit @lang from the column id', () => {
    expect(
      cellEditValuesBody({
        attrKey: 'description',
        nextText: 'Hej',
        columnLanguage: 'sv',
        languageDependent: true,
        uiLanguage: 'en',
      }),
    ).toEqual({ description: { sv: 'Hej' } })
  })

  it('sends a bare string for non-localized attributes', () => {
    expect(
      cellEditValuesBody({
        attrKey: 'brand',
        nextText: 'Acme',
        languageDependent: false,
        uiLanguage: 'en',
      }),
    ).toEqual({ brand: 'Acme' })
  })

  it('is red against the old bare-string shape for name', () => {
    // Old commitCellEdit used { name: text } when column id had no @lang.
    const oldWrong = { name: 'Aged cheddar' }
    const fixed = cellEditValuesBody({
      attrKey: 'name',
      nextText: 'Aged cheddar',
      languageDependent: true,
      uiLanguage: 'en',
    })
    expect(oldWrong).not.toEqual(fixed)
    expect(fixed).toEqual({ name: { en: 'Aged cheddar' } })
  })
})

describe('isTextEditingTarget / isEditingCell', () => {
  it('is true when the event target is inside .cell-editor', () => {
    const wrap = document.createElement('div')
    wrap.className = 'cell-editor'
    const input = document.createElement('input')
    wrap.appendChild(input)
    document.body.appendChild(wrap)
    expect(isTextEditingTarget({ target: input })).toBe(true)
    expect(isEditingCell({ target: input })).toBe(true)
    wrap.remove()
  })

  it('is true when document.activeElement is inside .cell-editor even if target is not', () => {
    const wrap = document.createElement('div')
    wrap.className = 'cell-editor'
    const input = document.createElement('input')
    wrap.appendChild(input)
    document.body.appendChild(wrap)
    input.focus()
    expect(isTextEditingTarget({ target: document.body })).toBe(true)
    wrap.remove()
  })

  it('is false for ordinary grid targets without editingCell', () => {
    const cell = document.createElement('div')
    cell.className = 'active-cell'
    document.body.appendChild(cell)
    expect(isTextEditingTarget({ target: cell })).toBe(false)
    expect(isEditingCell({ target: cell }, null)).toBe(false)
    cell.remove()
  })

  it('is true when editingCell is set even if focus is elsewhere', () => {
    const cell = document.createElement('div')
    document.body.appendChild(cell)
    expect(isEditingCell({ target: cell }, { rowId: 'r1', columnId: 'name' })).toBe(true)
    cell.remove()
  })
})


describe('isEditableColumn', () => {
  it('blocks hops and system columns', () => {
    expect(isEditableColumn('supplier>name')).toBe(false)
    expect(isEditableColumn('type')).toBe(false)
    expect(isEditableColumn('version')).toBe(false)
    expect(isEditableColumn('key')).toBe(false)
    expect(isEditableColumn('number')).toBe(false)
    expect(isEditableColumn('created_at')).toBe(false)
    expect(isEditableColumn('updated_at')).toBe(false)
    expect(isEditableColumn('updated_by')).toBe(false)
  })

  it('allows name and status (value_list) columns', () => {
    expect(isEditableColumn('name')).toBe(true)
    expect(isEditableColumn('name@en')).toBe(true)
    expect(isEditableColumn('status')).toBe(true)
  })
})

describe('valueListKeyForColumn', () => {
  const defs = {
    attributes: [
      { key: 'status', kind: { type: 'value_list', list: 'status' } },
      { key: 'name', kind: { type: 'text' } },
      { key: 'lifecycle', kind: { type: 'lifecycle', list: 'product_life' } },
    ],
  }

  it('returns the list key for value_list / lifecycle attrs', () => {
    expect(valueListKeyForColumn('status', defs)).toBe('status')
    expect(valueListKeyForColumn('lifecycle', defs)).toBe('product_life')
  })

  it('returns null for free-text columns', () => {
    expect(valueListKeyForColumn('name', defs)).toBeNull()
    expect(columnBaseKey('name@en')).toBe('name')
  })
})


describe('resolveCellEditInitial', () => {
  it('uses fallback text for indexed columns without waiting on path-values', () => {
    expect(
      resolveCellEditInitial({
        needsPathValues: false,
        pathValuesPending: true,
        pathValuesHasData: false,
        pathValuesForCell: undefined,
        fallbackText: 'Mature cheddar',
      }),
    ).toEqual({ ok: true, text: 'Mature cheddar' })
  })

  it('blocks path-values columns while the batch is pending', () => {
    expect(
      resolveCellEditInitial({
        needsPathValues: true,
        pathValuesPending: true,
        pathValuesHasData: false,
        pathValuesForCell: undefined,
        fallbackText: '',
      }),
    ).toEqual({ ok: false, reason: 'unloaded' })
  })

  it('blocks when data arrived but this cell key is still missing', () => {
    expect(
      resolveCellEditInitial({
        needsPathValues: true,
        pathValuesPending: false,
        pathValuesHasData: true,
        pathValuesForCell: undefined,
        fallbackText: '',
      }),
    ).toEqual({ ok: false, reason: 'unloaded' })
  })

  it('seeds from loaded path-values (including a legitimately empty cell)', () => {
    expect(
      resolveCellEditInitial({
        needsPathValues: true,
        pathValuesPending: false,
        pathValuesHasData: true,
        pathValuesForCell: ["firm cow's-milk cheddar"],
        fallbackText: '',
      }),
    ).toEqual({ ok: true, text: "firm cow's-milk cheddar" })

    expect(
      resolveCellEditInitial({
        needsPathValues: true,
        pathValuesPending: false,
        pathValuesHasData: true,
        pathValuesForCell: [],
        fallbackText: 'painted leftover',
      }),
    ).toEqual({ ok: true, text: '' })
  })

  it('uses formatPathValues when provided (summary shape)', () => {
    expect(
      resolveCellEditInitial({
        needsPathValues: true,
        pathValuesPending: false,
        pathValuesHasData: true,
        pathValuesForCell: ['A', 'B', 'C'],
        fallbackText: '',
        formatPathValues: (values) => values.slice(0, 2).join(', ') + ' +1',
      }),
    ).toEqual({ ok: true, text: 'A, B +1' })
  })
})
