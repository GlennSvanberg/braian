import { describe, expect, it } from 'vitest'
import { exportPayload } from './export'

const columns = [
  { id: 'key', header: 'Number' },
  { id: 'name', header: 'Name' },
  { id: 'type', header: 'Type' },
  { id: 'status', header: 'Status' },
  { id: 'version', header: 'Version' },
]

describe('exportPayload', () => {
  it('sends the visible columns, filters, and data language with no format choice', () => {
    const body = exportPayload({
      columns,
      hidden: ['status'],
      order: ['name', 'key', 'type', 'status', 'version'],
      q: 'cheddar',
      filter: 'groups:=sortiment:friluft',
      sort: 'a_nummer:asc',
      ids: [],
      lang: 'sv',
    })
    expect(body).toEqual({
      columns: [
        { id: 'name', header: 'Name' },
        { id: 'key', header: 'Number' },
        { id: 'type', header: 'Type' },
        { id: 'version', header: 'Version' },
      ],
      lang: 'sv',
      sort: 'a_nummer:asc',
      q: 'cheddar',
      filter: 'groups:=sortiment:friluft',
    })
    expect(body).not.toHaveProperty('format')
  })

  it('exports the selection and ignores the search when rows are selected', () => {
    const body = exportPayload({
      columns,
      hidden: [],
      order: [],
      q: 'ignored',
      filter: 'groups:=sortiment:friluft',
      sort: 'version:desc',
      ids: ['018f0000-0000-7000-8000-000000000001'],
      lang: '  ',
    })
    expect(body.ids).toEqual(['018f0000-0000-7000-8000-000000000001'])
    expect(body.lang).toBe('en')
    expect(body.q).toBeUndefined()
    expect(body.filter).toBeUndefined()
    expect(body.columns.map((column) => column.id)).toEqual(['key', 'name', 'type', 'status', 'version'])
  })
})
