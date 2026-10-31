// @vitest-environment jsdom
import { describe, expect, it } from 'vitest'
import { cellEditShouldPatch, cellEditValuesBody, isEditingCell, isTextEditingTarget } from './cell-edit'

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
