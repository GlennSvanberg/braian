import type { Json } from './api'
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
  const extras = ids.slice(MODEL_WORKSPACE_VIEW_SPECS.length).filter(Boolean)
  return splitPair(
    'horizontal',
    0.22,
    tabGroup(
      [ids[0] ?? '', ids[1] ?? '', ids[2] ?? '', ids[3] ?? '', ids[5] ?? '', ...extras].filter(Boolean),
    ),
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

/** Old type-attributes pane (still data_model, mode classes). */
export function isTypeAttributesView(view: SavedView) {
  if (view.type !== 'data_model') return false
  const mode = modeOf(view)
  if (mode === 'classes' || mode === 'attributes') return true
  return titleOf(view) === 'attributes' || titleOf(view) === 'type attributes'
}

/**
 * Old ModelNav tree pane. Before #99 the template titled it "Model" with mode
 * `tree`; the Structures tree kept the same type with title "Structures".
 */
export function isLegacyModelNavView(view: SavedView) {
  if (view.type !== 'data_model') return false
  if (isLegacyPreviewView(view) || isTypeAttributesView(view)) return false
  if (modeOf(view) === 'impact') return false
  return titleOf(view) === 'model' || modeOf(view) === 'tree' || modeOf(view) === ''
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
 * Rewrite a pre-#99 Data model workspace onto the standard tables.
 * Keeps the Structures tree and Type attributes panes; converts Preview into a
 * table view when possible; adds any missing standard tables; rebuilds layout.
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
  const updates: WorkspaceMigration['views'] = []
  const create: NonNullable<WorkspaceMigration['create']> = []
  const used = new Set<string>()

  const take = (predicate: (view: SavedView) => boolean): SavedView | undefined => {
    const found = views.find((view) => !used.has(view.id) && predicate(view))
    if (found) used.add(found.id)
    return found
  }

  const structures =
    take((view) => view.type === 'data_model' && titleOf(view) === 'structures')
    ?? take((view) => view.type === 'data_model' && titleOf(view) === 'model')
    ?? take((view) => view.type === 'data_model' && modeOf(view) === 'tree')
    ?? take((view) => view.type === 'data_model' && !isLegacyPreviewView(view) && !isTypeAttributesView(view))

  const classes = take(isTypeAttributesView)
  const attributeDefinitions = take((view) => view.type === 'attribute_definitions')
  let convertible = take(isLegacyPreviewView)

  const orderedIds: string[] = []

  const pushUpdate = (id: string, spec: (typeof MODEL_WORKSPACE_VIEW_SPECS)[number], previous: SavedView) => {
    orderedIds.push(id)
    const state: Json = { ...(spec.type === previous.type ? previous.state ?? {} : {}), ...spec.state }
    const typeChanged = previous.type !== spec.type
    const titleChanged = previous.title !== spec.title
    const stateChanged = JSON.stringify(previous.state ?? {}) !== JSON.stringify(state)
    if (typeChanged || titleChanged || stateChanged) {
      updates.push({
        id,
        title: spec.title,
        state,
        ...(typeChanged ? { type: spec.type } : {}),
      })
    }
  }

  const pushCreate = (spec: (typeof MODEL_WORKSPACE_VIEW_SPECS)[number]) => {
    const id = newId()
    orderedIds.push(id)
    create.push({
      id,
      type: spec.type,
      title: spec.title,
      state: { ...spec.state },
      pane: 'main',
      position: orderedIds.length - 1,
    })
  }

  for (const spec of MODEL_WORKSPACE_VIEW_SPECS) {
    if (spec.type === 'data_model' && spec.state.mode === 'tree') {
      if (structures) pushUpdate(structures.id, spec, structures)
      else pushCreate(spec)
      continue
    }
    if (spec.type === 'data_model' && spec.state.mode === 'classes') {
      if (classes) pushUpdate(classes.id, spec, classes)
      else pushCreate(spec)
      continue
    }
    if (spec.type === 'attribute_definitions') {
      if (attributeDefinitions) pushUpdate(attributeDefinitions.id, spec, attributeDefinitions)
      else pushCreate(spec)
      continue
    }
    const existing = take((view) => view.type === spec.type)
    if (existing) {
      pushUpdate(existing.id, spec, existing)
      continue
    }
    if (convertible) {
      pushUpdate(convertible.id, spec, convertible)
      convertible = undefined
      continue
    }
    pushCreate(spec)
  }

  // Keep panes the user added (embeds, extra tables). Do not re-home leftover
  // unused legacy Model/Preview clones — those are replaced by the specs above.
  for (const view of views) {
    if (used.has(view.id)) continue
    if (isLegacyPreviewView(view) || isLegacyModelNavView(view)) continue
    orderedIds.push(view.id)
  }

  return {
    changed: true,
    views: updates,
    create,
    layout: modelWorkspaceLayout(orderedIds),
  }
}
