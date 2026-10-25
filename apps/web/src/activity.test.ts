import { describe, expect, it } from 'vitest'
import { humanizeLive, objectIdFromLive } from './activity'

describe('activity', () => {
  it('turns a live event into a sentence and keeps the word live', () => {
    const line = humanizeLive(JSON.stringify({ op: 'create', type: 'artikel', key: 'ART-1' }))
    expect(line).toContain('live')
    expect(line).toContain('ART-1')
    expect(line).not.toContain('{')
    expect(objectIdFromLive(JSON.stringify({ object_id: 'abc' }))).toBe('abc')
  })
})
