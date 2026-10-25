import { describe, expect, it } from 'vitest'
import { humanizeLive, objectIdFromLive } from './activity'

describe('activity', () => {
  it('turns a live event into a sentence and keeps the word live', () => {
    const line = humanizeLive(JSON.stringify({ op: 'create', type: 'artikel', key: 'ART-1' }))
    expect(line).toContain('live')
    expect(line).toContain('ART-1')
    expect(line).not.toContain('{')
    expect(objectIdFromLive(JSON.stringify({ object_id: 'abc' }))).toBe('abc')
    expect(humanizeLive(JSON.stringify({ op: 'update', type: 'produkt', key: 'PROD-1' }), 'Product')).toBe(
      'live · Updated Product PROD-1',
    )
  })

  it('summarises a bulk fold and a lagged stream', () => {
    expect(humanizeLive(JSON.stringify({ op: 'bulk_summary', count: 40 }))).toBe(
      'live · Bulk update (40)',
    )
    expect(humanizeLive(JSON.stringify({ op: 'lagged', skipped: 3 }))).toBe(
      'live · Skipped 3 events',
    )
    expect(humanizeLive(JSON.stringify({ op: 'job', status: 'running', done: 2, total: 8 }))).toBe(
      'live · Job running 2/8',
    )
    expect(humanizeLive(JSON.stringify({ op: 'import', id: 'session-1', status: 'diffing' }))).toBe('')
    expect(objectIdFromLive(JSON.stringify({ op: 'import', id: 'session-1' }))).toBeNull()
  })
})
