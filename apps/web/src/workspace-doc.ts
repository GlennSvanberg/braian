import { migrateLayout, views, type LayoutDoc, type Node } from './layout'

/** One layout document per person and environment. The web client uses `default`. */
export const LAYOUT_ENVIRONMENT = 'default'

export type StoredLayout = { document: LayoutDoc; savedAt: number }

export type ServerLayout = {
  document?: LayoutDoc | { [key: string]: unknown } | null
  updated_at?: string | null
}

export function layoutStorageKey(userId: string, workspaceId: string) {
  return `braian.layout.${userId}.${LAYOUT_ENVIRONMENT}.${workspaceId}`
}

export function readLayoutStore(key: string): StoredLayout | null {
  if (typeof localStorage === 'undefined') return null
  try {
    const raw = localStorage.getItem(key)
    if (!raw) return null
    const parsed = JSON.parse(raw) as StoredLayout
    if (!parsed?.document?.layout || typeof parsed.savedAt !== 'number') return null
    return parsed
  } catch {
    return null
  }
}

export function writeLayoutStore(key: string, document: LayoutDoc) {
  if (typeof localStorage === 'undefined') return
  const stored: StoredLayout = { document, savedAt: Date.now() }
  localStorage.setItem(key, JSON.stringify(stored))
}

export function canonicalDoc(workspace: { id: string; name: string; layout: unknown; owner_id?: string }): LayoutDoc {
  const layout = migrateLayout(workspace.layout)
  const open = views(layout)
  return {
    id: workspace.id,
    title: workspace.name,
    layout,
    focus: open[0],
    createdBy: workspace.owner_id,
  }
}

export function migrateDoc(document: LayoutDoc | { [key: string]: unknown }): LayoutDoc {
  const id = typeof document.id === 'string' ? document.id : ''
  const title = typeof document.title === 'string' ? document.title : ''
  const layout = migrateLayout(document.layout)
  const open = views(layout)
  const focus = typeof document.focus === 'string' && open.includes(document.focus) ? document.focus : open[0]
  const zoom = typeof document.zoom === 'string' && open.includes(document.zoom) ? document.zoom : undefined
  const createdBy = typeof document.createdBy === 'string' ? document.createdBy : undefined
  return { id, title, layout, focus, zoom, ...(createdBy ? { createdBy } : {}) }
}

/** Paint local first. A newer server document replaces it; otherwise the canonical workspace layout is the default. */
export function reconcileLayout(local: StoredLayout | null, server: ServerLayout | null, canonical: LayoutDoc): LayoutDoc {
  const serverAt = server?.updated_at ? Date.parse(server.updated_at) : 0
  const localAt = local?.savedAt ?? 0
  if (local && localAt > 0 && localAt >= serverAt) return migrateDoc(local.document)
  if (server?.document) return migrateDoc(server.document)
  return canonical
}

/** A locked or shared workspace stays as other people see it. Edits are stored as that person's copy. */
export function editsArePersonal(workspace: { locked: boolean; share_token?: string | null }) {
  return workspace.locked || Boolean(workspace.share_token)
}

export function layoutChanged(left: Node, right: Node) {
  return JSON.stringify(left) !== JSON.stringify(right)
}
