import { describe, expect, it } from 'vitest'
import {
  persistWorkspaceMigration,
  type PersistDeps,
  type PersistPatch,
  type PersistWorkspace,
} from './workspace-migration-persist'

function freeObject(value: object) {
  return { ...(value as Record<string, unknown>) }
}

/** Old order (layout before creates) — kept in the test to prove the regression. */
async function persistWorkspaceMigrationLayoutFirst(
  workspace: PersistWorkspace,
  patch: PersistPatch,
  deps: PersistDeps,
) {
  if ((patch.name && patch.name !== workspace.name) || patch.layout) {
    const layout = patch.layout ?? workspace.layout ?? { tabs: [] }
    await deps.postWorkspace({
      id: workspace.id,
      name: patch.name ?? workspace.name,
      locked: workspace.locked,
      share_token: workspace.share_token,
      layout: deps.freeObject(layout && typeof layout === 'object' ? (layout as object) : { tabs: [] }),
    })
  }
  for (const view of patch.create ?? []) {
    await deps.postView({
      id: view.id,
      workspace_id: workspace.id,
      type: view.type,
      title: view.title,
      pane: view.pane ?? 'main',
      position: view.position ?? 0,
      state: deps.freeObject(view.state),
    })
  }
  for (const view of patch.views) {
    const full = (workspace.views ?? []).find((item) => item.id === view.id)
    if (!full) continue
    await deps.postView({
      id: full.id,
      workspace_id: workspace.id,
      type: view.type ?? full.type,
      title: view.title,
      pane: full.pane ?? 'center',
      position: full.position ?? 0,
      state: deps.freeObject(view.state),
    })
  }
}


function workspace(): PersistWorkspace {
  return {
    id: 'ws-1',
    name: 'Heal dangling panes',
    locked: false,
    share_token: null,
    layout: { type: 'tabs', views: ['old'], active: 0 },
    views: [{ id: 'structures', type: 'data_model', title: 'Structures', pane: 'main', position: 0, state: { mode: 'tree' } }],
  }
}

function patch(): PersistPatch {
  return {
    views: [],
    create: [
      { id: 'ot', type: 'object_types', title: 'Object types', state: {}, pane: 'main', position: 1 },
      { id: 'lt', type: 'link_types', title: 'Link types', state: {}, pane: 'main', position: 2 },
    ],
    layout: {
      type: 'tabs',
      views: ['structures', 'ot', 'lt'],
      active: 0,
    },
  }
}

function trackingDeps(failOnCreateId?: string) {
  const views: string[] = []
  const workspaces: unknown[] = []
  const deps: PersistDeps = {
    freeObject,
    postView: async (body) => {
      const id = String(body.id)
      if (failOnCreateId && id === failOnCreateId) throw new Error(`create failed: ${id}`)
      views.push(id)
    },
    postWorkspace: async (body) => {
      workspaces.push(body.layout)
    },
  }
  return { deps, views, workspaces }
}

describe('persistWorkspaceMigration view-then-layout', () => {
  it('does not write layout when a view create fails, then succeeds on retry', async () => {
    const ws = workspace()
    const p = patch()

    const first = trackingDeps('lt')
    await expect(persistWorkspaceMigration(ws, p, first.deps)).rejects.toThrow('create failed: lt')
    expect(first.views).toEqual(['ot'])
    expect(first.workspaces, 'layout must not be saved after a failed create').toEqual([])

    // Retry after the failed create (migratedRef cleared by shell): all creates ok → layout written.
    const second = trackingDeps()
    await persistWorkspaceMigration(ws, p, second.deps)
    expect(second.views).toEqual(['ot', 'lt'])
    expect(second.workspaces).toHaveLength(1)
    expect(JSON.stringify(second.workspaces[0])).toContain('lt')
  })

  it('writes layout only after every create succeeds', async () => {
    const { deps, views, workspaces } = trackingDeps()
    await persistWorkspaceMigration(workspace(), patch(), deps)
    expect(views).toEqual(['ot', 'lt'])
    expect(workspaces).toHaveLength(1)
  })

  it('is red against the old layout-first order (layout saved even when create fails)', async () => {
    const { deps, views, workspaces } = trackingDeps('lt')
    await expect(persistWorkspaceMigrationLayoutFirst(workspace(), patch(), deps)).rejects.toThrow('create failed: lt')
    // Old bug: layout already written with the new ids before creates finished.
    expect(workspaces, 'old order incorrectly persists layout before creates').toHaveLength(1)
    expect(views).toEqual(['ot'])
  })
})
