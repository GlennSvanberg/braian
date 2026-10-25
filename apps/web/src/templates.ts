import { api, type Json } from './api'

export type WorkspaceTemplate = {
  id: string
  name: string
  summary: string
  views: Array<{ type: string; title: string; state?: Json }>
  layout: (ids: string[]) => Json
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
    layout: (ids) => ({
      direction: 'row',
      ratio: 0.22,
      first: { tabs: [ids[0]], active: ids[0] },
      second: {
        direction: 'row',
        ratio: 0.62,
        first: { tabs: [ids[1]], active: ids[1] },
        second: { tabs: ids.slice(2), active: ids[2] },
      },
    }),
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
    layout: (ids) => ({
      direction: 'row',
      ratio: 0.28,
      first: { tabs: [ids[0]], active: ids[0] },
      second: {
        direction: 'row',
        ratio: 0.46,
        first: { tabs: [ids[1]], active: ids[1] },
        second: { tabs: [ids[2]], active: ids[2] },
      },
    }),
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
    layout: (ids) => ({
      direction: 'row',
      ratio: 0.62,
      first: { tabs: [ids[0]], active: ids[0] },
      second: { tabs: ids.slice(1), active: ids[1] },
    }),
  },
  {
    id: 'overview',
    name: 'Overview',
    summary: 'Dashboard and the setup list',
    views: [
      { type: 'dashboard', title: 'Overview', state: {} },
      { type: 'data_model', title: 'To do', state: { checklist: ['languages', 'units', 'types', 'import'] } },
    ],
    layout: (ids) => ({
      direction: 'row',
      ratio: 0.5,
      first: { tabs: [ids[0]], active: ids[0] },
      second: { tabs: [ids[1]], active: ids[1] },
    }),
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
