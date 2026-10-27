import { describe, expect, it } from 'vitest'
import { classSections } from './class-sections'
import type { Json } from './api'
import { typeNames } from './type-labels'
import { missingTypeWorkspaces, orderWorkspaces, typeShareToken, typeUsesTree } from './type-workspaces'
import { workspaceTemplates } from './templates'

const defs: Json = {
  attributes: [
    { id: 'name', key: 'name', labels: { en: 'Name' }, kind: { type: 'text' } },
    { id: 'weight', key: 'weight', labels: { en: 'Weight' }, kind: { type: 'number' } },
    { id: 'length', key: 'length', labels: { en: 'Length' }, kind: { type: 'number' } },
  ],
  types: [
    { id: '200', key: 'product', singular: { en: 'Product' }, plural: { en: 'Products' }, attributes: ['name'], structures: ['food'] },
    { id: '201', key: 'article', singular: { en: 'Article' }, plural: { en: 'Articles' }, attributes: ['name', 'weight'], structures: ['food'] },
    { id: '202', key: 'supplier', singular: { en: 'Supplier' }, plural: { en: 'Suppliers' }, attributes: ['name'], structures: [] },
  ],
  structures: [
    {
      key: 'food',
      labels: { en: 'Food' },
      groups: [
        { id: 'dairy', key: 'dairy', labels: { en: 'Dairy & eggs' }, attributes: ['weight'] },
        { id: 'cheese', key: 'cheese', parent: 'dairy', labels: { en: 'Cheese' }, attributes: ['length'] },
      ],
      classes: [],
    },
  ],
}

describe('type workspaces', () => {
  it('orders one workspace per type between the data model and review', () => {
    const types = typeNames(defs, 'en')
    const ordered = orderWorkspaces(
      [
        { name: 'Review', share_token: 'braian-review' },
        { name: 'Articles', share_token: 'braian-type-article' },
        { name: 'Suppliers', share_token: 'braian-type-supplier' },
        { name: 'Products', share_token: 'braian-type-product' },
        { name: 'Mine', share_token: 'user-1' },
        { name: 'Data model', share_token: 'braian-model' },
      ],
      types,
    )
    expect(ordered.map((item) => item.name)).toEqual([
      'Data model',
      'Products',
      'Articles',
      'Suppliers',
      'Mine',
      'Review',
    ])
    expect(typeShareToken('product')).toBe('braian-type-product')
    expect(typeUsesTree({ structures: ['food'] })).toBe(true)
    expect(typeUsesTree({})).toBe(true)
    expect(typeUsesTree({ structures: [] })).toBe(false)
    expect(missingTypeWorkspaces(ordered, types).map((type) => type.key)).toEqual([])
    expect(missingTypeWorkspaces([{ share_token: 'braian-model' }], types).map((type) => type.key)).toEqual([
      'product',
      'article',
      'supplier',
    ])
  })

  it('builds a template per type and none named Enrich', () => {
    const templates = workspaceTemplates(typeNames(defs, 'en'))
    expect(templates.map((item) => item.name)).toEqual(['Products', 'Articles', 'Suppliers', 'Data model', 'Review'])
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
    expect(articles?.views[2]?.state).toMatchObject({ type_key: 'article', object_type: '201' })
    expect(articles?.views[0]?.title).toBe('Food')
    expect(articles?.views[0]?.state).toEqual({})
    const suppliers = templates[2]
    expect(suppliers?.views.map((view) => view.type)).toEqual([
      'assortment_list',
      'object_table',
      'object_detail',
      'graph',
      'history',
    ])
  })
})

describe('class sections', () => {
  it('lists the type and each class on the group chain', () => {
    const sections = classSections(defs, 'article', { food: 'cheese' })
    expect(sections.map((section) => section.title)).toEqual(['Article', 'Dairy & eggs', 'Cheese'])
    expect(sections[0]?.attributes.map((item) => item.label)).toEqual(['Name', 'Weight'])
    expect(sections[1]?.attributes.map((item) => item.key)).toEqual(['weight'])
    expect(sections[2]?.attributes.map((item) => item.key)).toEqual(['length'])
  })
})
