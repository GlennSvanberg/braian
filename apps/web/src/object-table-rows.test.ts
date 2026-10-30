import { describe, expect, it } from 'vitest'
import { objectTableRowsQueryKey, toObjRow, type ObjectTableRowsScope } from './object-table-rows'

describe('object-table-rows', () => {
  it('builds a stable query key for the same scope', () => {
    const scope: ObjectTableRowsScope = {
      viewId: 'v1',
      queryText: '',
      filterBy: 'type_key:=article',
      sortBy: 'a_number:asc',
      groupField: '',
      pages: [1, 2],
      pathValueColumns: ['description@en'],
      typeKey: 'article',
    }
    expect(objectTableRowsQueryKey(scope)).toEqual(objectTableRowsQueryKey({ ...scope, pages: [1, 2] }))
    expect(objectTableRowsQueryKey(scope)[0]).toBe('object-table-rows')
  })

  it('toObjRow maps search documents', () => {
    const row = toObjRow({ id: '1', key: 'A', type_key: 'article', version: 3, name: 'N' })
    expect(row).toMatchObject({ id: '1', key: 'A', type: 'article', version: '3' })
  })
})
