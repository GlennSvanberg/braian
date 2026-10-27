import type { PaletteItem } from './palette-model'

export type ModelHint = { title: string; subtitle: string }

const review = 'Proposal · review before it is saved'

/** Palette hint. The server `interpret` is the source of truth for the draft. */
export function modelCommandTitle(query: string): ModelHint | null {
  const text = query.trim()
  if (!text) return null
  const add = text.match(/^add attribute\s+(.+?)\s+to\s+(.+)$/i)
  if (add) return { title: `Add attribute ${add[1]} to ${add[2]}`, subtitle: review }
  const remove = text.match(/^remove attribute\s+(.+?)\s+from\s+(.+)$/i)
  if (remove) return { title: `Remove attribute ${remove[1]} from ${remove[2]}`, subtitle: review }
  const group = text.match(/^(add|rename|move|duplicate|delete) group\s+(.+)$/i)
  if (group) {
    const verb = group[1].toLowerCase()
    const title = verb === 'add'
      ? 'Add group'
      : verb === 'rename'
        ? 'Rename group'
        : verb === 'move'
          ? 'Move group'
          : verb === 'duplicate'
            ? 'Duplicate group'
            : 'Delete group'
    return { title: `${title} ${group[2]}`, subtitle: review }
  }
  const move = text.match(/^move(?:\s+class)?\s+(.+?)\s+to\s+(.+)$/i)
  if (move) return { title: `Move ${move[1]} to ${move[2]}`, subtitle: review }
  return null
}

export function assortmentSaveTitle(query: string): { title: string; name: string } | null {
  const match = query.trim().match(/^save (?:assortment|selection)\s+(.+)$/i)
  const name = match?.[1]?.trim() ?? ''
  if (!name) return null
  return { title: `Save selection ${name}`, name }
}

export function withModelCommand(query: string, rows: PaletteItem[]): PaletteItem[] {
  const save = assortmentSaveTitle(query)
  const model = modelCommandTitle(query)
  const hint = save
    ? { id: 'best:assortment:save', sourceId: 'assortment:save', title: save.title, subtitle: review }
    : model
      ? { id: 'best:model:command', sourceId: 'model:command', title: model.title, subtitle: review }
      : null
  if (!hint) return rows
  const command: PaletteItem = {
    id: hint.id,
    sourceId: hint.sourceId,
    group: 'best',
    title: hint.title,
    subtitle: hint.subtitle,
  }
  const rest = rows.filter((row) => row.id !== command.id && row.sourceId !== command.sourceId)
  const recentCount = rest.filter((row) => row.group === 'recent').length
  rest.splice(recentCount, 0, command)
  return rest
}
