import { describe, expect, it } from 'vitest'
import {
  MODEL_WORKSPACE_VIEW_SPECS,
  isKnownViewType,
  isLegacyPreviewView,
  migrateLegacyModelViews,
  needsLegacyModelMigration,
} from './model-workspace-migrate'
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
    expect(isLegacyPreviewView(legacyViews[2]!)).toBe(true)
    const modern = [
      { id: 's', type: 'data_model', title: 'Structures', state: { mode: 'tree' } },
      { id: 'o', type: 'object_types', title: 'Object types', state: {} },
    ]
    expect(needsLegacyModelMigration({ share_token: 'braian-model' }, modern)).toBe(false)
  })

  it('remaps Model/Preview onto the standard template and creates missing tables', () => {
    let n = 0
    const patch = migrateLegacyModelViews({ share_token: 'braian-model' }, legacyViews, {
      newId: () => `new-${++n}`,
    })
    expect(patch.changed).toBe(true)
    expect(patch.views.find((view) => view.id === 'v-model')).toMatchObject({ title: 'Structures' })
    expect(patch.views.find((view) => view.id === 'v-attrs')).toMatchObject({ title: 'Type attributes' })
    expect(patch.views.find((view) => view.id === 'v-preview')).toMatchObject({
      type: 'object_types',
      title: 'Object types',
    })
    expect((patch.create ?? []).map((view) => view.type)).toEqual([
      'link_types',
      'value_lists',
      'value_list_values',
      'units',
    ])
    expect(JSON.stringify(patch.layout)).toContain('v-model')
  })

  it('is a no-op once object_types exists (persist must not run)', () => {
    const modern = [
      { id: 's', type: 'data_model', title: 'Structures', state: { mode: 'tree' } },
      { id: 'o', type: 'object_types', title: 'Object types', state: {} },
    ]
    const patch = migrateLegacyModelViews({ share_token: 'braian-model' }, modern)
    expect(patch.changed).toBe(false)
    expect(patch.create ?? []).toEqual([])
    expect(patch.layout).toBeUndefined()
  })

  it('marks unknown view types', () => {
    expect(isKnownViewType('object_types')).toBe(true)
    expect(isKnownViewType('model_nav')).toBe(false)
  })
})
