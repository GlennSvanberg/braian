import { copy } from './i18n'
import { displayViewTitle, type TypeName } from './type-labels'
import type { PaletteItem } from './palette-model'
import type { Json } from './api'

export type AddableView = {
  id: string
  title: string
  type: string
  state?: Json
}

/** Palette root row that opens the add-view submenu. */
export function addViewRootItem(): PaletteItem {
  return {
    id: 'layout:add-view',
    group: 'views',
    title: `${copy.addView} ›`,
    subtitle: 'Add a view to the focused pane',
    keywords: ['add', 'view', 'pane', 'tab', 'layout'],
  }
}

/** Rows listed under "Add view ›" for the focused pane. */
export function addViewSubmenuRows(
  views: AddableView[],
  types: TypeName[],
  openTypeKey = '',
): PaletteItem[] {
  const fromViews = views.map((view) => ({
    id: `add-view:view:${view.id}`,
    group: 'views' as const,
    title: displayViewTitle(view, types, openTypeKey),
    subtitle: copy.addView,
    keywords: [view.type, view.title],
  }))
  const fromTypes = types
    .filter((type) => type.id)
    .map((type) => ({
      id: `add-view:type:${type.id}`,
      group: 'views' as const,
      title: `${type.plural} view`,
      subtitle: copy.addView,
      keywords: [type.key, type.singular, type.plural],
    }))
  return [...fromViews, ...fromTypes]
}

export function isAddViewRoot(id: string) {
  return id === 'layout:add-view'
}

export function parseAddViewChoice(id: string): { kind: 'view' | 'type'; value: string } | null {
  if (id.startsWith('add-view:view:')) return { kind: 'view', value: id.slice('add-view:view:'.length) }
  if (id.startsWith('add-view:type:')) return { kind: 'type', value: id.slice('add-view:type:'.length) }
  return null
}
