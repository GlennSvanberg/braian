import { describe, expect, it } from 'vitest'
import { cellEditShouldPatch } from './cell-edit'

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
