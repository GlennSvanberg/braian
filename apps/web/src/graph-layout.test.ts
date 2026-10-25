import { describe, expect, it } from 'vitest'
import { neighbor, placeNodes } from './graph-layout'

describe('graph layout', () => {
  it('puts sources left of the object and relations to the right', () => {
    const placed = placeNodes([
      { id: 'self', rank: 1 },
      { id: 'rel', rank: 2 },
      { id: 'source', rank: 0 },
    ])
    const x = (id: string) => placed.find((node) => node.id === id)?.x ?? 0
    expect(x('source')).toBeLessThan(x('self'))
    expect(x('self')).toBeLessThan(x('rel'))
  })

  it('moves to the neighbour in the given direction', () => {
    const order = ['source', 'self', 'rel']
    expect(neighbor(order, 'self', 'right')).toBe('rel')
    expect(neighbor(order, 'self', 'left')).toBe('source')
    expect(neighbor(order, 'source', 'up')).toBe('source')
  })
})
