import { describe, expect, it } from 'vitest'
import { classSections } from './class-sections'
import type { Json } from './api'
import { typeNames } from './type-labels'
import { missingTypeWorkspaces, orderWorkspaces, typeShareToken } from './type-workspaces'
import { workspaceTemplates } from './templates'

const defs: Json = {
  attributes: [
    { id: 'name', key: 'name', labels: { en: 'Name' }, kind: { type: 'text' } },
    { id: 'weight', key: 'weight', labels: { en: 'Weight' }, kind: { type: 'number' } },
    { id: 'length', key: 'length', labels: { en: 'Length' }, kind: { type: 'number' } },
  ],
  types: [
    { id: '200', key: 'produkt', labels: { en: 'Product' }, attributes: ['name'] },
    { id: '201', key: 'artikel', singular: { en: 'Article' }, plural: { en: 'Articles' }, attributes: ['name', 'weight'] },
  ],
  structures: [
    {
      key: 'sortiment',
      groups: [
        { id: 'outdoor', key: 'friluft', labels: { en: 'Outdoor' } },
        { id: 'tents', key: 'talt', parent: 'outdoor', labels: { en: 'Tents' } },
      ],
      classes: [
        { id: 'c-outdoor', key: 'friluft', group: 'outdoor', attributes: ['weight'] },
        { id: 'c-tents', key: 'talt', group: 'tents', parent: 'c-outdoor', attributes: ['length'] },
      ],
    },
  ],
}

describe('type workspaces', () => {
  it('orders one workspace per type between the data model and review', () => {
    const types = typeNames(defs, 'en')
    const ordered = orderWorkspaces(
      [
        { name: 'Review', share_token: 'braian-review' },
        { name: 'Articles', share_token: 'braian-type-artikel' },
        { name: 'Products', share_token: 'braian-type-produkt' },
        { name: 'Mine', share_token: 'user-1' },
        { name: 'Data model', share_token: 'braian-model' },
      ],
      types,
    )
    expect(ordered.map((item) => item.name)).toEqual([
      'Data model',
      'Products',
      'Articles',
      'Mine',
      'Review',
    ])
    expect(typeShareToken('produkt')).toBe('braian-type-produkt')
    expect(missingTypeWorkspaces(ordered, types).map((type) => type.key)).toEqual([])
    expect(missingTypeWorkspaces([{ share_token: 'braian-model' }], types).map((type) => type.key)).toEqual([
      'produkt',
      'artikel',
    ])
  })

  it('builds a template per type and none named Enrich', () => {
    const templates = workspaceTemplates(typeNames(defs, 'en'))
    expect(templates.map((item) => item.name)).toEqual(['Products', 'Articles', 'Data model', 'Review', 'Overview'])
    expect(templates.some((item) => item.id === 'enrich' || /enrich/i.test(item.name))).toBe(false)
    const articles = templates[1]
    expect(articles?.views.map((view) => view.type)).toEqual([
      'structure_tree',
      'assortment_list',
      'object_table',
      'object_detail',
      'graph',
      'history',
      'class_attributes',
    ])
    expect(articles?.views[2]?.state).toMatchObject({ type_key: 'artikel', object_type: '201' })
  })
})

describe('class sections', () => {
  it('lists the type and each class on the group chain', () => {
    const sections = classSections(defs, 'artikel', { sortiment: 'talt' })
    expect(sections.map((section) => section.title)).toEqual(['Article', 'Outdoor', 'Tents'])
    expect(sections[0]?.attributes.map((item) => item.label)).toEqual(['Name', 'Weight'])
    expect(sections[1]?.attributes.map((item) => item.key)).toEqual(['weight'])
    expect(sections[2]?.attributes.map((item) => item.key)).toEqual(['length'])
  })
})
