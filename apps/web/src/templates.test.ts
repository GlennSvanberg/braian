import { describe, expect, it } from 'vitest'
import type { Json } from './api'
import { typeNames } from './type-labels'
import { canonicalShareToken, workspaceTemplates } from './templates'
import { typeShareToken } from './type-workspaces'

const defs: Json = {
  attributes: [{ id: 'name', key: 'name', labels: { en: 'Name' }, kind: { type: 'text' } }],
  types: [
    { id: '200', key: 'product', singular: { en: 'Product' }, plural: { en: 'Products' }, attributes: ['name'], structures: ['food'] },
  ],
  structures: [{ key: 'food', labels: { en: 'Food' }, groups: [], classes: [] }],
}

describe('canonicalShareToken', () => {
  it('maps model, review and type templates to stable share tokens', () => {
    const templates = workspaceTemplates(typeNames(defs, 'en'))
    expect(canonicalShareToken(templates.find((t) => t.id === 'model')!)).toBe('braian-model')
    expect(canonicalShareToken(templates.find((t) => t.id === 'review')!)).toBe('braian-review')
    expect(canonicalShareToken(templates.find((t) => t.id === 'type:product')!)).toBe(typeShareToken('product'))
  })
})
