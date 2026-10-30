import { describe, expect, it } from 'vitest'
import { withSystemViewFields, type FieldDef } from './catalog'

const base: FieldDef[] = [
  { id: 'number', name: 'Number', typeLabel: 'text', shape: 'text' },
  { id: 'name', name: 'Name', typeLabel: 'text', shape: 'text' },
  { id: 'status', name: 'Status', typeLabel: 'text', shape: 'text' },
]

describe('withSystemViewFields', () => {
  it('inserts type and version into the same list without a separate group', () => {
    const fields = withSystemViewFields(base, [
      { key: 'type', label: 'Type', default_index: 2 },
      { key: 'version', label: 'Version', default_index: 4 },
    ])
    expect(fields.map((field) => field.id)).toEqual(['number', 'name', 'type', 'status', 'version'])
    expect(fields.find((field) => field.id === 'type')?.name).toBe('Type')
  })

  it('does not duplicate an existing field id', () => {
    const fields = withSystemViewFields(
      [...base, { id: 'type', name: 'Type', typeLabel: 'text', shape: 'text' }],
      [{ key: 'type', label: 'Type', default_index: 2 }],
    )
    expect(fields.filter((field) => field.id === 'type')).toHaveLength(1)
  })
})
