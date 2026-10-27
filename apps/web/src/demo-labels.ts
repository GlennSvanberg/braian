import { copy } from './i18n'

/** Demo workspaces keep Swedish names in older databases until seed runs again. */
const LEGACY_WORKSPACE_NAMES: Record<string, string> = {
  'Datamodell': copy.dataModel,
  'Berika objekt': copy.enrichObjects,
  'Enrich objects': copy.enrichObjects,
  Granska: copy.reviewWorkspace,
}

const WORKSPACE_BY_SHARE: Record<string, string> = {
  'braian-model': copy.dataModel,
  'braian-enrich': copy.enrichObjects,
  'braian-review': copy.reviewWorkspace,
}

const LEGACY_VIEW_TITLES: Record<string, string> = {
  Struktur: copy.structure,
  Klass: copy.classTab,
  Konsekvens: copy.impactTab,
  Sortiment: copy.structure,
  Assortment: copy.structure,
  Objekt: copy.objects,
  Detalj: copy.detail,
  Graf: copy.graph,
  Historik: copy.history,
  Förslag: copy.proposals,
  Batchlogg: copy.batches,
  Jobb: copy.jobs,
  Externt: copy.external,
}

export function workspaceDisplayName(name: string, shareToken?: string | null): string {
  if (LEGACY_WORKSPACE_NAMES[name]) return LEGACY_WORKSPACE_NAMES[name]
  if (shareToken && WORKSPACE_BY_SHARE[shareToken]) {
    const canonical = WORKSPACE_BY_SHARE[shareToken]
    if (!name || name === canonical) return canonical
  }
  return name
}

export function viewDisplayTitle(title: string): string {
  return LEGACY_VIEW_TITLES[title] ?? title
}

const LEGACY_JOB_TYPE_NAMES: Record<string, string> = {
  Statuskontroll: copy.statusCheck,
  Masskrivning: copy.bulkWrite,
  Massinläsning: copy.bulkWrite,
}

export function assortmentDisplayName(name: string): string {
  return name
}

export function jobTypeDisplayTitle(title: string): string {
  return LEGACY_JOB_TYPE_NAMES[title] ?? title
}
