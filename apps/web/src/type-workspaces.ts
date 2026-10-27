import type { TypeName } from './type-labels'

const TYPE_SHARE_PREFIX = 'braian-type-'

export function typeShareToken(key: string) {
  const slug = key
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-|-$/g, '')
  return `${TYPE_SHARE_PREFIX}${slug || 'type'}`
}

export function isTypeWorkspace(shareToken?: string | null) {
  return Boolean(shareToken?.startsWith(TYPE_SHARE_PREFIX))
}

/** Type key encoded in a type workspace share token. Empty for other workspaces. */
export function typeKeyFromShare(shareToken?: string | null) {
  if (!shareToken?.startsWith(TYPE_SHARE_PREFIX)) return ''
  return shareToken.slice(TYPE_SHARE_PREFIX.length)
}

/** Setup, data model, one workspace per type, then everything else, then Review. */
export function orderWorkspaces<T extends { share_token?: string | null }>(rows: T[], types: TypeName[]): T[] {
  return rows
    .map((row, index) => ({ row, index, rank: workspaceRank(row.share_token, types) }))
    .sort((a, b) => a.rank - b.rank || a.index - b.index)
    .map((item) => item.row)
}

export function missingTypeWorkspaces(rows: Array<{ share_token?: string | null }>, types: TypeName[]) {
  const tokens = new Set(rows.map((row) => row.share_token).filter((token): token is string => Boolean(token)))
  return types.filter((type) => type.id && type.key && !tokens.has(typeShareToken(type.key)))
}

/** A type with no `structures` field still uses a tree. An empty list does not. */
export function typeUsesTree(type: { structures?: string[] | null }) {
  return type.structures == null || type.structures.length > 0
}

function workspaceRank(shareToken: string | null | undefined, types: TypeName[]) {
  if (shareToken === 'braian-model') return 1
  if (shareToken?.startsWith(TYPE_SHARE_PREFIX)) {
    const key = shareToken.slice(TYPE_SHARE_PREFIX.length)
    const index = types.findIndex((type) => typeShareToken(type.key) === shareToken || type.key === key)
    return 10 + (index === -1 ? 400 : index)
  }
  if (shareToken === 'braian-review') return 900
  return 800
}
