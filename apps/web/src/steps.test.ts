import { describe, expect, it } from 'vitest'
import { stepText } from './steps'

describe('stepText', () => {
  it('uses the palette sentences from the design spec', () => {
    expect(stepText('list_objects')).toBe('Opening a list of objects…')
    expect(stepText('list_objects', {}, { plural: 'Products' })).toBe('Opening a list of products…')
    expect(stepText('get_object', {}, { singular: 'Product' })).toBe('Opening the Product…')
    expect(stepText('ui.filter', { q: 'cheddar' })).toBe("Filtering on 'cheddar'…")
    expect(stepText('ui.get_selection')).toBe('Reading the selection…')
    expect(stepText('ui.get_context')).toBe('Reading the screen…')
    expect(stepText('search', { q: 'cheddar' })).toBe("Filtering on 'cheddar'…")
  })
})
