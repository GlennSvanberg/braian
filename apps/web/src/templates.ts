import { api, type Json } from './api'
import { splitPair, tabGroup, type Node } from './layout'

export type WorkspaceTemplate = {
  id: string
  name: string
  summary: string
  views: Array<{ type: string; title: string; state?: Json }>
  layout: (ids: string[]) => Node
}

export const workspaceTemplates: WorkspaceTemplate[] = [
  {
    id: 'enrich',
    name: 'Enrich objects',
    summary: 'Tree, table and detail',
    views: [
      { type: 'structure_tree', title: 'Assortment', state: { expanded: ['friluft', 'bygg'] } },
      { type: 'object_table', title: 'Objects', state: {} },
      { type: 'object_detail', title: 'Detail', state: { language: 'sv' } },
      { type: 'graph', title: 'Graph', state: {} },
      { type: 'history', title: 'History', state: {} },
    ],
    layout: (ids) => splitPair('horizontal', 0.22, tabGroup([ids[0]]), splitPair('horizontal', 0.62, tabGroup([ids[1]]), tabGroup(ids.slice(2)))),
  },
  {
    id: 'model',
    name: 'Data model',
    summary: 'Structure, classes and the class card',
    views: [
      { type: 'data_model', title: 'Structure', state: { mode: 'tree' } },
      { type: 'data_model', title: 'Classes', state: { mode: 'classes' } },
      { type: 'data_model', title: 'Class', state: { mode: 'card' } },
    ],
    layout: (ids) => splitPair('horizontal', 0.28, tabGroup([ids[0]]), splitPair('horizontal', 0.46, tabGroup([ids[1]]), tabGroup([ids[2]]))),
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

export async function createWorkspaceFromTemplate(template: WorkspaceTemplate, ownerName?: string) {
  const workspaceId = crypto.randomUUID()
  const viewIds = template.views.map(() => crypto.randomUUID())
  const name = ownerName ? `${template.name}` : template.name
  await api('/v1/workspaces', {
    method: 'POST',
    body: JSON.stringify({
      id: workspaceId,
      name,
      locked: false,
      share_token: `user-${workspaceId.slice(0, 8)}`,
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
