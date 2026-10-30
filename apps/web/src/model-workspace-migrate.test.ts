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
    expect(needsLegacyModelMigration({ name: 'Data model' }, legacyViews)).toBe(true)
    expect(isLegacyPreviewView(legacyViews[2]!)).toBe(true)
    const modern = [
      { id: 's', type: 'data_model', title: 'Structures', state: { mode: 'tree' } },
      { id: 'o', type: 'object_types', title: 'Object types', state: {} },
    ]
    expect(needsLegacyModelMigration({ share_token: 'braian-model' }, modern)).toBe(false)
  })

  it('resets a legacy workspace onto the standard template views', () => {
    let n = 0
    const patch = migrateLegacyModelViews({ share_token: 'braian-model' }, legacyViews, {
      newId: () => `new-${++n}`,
    })
    expect(patch.changed).toBe(true)
    expect(patch.views).toEqual([])
    expect((patch.create ?? []).map((view) => view.type)).toEqual(
      MODEL_WORKSPACE_VIEW_SPECS.map((view) => view.type),
    )
    expect((patch.create ?? []).map((view) => view.title)).toEqual(
      MODEL_WORKSPACE_VIEW_SPECS.map((view) => view.title),
    )
    expect(patch.layout).toBeTruthy()
    const layoutText = JSON.stringify(patch.layout)
    expect(layoutText).toContain('new-1')
    expect(layoutText).toContain('new-8')
  })

  it('is a no-op once object_types exists (persist must not run)', () => {
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
    expect(patch.changed).toBe(false)
    expect(patch.views).toEqual([])
    expect(patch.create ?? []).toEqual([])
    expect(patch.layout).toBeUndefined()
  })

  it('marks unknown view types and keeps known ones', () => {
    expect(isKnownViewType('object_types')).toBe(true)
    expect(isKnownViewType('data_model')).toBe(true)
    expect(isKnownViewType('model_nav')).toBe(false)
    expect(isKnownViewType('preview')).toBe(false)
  })
})
