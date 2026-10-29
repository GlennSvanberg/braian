import { describe, expect, it } from 'vitest'
import { cellPathValues } from './path-values'

describe('cellPathValues', () => {
  it('prefers the full column id so all/first do not collide', () => {
    const all = 'articles>net_weight#cardinality:all|unit:kg'
    const first = 'articles>net_weight#cardinality:first|unit:kg'
    const map = {
      [all]: ['0.4 kg', '0.45 kg'],
      [first]: ['0.4 kg'],
    }
    expect(cellPathValues(map, all)).toEqual(['0.4 kg', '0.45 kg'])
    expect(cellPathValues(map, first)).toEqual(['0.4 kg'])
  })

  it('falls back to bare path for older responses', () => {
    const map = { 'articles>net_weight': ['0.2 kg'] }
    expect(cellPathValues(map, 'articles>net_weight#cardinality:all|unit:kg')).toEqual(['0.2 kg'])
  })
})
