import { splitPair, tabGroup, type Node } from './layout'
import type { SavedView, WorkspaceMigration } from './type-labels'

/** View specs for the Data model workspace, matching `templates.ts` id `model`. */
export const MODEL_WORKSPACE_VIEW_SPECS = [
  { type: 'data_model', title: 'Structures', state: { mode: 'tree' } },
  { type: 'object_types', title: 'Object types', state: {} },
  { type: 'link_types', title: 'Link types', state: {} },
  { type: 'value_lists', title: 'Value lists', state: {} },
  { type: 'value_list_values', title: 'List values', state: {} },
  { type: 'units', title: 'Units', state: {} },
  { type: 'data_model', title: 'Type attributes', state: { mode: 'classes' } },
  { type: 'attribute_definitions', title: 'Attribute definitions', state: {} },
] as const

/** Built-in view types the shell can render. Anything else is an unknown placeholder. */
export const KNOWN_VIEW_TYPES = new Set([
  'structure_tree',
  'assortment_list',
  'object_table',
  'object_detail',
  'graph',
  'history',
  'class_attributes',
  'object_types',
  'link_types',
  'value_lists',
  'value_list_values',
  'units',
  'data_model',
  'attribute_definitions',
  'proposal',
  'batch_log',
  'job_history',
  'embed',
])

export function isKnownViewType(type: string) {
  return KNOWN_VIEW_TYPES.has(type)
}

export function modelWorkspaceLayout(ids: string[]): Node {
  return splitPair(
    'horizontal',
    0.22,
    tabGroup([ids[0] ?? '', ids[1] ?? '', ids[2] ?? '', ids[3] ?? '', ids[5] ?? ''].filter(Boolean)),
    splitPair(
      'horizontal',
      0.55,
      tabGroup([ids[7] ?? '', ids[6] ?? ''].filter(Boolean)),
      tabGroup([ids[4] ?? ''].filter(Boolean)),
    ),
  )
}

function modeOf(view: SavedView) {
  return String(view.state?.mode ?? '').trim().toLowerCase()
}

function titleOf(view: SavedView) {
  return view.title.trim().toLowerCase()
}

/** Old Preview / card editor pane removed in #99. */
export function isLegacyPreviewView(view: SavedView) {
  if (view.type !== 'data_model') return false
  const mode = modeOf(view)
  return mode === 'card' || mode === 'editor' || titleOf(view) === 'preview'
}

export function needsLegacyModelMigration(
  workspace: { share_token?: string | null; name?: string },
  views: SavedView[],
): boolean {
  if (!views.some((view) => view.type === 'data_model')) return false
  if (views.some((view) => view.type === 'object_types')) return false
  if (workspace.share_token === 'braian-model') return true
  if (views.some(isLegacyPreviewView)) return true
  if (views.some((view) => view.type === 'data_model' && titleOf(view) === 'model')) return true
  return false
}

type ModelMigration = Pick<WorkspaceMigration, 'changed' | 'views' | 'create' | 'layout'>

/**
 * Replace a pre-#99 Data model workspace with the current standard template.
 * User panes and column prefs are not preserved (personal layouts can be reset).
 * Idempotent: once `object_types` exists, this is a no-op.
 */
export function migrateLegacyModelViews(
  workspace: { share_token?: string | null; name?: string },
  views: SavedView[],
  options?: { newId?: () => string },
): ModelMigration {
  if (!needsLegacyModelMigration(workspace, views)) {
    return { changed: false, views: [], create: [], layout: undefined }
  }

  const newId = options?.newId ?? (() => crypto.randomUUID())
  const orderedIds: string[] = []
  const create: NonNullable<WorkspaceMigration['create']> = MODEL_WORKSPACE_VIEW_SPECS.map((spec, index) => {
    const id = newId()
    orderedIds.push(id)
    return {
      id,
      type: spec.type,
      title: spec.title,
      state: { ...spec.state },
      pane: 'main',
      position: index,
    }
  })

  return {
    changed: true,
    views: [],
    create,
    layout: modelWorkspaceLayout(orderedIds),
  }
}
