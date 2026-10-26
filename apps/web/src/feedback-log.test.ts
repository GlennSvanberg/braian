import { describe, expect, it } from 'vitest'
import { formatLogPreview, sanitizeRoute, sanitizeText, snapshotClientLog } from './feedback-log'

describe('feedback log', () => {
  it('redacts bearer tokens and sensitive query params', () => {
    expect(sanitizeText('Bearer abc.def.ghi')).toBe('Bearer [redacted]')
    expect(sanitizeRoute('/v1/objects?token=secret&page=2')).toBe('/v1/objects?token=%5Bredacted%5D&page=2')
  })

  it('snapshots the ring buffer as a copy', () => {
    const first = snapshotClientLog()
    expect(Array.isArray(first)).toBe(true)
    expect(formatLogPreview(first)).toContain('Recent log')
  })
})
