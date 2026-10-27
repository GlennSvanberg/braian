import { describe, expect, it } from 'vitest'
import { formatCount, formatMeasure, formatTimestamp } from './format'

describe('format', () => {
  it('prints a local timestamp as YYYY-MM-DD HH:mm', () => {
    expect(formatTimestamp(new Date(2026, 9, 27, 14, 5))).toBe('2026-10-27 14:05')
    expect(formatTimestamp(new Date(2026, 0, 2, 3, 4))).toBe('2026-01-02 03:04')
  })

  it('prints a measure with an English decimal and the entered unit', () => {
    expect(formatMeasure('500', 'g')).toBe('500 g')
    expect(formatMeasure('1500.5', 'kg')).toBe('1,500.5 kg')
    expect(formatMeasure('0.6')).toBe('0.6')
  })

  it('groups counts in English', () => {
    expect(formatCount(1240)).toBe('1,240')
    expect(formatCount(0)).toBe('0')
    expect(formatCount('12')).toBe('12')
  })
})
