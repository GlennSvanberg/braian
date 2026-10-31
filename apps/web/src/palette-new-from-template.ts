import { copy } from './i18n'
import type { PaletteItem } from './palette-model'
import { workspaceTemplates, type WorkspaceTemplate } from './templates'
import type { TypeName } from './type-labels'

/** Palette root row that opens the New from template submenu. */
export function newFromTemplateRootItem(): PaletteItem {
  return {
    id: 'workspace:new',
    group: 'views',
    title: `${copy.newWorkspace} ›`,
    subtitle: 'Create or open a workspace from a template',
    keywords: ['new', 'template', 'workspace', 'tab', 'create'],
  }
}

/** Rows listed under "New from template ›". */
export function newFromTemplateSubmenuRows(types: TypeName[]): PaletteItem[] {
  return workspaceTemplates(types).map((template) => templatePaletteItem(template))
}

export function templatePaletteItem(template: WorkspaceTemplate): PaletteItem {
  return {
    id: `template:${template.id}`,
    group: 'views',
    title: template.name,
    subtitle: `Template · ${template.summary}`,
    keywords: ['template', template.id, template.name],
  }
}

export function isNewFromTemplateRoot(id: string) {
  return id === 'workspace:new'
}
