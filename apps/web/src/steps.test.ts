import { describe, expect, it } from 'vitest'
import { stepText } from './steps'

describe('stepText', () => {
  it('uses the palette sentences from the design spec', () => {
    expect(stepText('list_objects')).toBe('Öppnar en lista med artiklar…')
    expect(stepText('ui.filter', { q: 'tält' })).toBe("Filtrerar på 'tält'…")
    expect(stepText('search', { q: 'tält' })).toBe("Filtrerar på 'tält'…")
  })
})
