import { score, type FrecencyEntry } from './frecency'

export type PaletteGroup = 'recent' | 'best' | 'object' | 'views' | 'search' | 'ask'

export type PaletteChip = { text: string; tone: 'neutral' | 'ok' | 'warn' | 'danger' | 'info' }

export type PaletteItem = {
  id: string
  group: PaletteGroup
  title: string
  subtitle: string
  kbd?: string
  shortcut?: string
  sourceId?: string
  chips?: PaletteChip[]
}

export const groupTitle: Record<Exclude<PaletteGroup, 'ask'>, string> = {
  recent: 'Recent',
  best: 'Best match',
  object: 'On selected object',
  views: 'Views & workspaces',
  search: 'Objects',
}

export function askTitle(query: string) {
  const text = query.trim()
  return text ? `Ask AI: "${text}"` : 'Ask AI'
}

export function visibleRows(
  query: string,
  catalog: PaletteItem[],
  hits: PaletteItem[],
  frecency: Record<string, FrecencyEntry>,
  now = Date.now(),
): PaletteItem[] {
  const needle = query.trim().toLowerCase()
  const matches = (item: PaletteItem) => {
    if (!needle) return true
    return `${item.title} ${item.subtitle}`.toLowerCase().includes(needle)
  }
  const scored = [...catalog].sort((left, right) => score(frecency[right.id], now) - score(frecency[left.id], now))
  const recent = needle ? [] : scored.filter((item) => score(frecency[item.id], now) > 0).slice(0, 4)
  const recentIds = new Set(recent.map((item) => item.id))
  const matched = scored.filter((item) => matches(item) && !recentIds.has(item.id))
  const best = needle ? matched[0] : undefined
  const rest = matched.filter((item) => item.id !== best?.id)
  const rows: PaletteItem[] = [
    ...recent.map((item) => ({ ...item, group: 'recent' as const })),
    ...(best ? [{ ...best, id: `best:${best.id}`, sourceId: best.id, group: 'best' as const }] : []),
    ...rest.filter((item) => item.group === 'object'),
    ...rest.filter((item) => item.group === 'views'),
    ...hits.map((item) => ({ ...item, group: 'search' as const })),
    {
      id: 'ask',
      group: 'ask',
      title: askTitle(query),
      subtitle: '',
      kbd: 'Tab',
    },
  ]
  return rows
}

export function askIndex(rows: PaletteItem[]) {
  return rows.findIndex((row) => row.group === 'ask')
}
