import { describe, expect, it } from 'vitest'
import { stepText } from './steps'

describe('stepText', () => {
  it('uses the palette sentences from the design spec', () => {
    expect(stepText('list_objects')).toBe('Opening a list of articles…')
    expect(stepText('ui.filter', { q: 'tält' })).toBe("Filtering on 'tält'…")
    expect(stepText('search', { q: 'tält' })).toBe("Filtering on 'tält'…")
  })
})
