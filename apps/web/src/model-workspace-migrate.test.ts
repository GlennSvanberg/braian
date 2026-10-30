import { describe, expect, it } from 'vitest'
import { tabGroup } from './layout'
import {
  MODEL_WORKSPACE_VIEW_SPECS,
  healDataModelWorkspace,
  hasFullModelTemplate,
  isKnownViewType,
  isLegacyPreviewView,
  looksLikeRawViewId,
  migrateLegacyModelViews,
  needsLegacyModelMigration,
  pruneMissingViewIds,
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

describe('dangling Data model layout heal', () => {
  it('detects raw UUID tab labels', () => {
    expect(looksLikeRawViewId('fc83990b-8fa8-4c11-9abc-1234567890ab')).toBe(true)
    expect(looksLikeRawViewId('Structures')).toBe(false)
  })

  it('prunes layout ids that are not in the view list', () => {
    const ghost = 'fc83990b-8fa8-4c11-9abc-1234567890ab'
    const layout = tabGroup(['v-model', ghost])
    const pruned = pruneMissingViewIds(layout, new Set(['v-model']))
    expect(pruned.removed).toEqual([ghost])
    expect(JSON.stringify(pruned.layout)).toContain('v-model')
    expect(JSON.stringify(pruned.layout)).not.toContain(ghost)
  })

  it('rebuilds when a Data model workspace has dangling panes and incomplete tables', () => {
    const ghostA = 'fc83990b-8fa8-4c11-9abc-1234567890ab'
    const ghostB = 'c7a89fa4-d2dd-4aaa-bbbb-cccccccccccc'
    const views = [
      { id: 'v-model', type: 'data_model', title: 'Structures', state: { mode: 'tree' } },
      { id: 'v-ot', type: 'object_types', title: 'Object types', state: {} },
    ]
    expect(hasFullModelTemplate(views)).toBe(false)
    const layout = {
      type: 'split' as const,
      dir: 'horizontal' as const,
      ratios: [0.5, 0.5],
      children: [tabGroup(['v-model', ghostA]), tabGroup([ghostB])],
    }
    let n = 0
    const patch = healDataModelWorkspace(
      { name: 'Data model 8', share_token: null },
      views,
      layout,
      { newId: () => `new-${++n}` },
    )
    expect(patch.changed).toBe(true)
    expect(patch.removed).toEqual(expect.arrayContaining([ghostA, ghostB]))
    expect((patch.create ?? []).map((view) => view.type)).toEqual(
      expect.arrayContaining(['link_types', 'value_lists', 'units', 'value_list_values']),
    )
    expect(JSON.stringify(patch.layout)).not.toContain(ghostA)
  })

  it('only prunes when the template is already complete', () => {
    const ghost = '03a1fbaf-aa57-43e7-aaaa-bbbbbbbbbbbb'
    const views = MODEL_WORKSPACE_VIEW_SPECS.map((spec, index) => ({
      id: `v-${index}`,
      type: spec.type,
      title: spec.title,
      state: { ...spec.state },
    }))
    expect(hasFullModelTemplate(views)).toBe(true)
    const layout = tabGroup([...views.map((view) => view.id), ghost])
    const patch = healDataModelWorkspace({ name: 'Data model', share_token: 'braian-model' }, views, layout)
    expect(patch.changed).toBe(true)
    expect(patch.create ?? []).toEqual([])
    expect(patch.removed).toEqual([ghost])
    expect(JSON.stringify(patch.layout)).not.toContain(ghost)
  })

  it('is a no-op for a healthy modern workspace', () => {
    const views = MODEL_WORKSPACE_VIEW_SPECS.map((spec, index) => ({
      id: `v-${index}`,
      type: spec.type,
      title: spec.title,
      state: { ...spec.state },
    }))
    const layout = tabGroup(views.map((view) => view.id))
    const patch = healDataModelWorkspace({ name: 'Data model', share_token: 'braian-model' }, views, layout)
    expect(patch.changed).toBe(false)
    expect(patch.layout).toBeUndefined()
    expect(patch.removed).toEqual([])
  })
})
