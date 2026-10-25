import { copy } from './i18n'

/** Demo workspaces keep Swedish names in older databases until seed runs again. */
const LEGACY_WORKSPACE_NAMES: Record<string, string> = {
  'Sätt upp systemet': copy.setup,
  'Datamodell': copy.dataModel,
  'Berika objekt': copy.enrichObjects,
  Granska: copy.reviewWorkspace,
}

const WORKSPACE_BY_SHARE: Record<string, string> = {
  'braian-setup': copy.setup,
  'braian-model': copy.dataModel,
  'braian-enrich': copy.enrichObjects,
  'braian-review': copy.reviewWorkspace,
}

const LEGACY_VIEW_TITLES: Record<string, string> = {
  Översikt: copy.dashboard,
  'Att göra': copy.toDo,
  Struktur: copy.structure,
  Klass: copy.classTab,
  Konsekvens: copy.impactTab,
  Sortiment: copy.assortment,
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
  if (shareToken && WORKSPACE_BY_SHARE[shareToken]) return WORKSPACE_BY_SHARE[shareToken]
  return LEGACY_WORKSPACE_NAMES[name] ?? name
}

export function viewDisplayTitle(title: string): string {
  return LEGACY_VIEW_TITLES[title] ?? title
}

const LEGACY_ASSORTMENT_NAMES: Record<string, string> = {
  Tält: copy.assortmentTents,
  Virke: copy.assortmentTimber,
}

const LEGACY_JOB_TYPE_NAMES: Record<string, string> = {
  Statuskontroll: copy.statusCheck,
  Masskrivning: copy.bulkWrite,
  Massinläsning: copy.bulkWrite,
}

export function assortmentDisplayName(name: string): string {
  return LEGACY_ASSORTMENT_NAMES[name] ?? name
}

export function jobTypeDisplayTitle(title: string): string {
  return LEGACY_JOB_TYPE_NAMES[title] ?? title
}
