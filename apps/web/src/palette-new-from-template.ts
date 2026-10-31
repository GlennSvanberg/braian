import { copy } from './i18n'
import type { PaletteItem } from './palette-model'
import { canonicalShareToken, workspaceTemplates, type WorkspaceTemplate } from './templates'
import type { TypeName } from './type-labels'

export type ListedTemplateWorkspace = { id: string; name: string; share_token?: string | null }

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
export function newFromTemplateSubmenuRows(
  types: TypeName[],
  workspaces: ListedTemplateWorkspace[] = [],
): PaletteItem[] {
  return workspaceTemplates(types).map((template) => templatePaletteItem(template, workspaces))
}

export function templatePaletteItem(
  template: WorkspaceTemplate,
  workspaces: ListedTemplateWorkspace[] = [],
): PaletteItem {
  const token = canonicalShareToken(template)
  const open = token ? workspaces.some((row) => row.share_token === token) : false
  const summary = template.summary
  return {
    id: `template:${template.id}`,
    group: 'views',
    title: template.name,
    subtitle: open ? `Open existing · ${summary}` : `Create · ${summary}`,
    keywords: ['template', template.id, template.name, open ? 'open' : 'create'],
  }
}

export function isNewFromTemplateRoot(id: string) {
  return id === 'workspace:new'
}
