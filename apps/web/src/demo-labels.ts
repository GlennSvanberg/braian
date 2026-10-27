import { copy } from './i18n'

const WORKSPACE_BY_SHARE: Record<string, string> = {
  'braian-model': copy.dataModel,
  'braian-enrich': copy.enrichObjects,
  'braian-review': copy.reviewWorkspace,
}

export function workspaceDisplayName(name: string, shareToken?: string | null): string {
  if (shareToken && WORKSPACE_BY_SHARE[shareToken]) {
    const canonical = WORKSPACE_BY_SHARE[shareToken]
    if (!name || name === canonical) return canonical
  }
  return name
}

export function viewDisplayTitle(title: string): string {
  return title
}

export function assortmentDisplayName(name: string): string {
  return name
}

export function jobTypeDisplayTitle(title: string): string {
  return title
}
