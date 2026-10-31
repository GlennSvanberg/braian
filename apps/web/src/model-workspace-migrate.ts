import type { Json } from './api'
import { remove, splitPair, tabGroup, views as layoutViews, type Node } from './layout'
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
  'import_mappings',
  'data_model',
  'attribute_definitions',
  'proposal',
  'proposal_list',
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
      // Type attributes first so it stays the active center tab after a legacy reset
      // (same order as the seed: Attributes @0, Attribute definitions @1).
      tabGroup([ids[6] ?? '', ids[7] ?? ''].filter(Boolean)),
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

function isTypeAttributesView(view: SavedView) {
  if (view.type !== 'data_model') return false
  const mode = modeOf(view)
  if (mode === 'classes' || mode === 'attributes') return true
  return titleOf(view) === 'attributes' || titleOf(view) === 'type attributes'
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

/** Data model workspaces: seeded share token, name, or model table / tree panes. */
export function isDataModelWorkspace(
  workspace: { share_token?: string | null; name?: string },
  views: SavedView[],
): boolean {
  if (workspace.share_token === 'braian-model') return true
  if (/^data model\b/i.test(String(workspace.name ?? '').trim())) return true
  if (views.some((view) => view.type === 'data_model')) return true
  return views.some((view) =>
    view.type === 'object_types'
    || view.type === 'link_types'
    || view.type === 'value_lists'
    || view.type === 'units'
    || view.type === 'attribute_definitions',
  )
}

export function hasFullModelTemplate(views: SavedView[]): boolean {
  for (const spec of MODEL_WORKSPACE_VIEW_SPECS) {
    if (spec.type === 'data_model' && spec.state.mode === 'tree') {
      const ok = views.some(
        (view) =>
          view.type === 'data_model'
          && !isLegacyPreviewView(view)
          && !isTypeAttributesView(view)
          && (modeOf(view) === 'tree' || titleOf(view) === 'structures' || titleOf(view) === 'model' || modeOf(view) === ''),
      )
      if (!ok) return false
      continue
    }
    if (spec.type === 'data_model' && spec.state.mode === 'classes') {
      if (!views.some(isTypeAttributesView)) return false
      continue
    }
    if (!views.some((view) => view.type === spec.type)) return false
  }
  return true
}

const UUID_RE = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i

/** True when a tab label would paint as a raw id. */
export function looksLikeRawViewId(title: string) {
  return UUID_RE.test(title.trim())
}

export function pruneMissingViewIds(layout: Node, existingIds: Set<string>): { layout: Node; removed: string[] } {
  const open = viewsFromLayout(layout)
  const removed = open.filter((id) => !existingIds.has(id))
  let next = layout
  for (const id of removed) next = remove(next, id)
  return { layout: next, removed }
}

/** True when the dock would paint Unavailable / empty panes for missing view ids. */
export function layoutHasMissingViews(layout: Node | undefined, views: SavedView[]): boolean {
  if (!layout) return false
  const existingIds = new Set(views.map((view) => view.id))
  return viewsFromLayout(layout).some((id) => Boolean(id) && !existingIds.has(id))
}

function viewsFromLayout(layout: Node): string[] {
  return layoutViews(layout)
}

type ModelMigration = Pick<WorkspaceMigration, 'changed' | 'views' | 'create' | 'layout'>

/**
 * Reset / rebuild onto the current standard Data model template.
 * Remaps existing panes onto template slots and creates any missing tables.
 */
export function buildModelTemplateMigration(
  views: SavedView[],
  options?: { newId?: () => string },
): ModelMigration {
  const newId = options?.newId ?? (() => crypto.randomUUID())
  const updates: WorkspaceMigration['views'] = []
  const create: NonNullable<WorkspaceMigration['create']> = []
  const used = new Set<string>()
  const orderedIds: string[] = []

  const take = (predicate: (view: SavedView) => boolean) => {
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

  const assign = (spec: (typeof MODEL_WORKSPACE_VIEW_SPECS)[number], previous: SavedView | undefined) => {
    if (!previous) {
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
      return
    }
    orderedIds.push(previous.id)
    const state: Json = { ...(spec.type === previous.type ? previous.state ?? {} : {}), ...spec.state }
    const typeChanged = previous.type !== spec.type
    const titleChanged = previous.title !== spec.title
    const stateChanged = JSON.stringify(previous.state ?? {}) !== JSON.stringify(state)
    if (typeChanged || titleChanged || stateChanged) {
      updates.push({
        id: previous.id,
        title: spec.title,
        state,
        ...(typeChanged ? { type: spec.type } : {}),
      })
    }
  }

  for (const spec of MODEL_WORKSPACE_VIEW_SPECS) {
    if (spec.type === 'data_model' && spec.state.mode === 'tree') {
      assign(spec, structures)
      continue
    }
    if (spec.type === 'data_model' && spec.state.mode === 'classes') {
      assign(spec, classes)
      continue
    }
    if (spec.type === 'attribute_definitions') {
      assign(spec, attributeDefinitions)
      continue
    }
    const existing = take((view) => view.type === spec.type)
    if (existing) {
      assign(spec, existing)
      continue
    }
    if (convertible) {
      const previous = convertible
      convertible = undefined
      assign(spec, previous)
      continue
    }
    assign(spec, undefined)
  }

  return {
    changed: true,
    views: updates,
    create,
    layout: modelWorkspaceLayout(orderedIds),
  }
}

/**
 * Reset a pre-#99 Data model workspace onto the current standard template.
 * Idempotent once `object_types` exists (unless healDataModelWorkspace takes over).
 */
export function migrateLegacyModelViews(
  workspace: { share_token?: string | null; name?: string },
  views: SavedView[],
  options?: { newId?: () => string },
): ModelMigration {
  if (!needsLegacyModelMigration(workspace, views)) {
    return { changed: false, views: [], create: [], layout: undefined }
  }
  return buildModelTemplateMigration(views, options)
}

export type HealResult = ModelMigration & { removed: string[] }

/**
 * Drop layout entries that point at missing views. If this is a Data model
 * workspace that still lacks the standard tables, rebuild the template.
 * Idempotent when the layout is already clean and the template is complete.
 */
export function healDataModelWorkspace(
  workspace: { share_token?: string | null; name?: string },
  views: SavedView[],
  layout: Node | undefined,
  options?: { newId?: () => string },
): HealResult {
  const existingIds = new Set(views.map((view) => view.id))
  const baseLayout = layout ?? tabGroup([])
  const pruned = pruneMissingViewIds(baseLayout, existingIds)
  const incomplete = isDataModelWorkspace(workspace, views) && !hasFullModelTemplate(views)

  if (incomplete || needsLegacyModelMigration(workspace, views)) {
    const rebuilt = buildModelTemplateMigration(views, options)
    return { ...rebuilt, removed: pruned.removed }
  }

  if (pruned.removed.length === 0) {
    return { changed: false, views: [], create: [], layout: undefined, removed: [] }
  }

  return {
    changed: true,
    views: [],
    create: [],
    layout: pruned.layout,
    removed: pruned.removed,
  }
}
