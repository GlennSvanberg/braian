import type { Json } from './api'

/** Palette progress lines. The wording is the Ctrl+K spec. */
export function stepText(op: string, args: Json = {}): string {
  const query = text(args.q ?? args.text ?? args.filter)
  switch (op) {
    case 'read':
      return 'Läser frågan…'
    case 'search':
    case 'list_objects':
      return query ? `Filtrerar på '${query}'…` : 'Öppnar en lista med artiklar…'
    case 'ui.filter':
      return `Filtrerar på '${query}'…`
    case 'ui.group':
      return 'Grupperar listan…'
    case 'ui.sort':
      return 'Sorterar listan…'
    case 'ui.open_workspace':
    case 'ui.open_template':
      return 'Öppnar arbetsytan…'
    case 'ui.open_view':
    case 'ui.place':
      return 'Öppnar vyn…'
    case 'ui.select':
    case 'get_object':
      return 'Öppnar objektet…'
    case 'ui.select_rows':
      return 'Markerar raderna…'
    case 'ui.show_column':
    case 'ui.hide_column':
    case 'ui.move_column':
      return 'Ändrar kolumnerna…'
    case 'ui.set_theme':
      return 'Byter tema…'
    case 'ui.export':
    case 'export':
      return 'Exporterar det som syns…'
    case 'patch_object':
    case 'create_object':
    case 'mass_edit':
    case 'set_relation':
    case 'rollback':
      return 'Förbereder ett förslag…'
    default:
      return `Kör ${op}…`
  }
}

function text(value: unknown) {
  return typeof value === 'string' ? value : ''
}
