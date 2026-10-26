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

/** Supplier sits beside the Food tree and is linked with object references. */
export function typeUsesTree(key: string) {
  return key !== 'supplier'
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
