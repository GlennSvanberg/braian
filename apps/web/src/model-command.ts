import type { PaletteItem } from './palette-model'

export type ModelHint = { title: string; subtitle: string }

const review = 'Proposal · review before it is saved'

/** Palette hint. The server `interpret` is the source of truth for the draft. */
export function modelCommandTitle(query: string): ModelHint | null {
  const text = query.trim()
  if (!text) return null
  const lower = text.toLowerCase()
  if (
    lower.includes('power tool') &&
    (lower.includes('model') || lower.includes('set up') || lower.includes('setup') || lower.includes('draft'))
  ) {
    return { title: 'Draft a model for power tools', subtitle: review }
  }
  const add = text.match(/^add attribute\s+(.+?)\s+to\s+(.+)$/i)
  if (add) return { title: `Add attribute ${add[1]} to ${add[2]}`, subtitle: review }
  const remove = text.match(/^remove attribute\s+(.+?)\s+from\s+(.+)$/i)
  if (remove) return { title: `Remove attribute ${remove[1]} from ${remove[2]}`, subtitle: review }
  const move = text.match(/^move(?:\s+class)?\s+(.+?)\s+to\s+(.+)$/i)
  if (move) return { title: `Move ${move[1]} to ${move[2]}`, subtitle: review }
  return null
}

export function withModelCommand(query: string, rows: PaletteItem[]): PaletteItem[] {
  const hint = modelCommandTitle(query)
  if (!hint) return rows
  const command: PaletteItem = {
    id: 'best:model:command',
    sourceId: 'model:command',
    group: 'best',
    title: hint.title,
    subtitle: hint.subtitle,
  }
  const rest = rows.filter((row) => row.id !== command.id && row.sourceId !== command.sourceId)
  const recentCount = rest.filter((row) => row.group === 'recent').length
  rest.splice(recentCount, 0, command)
  return rest
}
