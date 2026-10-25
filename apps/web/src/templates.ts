import { api, type Json } from './api'
import { splitPair, tabGroup, type Node } from './layout'
import type { TypeName } from './type-labels'

export type WorkspaceTemplate = {
  id: string
  name: string
  summary: string
  views: Array<{ type: string; title: string; state?: Json }>
  layout: (ids: string[]) => Node
}

const TABLE_STATE = {
  columns: ['nummer', 'namn', 'type', 'status', 'farg', 'vikt', 'langd'],
  sort: 'nummer',
}

export function typeWorkspaceTemplate(type: TypeName): WorkspaceTemplate {
  return {
    id: `type:${type.key}`,
    name: type.plural,
    summary: 'Assortments, structure, list, detail, graph, history and classes',
    views: [
      { type: 'structure_tree', title: 'eCom', state: { expanded: ['friluft', 'bygg'] } },
      { type: 'assortment_list', title: 'Assortments', state: {} },
      {
        type: 'object_table',
        title: type.plural,
        state: { ...TABLE_STATE, object_type: type.id, type_key: type.key },
      },
      { type: 'object_detail', title: 'Detail', state: { language: 'sv' } },
      { type: 'graph', title: 'Graph', state: {} },
      { type: 'history', title: 'History', state: {} },
      { type: 'class_attributes', title: 'Classes', state: {} },
    ],
    layout: (ids) =>
      splitPair(
        'horizontal',
        0.22,
        tabGroup([ids[0] ?? '', ids[1] ?? ''].filter(Boolean)),
        splitPair('horizontal', 0.62, tabGroup([ids[2] ?? ''].filter(Boolean)), tabGroup(ids.slice(3))),
      ),
  }
}

export function workspaceTemplates(types: TypeName[] = []): WorkspaceTemplate[] {
  return [
  ...types.filter((type) => type.id && type.key).map(typeWorkspaceTemplate),
  {
    id: 'model',
    name: 'Data model',
    summary: 'Structure, classes and the class card',
    views: [
      { type: 'data_model', title: 'Model', state: { mode: 'tree' } },
      { type: 'data_model', title: 'Attributes', state: { mode: 'classes' } },
      { type: 'data_model', title: 'Preview', state: { mode: 'card' } },
    ],
    layout: (ids) => splitPair('horizontal', 0.22, tabGroup([ids[0]]), splitPair('horizontal', 0.58, tabGroup([ids[1]]), tabGroup([ids[2]]))),
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
  {
    id: 'overview',
    name: 'Overview',
    summary: 'Dashboard and the setup list',
    views: [
      { type: 'dashboard', title: 'Overview', state: {} },
      { type: 'data_model', title: 'To do', state: { checklist: ['languages', 'units', 'types', 'import'] } },
    ],
    layout: (ids) => splitPair('horizontal', 0.5, tabGroup([ids[0]]), tabGroup([ids[1]])),
  },
  ]
}

export async function createWorkspaceFromTemplate(template: WorkspaceTemplate, options?: { shareToken?: string }) {
  const workspaceId = crypto.randomUUID()
  const viewIds = template.views.map(() => crypto.randomUUID())
  await api('/v1/workspaces', {
    method: 'POST',
    body: JSON.stringify({
      id: workspaceId,
      name: template.name,
      locked: false,
      share_token: options?.shareToken ?? `user-${workspaceId.slice(0, 8)}`,
      layout: template.layout(viewIds),
    }),
  })
  for (const [index, view] of template.views.entries()) {
    await api('/v1/views', {
      method: 'POST',
      body: JSON.stringify({
        id: viewIds[index],
        workspace_id: workspaceId,
        type: view.type,
        title: view.title,
        pane: index === 0 ? 'left' : 'main',
        position: index,
        state: view.state ?? {},
      }),
    })
  }
  return workspaceId
}
