import { describe, expect, it } from 'vitest'
import {
  MODEL_WORKSPACE_VIEW_SPECS,
  isKnownViewType,
  isLegacyPreviewView,
  migrateLegacyModelViews,
  needsLegacyModelMigration,
} from './model-workspace-migrate'
import { migrateSavedWorkspace, typeNames } from './type-labels'
import { workspaceTemplates } from './templates'

const legacyViews = [
  { id: 'v-model', type: 'data_model', title: 'Model', state: { mode: 'tree' } },
  { id: 'v-attrs', type: 'data_model', title: 'Attributes', state: { mode: 'classes' } },
  { id: 'v-preview', type: 'data_model', title: 'Preview', state: { mode: 'card' } },
  { id: 'v-defs', type: 'attribute_definitions', title: 'Attribute definitions', state: {} },
]

describe('legacy data model workspace migration', () => {
  it('matches the model template view list', () => {
    const model = workspaceTemplates([]).find((item) => item.id === 'model')
    expect(model?.views.map((view) => ({ type: view.type, title: view.title, state: view.state }))).toEqual(
      MODEL_WORKSPACE_VIEW_SPECS.map((view) => ({ type: view.type, title: view.title, state: { ...view.state } })),
    )
  })

  it('detects pre-#99 Model + Preview workspaces and not already-migrated ones', () => {
    expect(needsLegacyModelMigration({ share_token: 'braian-model' }, legacyViews)).toBe(true)
    expect(needsLegacyModelMigration({ name: 'Data model' }, legacyViews)).toBe(true)
    expect(isLegacyPreviewView(legacyViews[2]!)).toBe(true)
    const modern = [
      { id: 's', type: 'data_model', title: 'Structures', state: { mode: 'tree' } },
      { id: 'o', type: 'object_types', title: 'Object types', state: {} },
    ]
    expect(needsLegacyModelMigration({ share_token: 'braian-model' }, modern)).toBe(false)
  })

  it('rewrites Model/Preview into Structures and standard tables', () => {
    let n = 0
    const patch = migrateLegacyModelViews({ share_token: 'braian-model' }, legacyViews, {
      newId: () => `new-${++n}`,
    })
    expect(patch.changed).toBe(true)
    expect(patch.views.find((view) => view.id === 'v-model')).toMatchObject({
      title: 'Structures',
      state: { mode: 'tree' },
    })
    expect(patch.views.find((view) => view.id === 'v-attrs')).toMatchObject({
      title: 'Type attributes',
      state: { mode: 'classes' },
    })
    expect(patch.views.find((view) => view.id === 'v-preview')).toMatchObject({
      type: 'object_types',
      title: 'Object types',
    })
    const createdTypes = (patch.create ?? []).map((view) => view.type)
    expect(createdTypes).toEqual(['link_types', 'value_lists', 'value_list_values', 'units'])
    expect(patch.layout).toBeTruthy()
    const layoutText = JSON.stringify(patch.layout)
    expect(layoutText).toContain('v-model')
    expect(layoutText).toContain('v-preview')
    expect(layoutText).toContain('new-1')
  })

  it('preserves Structures and is idempotent on the second pass', () => {
    const first = migrateLegacyModelViews({ share_token: 'braian-model' }, legacyViews, {
      newId: () => 'created-id',
    })
    const nextViews = [
      { id: 'v-model', type: 'data_model', title: 'Structures', state: { mode: 'tree' } },
      { id: 'v-attrs', type: 'data_model', title: 'Type attributes', state: { mode: 'classes' } },
      { id: 'v-preview', type: 'object_types', title: 'Object types', state: {} },
      { id: 'v-defs', type: 'attribute_definitions', title: 'Attribute definitions', state: {} },
      ...(first.create ?? []).map((view) => ({
        id: view.id,
        type: view.type,
        title: view.title,
        state: view.state,
      })),
    ]
    const again = migrateLegacyModelViews({ share_token: 'braian-model' }, nextViews)
    expect(again.changed).toBe(false)
    expect(again.views).toEqual([])
    expect(again.create ?? []).toEqual([])
    expect(again.layout).toBeUndefined()
    expect(nextViews.find((view) => view.id === 'v-model')?.title).toBe('Structures')
  })

  it('marks unknown view types and keeps known ones', () => {
    expect(isKnownViewType('object_types')).toBe(true)
    expect(isKnownViewType('data_model')).toBe(true)
    expect(isKnownViewType('model_nav')).toBe(false)
    expect(isKnownViewType('preview')).toBe(false)
  })

  it('save-back only when the composed migration reports a change', () => {
    const types = typeNames({
      types: [{ id: '200', key: 'product', labels: { en: 'Product' } }],
    })
    const objectPatch = migrateSavedWorkspace({ name: 'Data model', share_token: 'braian-model' }, legacyViews, types)
    const modelPatch = migrateLegacyModelViews({ share_token: 'braian-model' }, legacyViews, {
      newId: () => 'x',
    })
    const first = {
      changed: objectPatch.changed || modelPatch.changed,
      create: modelPatch.create,
      layout: modelPatch.layout,
    }
    expect(first.changed).toBe(true)
    expect(first.create?.length).toBeGreaterThan(0)
    expect(first.layout).toBeTruthy()

    const migratedViews = [
      { id: 'v-model', type: 'data_model', title: 'Structures', state: { mode: 'tree' } },
      { id: 'v-attrs', type: 'data_model', title: 'Type attributes', state: { mode: 'classes' } },
      { id: 'v-preview', type: 'object_types', title: 'Object types', state: {} },
      { id: 'v-defs', type: 'attribute_definitions', title: 'Attribute definitions', state: {} },
      { id: 'a', type: 'link_types', title: 'Link types', state: {} },
      { id: 'b', type: 'value_lists', title: 'Value lists', state: {} },
      { id: 'c', type: 'value_list_values', title: 'List values', state: {} },
      { id: 'd', type: 'units', title: 'Units', state: {} },
    ]
    const againObject = migrateSavedWorkspace({ name: 'Data model', share_token: 'braian-model' }, migratedViews, types)
    const againModel = migrateLegacyModelViews({ share_token: 'braian-model' }, migratedViews)
    const again = { changed: againObject.changed || againModel.changed }
    // Shell only calls persist when patch.changed is true.
    expect(again.changed).toBe(false)
  })

  it('keeps user-added panes outside the legacy set', () => {
    const withCustom = [
      ...legacyViews,
      { id: 'v-notes', type: 'embed', title: 'My notes', state: { url: 'https://example.com' } },
    ]
    const patch = migrateLegacyModelViews({ share_token: 'braian-model' }, withCustom, {
      newId: () => 'new-extra',
    })
    expect(patch.changed).toBe(true)
    const layoutText = JSON.stringify(patch.layout)
    expect(layoutText).toContain('v-notes')
    expect(withCustom.find((view) => view.id === 'v-notes')?.title).toBe('My notes')
  })

  it('reports no change for an already modern workspace (persist must not run)', () => {
    const modern = [
      { id: 's', type: 'data_model', title: 'Structures', state: { mode: 'tree' } },
      { id: 'o', type: 'object_types', title: 'Object types', state: {} },
      { id: 'l', type: 'link_types', title: 'Link types', state: {} },
      { id: 'vl', type: 'value_lists', title: 'Value lists', state: {} },
      { id: 'u', type: 'units', title: 'Units', state: {} },
      { id: 'ta', type: 'data_model', title: 'Type attributes', state: { mode: 'classes' } },
    ]
    expect(needsLegacyModelMigration({ share_token: 'braian-model' }, modern)).toBe(false)
    const patch = migrateLegacyModelViews({ share_token: 'braian-model' }, modern)
    // Shell only calls persistWorkspaceMigration when patch.changed is true.
    expect(patch.changed).toBe(false)
    expect(patch.views).toEqual([])
    expect(patch.create ?? []).toEqual([])
    expect(patch.layout).toBeUndefined()
  })
})
