import { api, type Json } from './api'
import { splitPair, tabGroup, type Node } from './layout'
import type { TypeName } from './type-labels'
import { typeUsesTree } from './type-workspaces'

export type WorkspaceTemplate = {
  id: string
  name: string
  summary: string
  views: Array<{ type: string; title: string; state?: Json }>
  layout: (ids: string[]) => Node
}

const TABLE_STATE = {
  columns: ['number', 'name', 'type', 'status'],
  sort: 'number',
}

export function typeWorkspaceTemplate(type: TypeName): WorkspaceTemplate {
  const tree = typeUsesTree(type)
  const treeTitle = type.structureLabel || 'Structure'
  const views: WorkspaceTemplate['views'] = [
    ...(tree ? [{ type: 'structure_tree', title: treeTitle, state: {} }] : []),
    { type: 'assortment_list', title: 'Selections', state: {} },
    {
      type: 'object_table',
      title: type.plural,
      state: { ...TABLE_STATE, object_type: type.id, type_key: type.key },
    },
    { type: 'object_detail', title: 'Detail', state: { language: 'en' } },
    { type: 'graph', title: 'Graph', state: {} },
    { type: 'history', title: 'History', state: {} },
    ...(tree ? [{ type: 'class_attributes', title: 'Group attributes', state: {} }] : []),
  ]
  return {
    id: `type:${type.key}`,
    name: type.plural,
    summary: tree
      ? 'Selections, structure, list, detail, graph, history and group attributes'
      : 'Selections, list, detail, graph and history',
    views,
    layout: (ids) =>
      splitPair(
        'horizontal',
        0.22,
        tabGroup([ids[0] ?? '', ids[1] ?? ''].filter(Boolean).slice(0, tree ? 2 : 1)),
        splitPair(
          'horizontal',
          0.62,
          tabGroup([ids[tree ? 2 : 1] ?? ''].filter(Boolean)),
          tabGroup(ids.slice(tree ? 3 : 2)),
        ),
      ),
  }
}

export function workspaceTemplates(types: TypeName[] = []): WorkspaceTemplate[] {
  return [
  ...types.filter((type) => type.kind !== 'link' && type.id && type.key).map(typeWorkspaceTemplate),
  {
    id: 'model',
    name: 'Data model',
    summary: 'Object types, links, value lists, units and attribute definitions',
    views: [
      { type: 'data_model', title: 'Structures', state: { mode: 'tree' } },
      { type: 'object_types', title: 'Object types', state: {} },
      { type: 'link_types', title: 'Link types', state: {} },
      { type: 'value_lists', title: 'Value lists', state: {} },
      { type: 'value_list_values', title: 'List values', state: {} },
      { type: 'units', title: 'Units', state: {} },
      { type: 'data_model', title: 'Type attributes', state: { mode: 'classes' } },
      { type: 'attribute_definitions', title: 'Attribute definitions', state: {} },
    ],
    layout: (ids) =>
      splitPair(
        'horizontal',
        0.22,
        tabGroup([ids[0] ?? '', ids[1] ?? '', ids[2] ?? '', ids[3] ?? '', ids[5] ?? ''].filter(Boolean)),
        splitPair('horizontal', 0.55, tabGroup([ids[7] ?? '', ids[6] ?? ''].filter(Boolean)), tabGroup([ids[4] ?? ''].filter(Boolean))),
      ),
  },
  {
    id: 'review',
    name: 'Review',
    summary: 'Proposals, batches and jobs',
    views: [
      { type: 'proposal', title: 'Proposals', state: {} },
      { type: 'batch_log', title: 'Batch log', state: {} },
      { type: 'job_history', title: 'Jobs', state: {} },
    ],
    layout: (ids) => splitPair('horizontal', 0.62, tabGroup([ids[0]]), tabGroup(ids.slice(1))),
  },
  ]
}

export async function createWorkspaceFromTemplate(template: WorkspaceTemplate, options?: { shareToken?: string }) {
  const workspaceId = crypto.randomUUID()
  const viewIds = template.views.map(() => crypto.randomUUID())
  await api.post('/v1/workspaces', { body: {
      id: workspaceId,
      name: template.name,
      locked: false,
      share_token: options?.shareToken ?? `user-${workspaceId.slice(0, 8)}`,
      layout: template.layout(viewIds),
    } })
  for (const [index, view] of template.views.entries()) {
    await api.post('/v1/views', { body: {
        id: viewIds[index],
        workspace_id: workspaceId,
        type: view.type,
        title: view.title,
        pane: index === 0 ? 'left' : 'main',
        position: index,
        state: view.state ?? {},
      } })
  }
  return workspaceId
}
