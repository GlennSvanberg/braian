import { copy } from './i18n'
import { chordForCommand } from './shortcuts'

/** Surfaces that can open a context menu. */
export type MenuContext =
  | 'empty'
  | 'table-header'
  | 'row'
  | 'cell'
  | 'object-detail'
  | 'group'
  | 'model'
  | 'model-type'
  | 'model-link'
  | 'model-list'
  | 'model-option'
  | 'model-family'
  | 'model-unit'
  | 'assortment'
  | 'attribute'
  | 'dock-view'
  | 'graph-node'
  | 'tab'
  | 'link'
  | 'import'

export type CommandTarget = {
  objectId?: string
  column?: string
  columnHeader?: string
  typeKey?: string
  typeLabel?: string
  structureKey?: string
  groupKey?: string
  proposalId?: string
  viewId?: string
  linkId?: string
  peerId?: string
  attributeKey?: string
  listKey?: string
  optionCode?: string
  familyKey?: string
  unitCode?: string
  workspaceId?: string
  feedbackEnabled?: boolean
  hiddenColumns?: Array<{ id: string; header: string }>
  singular?: string
  standard?: boolean
  locked?: boolean
  [key: string]: unknown
}

export type CommandGroup = 'recent' | 'best' | 'object' | 'views' | 'search' | 'ask'

export type RegistryCommand = {
  id: string
  title: string | ((target: CommandTarget) => string)
  contexts: MenuContext[]
  group?: CommandGroup
  keywords?: string[]
  kbd?: string
  danger?: boolean
  palette?: boolean
  when?: (target: CommandTarget) => boolean
}

type Runner = (target: CommandTarget) => void | Promise<void>

const runners = new Map<string, Runner>()

export function registerCommandRunner(id: string, run: Runner): () => void {
  runners.set(id, run)
  return () => {
    if (runners.get(id) === run) runners.delete(id)
  }
}

export function runCommand(id: string, target: CommandTarget = {}): void | Promise<void> {
  const run = runners.get(id)
  if (!run) {
    if (typeof window !== 'undefined') {
      window.dispatchEvent(new CustomEvent('braian:command', { detail: { id, target } }))
    }
    return
  }
  return run(target)
}

export function commandTitle(command: RegistryCommand, target: CommandTarget = {}): string {
  return typeof command.title === 'function' ? command.title(target) : command.title
}

export const registryCommands: RegistryCommand[] = [
  {
    id: 'palette:search',
    title: `${copy.search}…`,
    contexts: ['empty', 'object-detail', 'import', 'dock-view', 'tab'],
    group: 'views',
    keywords: ['search', 'command', 'palette'],
    kbd: chordForCommand('palette:search'),
  },
  {
    id: 'feedback:send',
    title: copy.sendFeedback,
    contexts: ['empty', 'object-detail', 'import', 'dock-view', 'tab'],
    group: 'views',
    keywords: ['feedback', 'bug'],
    kbd: chordForCommand('feedback:send'),
    when: (target) => Boolean(target.feedbackEnabled),
  },
  {
    id: 'object:new',
    title: (target) => (target.singular ? `New ${target.singular}` : copy.newObject),
    contexts: ['empty', 'row', 'cell', 'table-header', 'object-detail'],
    group: 'object',
    keywords: ['create', 'new', 'object'],
    when: (target) => Boolean(target.typeKey || target.singular),
  },
  {
    id: 'object:open',
    title: (target) => (target.singular ? `Open ${target.singular}` : copy.open),
    contexts: ['row', 'cell', 'link', 'group', 'graph-node'],
    group: 'object',
    keywords: ['open'],
    kbd: 'Enter',
    when: (target) => Boolean(target.objectId || target.groupKey || target.peerId),
  },
  {
    id: 'object:right',
    title: copy.placeRight,
    contexts: ['row', 'cell'],
    group: 'object',
    when: (target) => Boolean(target.objectId),
  },
  {
    id: 'object:history',
    title: copy.history,
    contexts: ['row', 'cell', 'object-detail'],
    group: 'object',
    when: (target) => Boolean(target.objectId),
  },
  {
    id: 'object:move-group',
    title: copy.moveToGroup,
    contexts: ['row', 'cell'],
    group: 'object',
    keywords: ['move', 'group'],
    when: (target) => Boolean(target.objectId),
  },
  {
    id: 'columns:hide',
    title: copy.hideColumn,
    contexts: ['table-header', 'cell', 'row'],
    group: 'views',
    keywords: ['column', 'hide'],
    when: (target) => Boolean(target.column),
  },
  {
    id: 'fields:select',
    title: `${copy.selectFields}…`,
    contexts: ['table-header', 'cell', 'row', 'empty'],
    group: 'views',
    keywords: ['field', 'column', 'select', 'show', 'hide', 'reorder'],
  },
  {
    id: 'fields:edit',
    title: 'Edit field…',
    contexts: ['table-header', 'cell'],
    group: 'views',
    keywords: ['field', 'column', 'edit', 'qualifier'],
    when: (target) => Boolean(target.column),
  },
  {
    id: 'selection:new',
    title: copy.newSelection,
    contexts: ['assortment', 'dock-view', 'empty'],
    group: 'views',
    keywords: ['assortment', 'selection'],
    when: (target) => !target.viewType || target.viewType === 'assortment_list',
  },
  {
    id: 'model:new-type',
    title: copy.newType,
    contexts: ['model', 'model-type'],
    group: 'object',
    keywords: ['create', 'type'],
  },
  {
    id: 'model:new-link-type',
    title: copy.newLinkType,
    contexts: ['model', 'model-link'],
    group: 'object',
    keywords: ['create', 'link', 'type'],
  },
  {
    id: 'model:delete-type',
    title: copy.deleteMenu,
    contexts: ['model-type'],
    group: 'object',
    danger: true,
    when: (target) => Boolean(target.typeKey),
  },
  {
    id: 'model:delete-link-type',
    title: copy.deleteMenu,
    contexts: ['model-link'],
    group: 'object',
    danger: true,
    when: (target) => Boolean(target.typeKey),
  },
  {
    id: 'model:new-list',
    title: copy.newValueList,
    contexts: ['model', 'model-list'],
    group: 'object',
  },
  {
    id: 'model:delete-list',
    title: copy.deleteValueList,
    contexts: ['model-list'],
    group: 'object',
    danger: true,
    when: (target) => Boolean(target.listKey),
  },
  {
    id: 'model:delete-option',
    title: copy.deleteOption,
    contexts: ['model-option'],
    group: 'object',
    danger: true,
    when: (target) => Boolean(target.listKey && target.optionCode),
  },
  {
    id: 'model:new-family',
    title: copy.newUnitFamily,
    contexts: ['model', 'model-family'],
    group: 'object',
  },
  {
    id: 'model:delete-family',
    title: copy.deleteUnitFamily,
    contexts: ['model-family'],
    group: 'object',
    danger: true,
    when: (target) => Boolean(target.familyKey),
  },
  {
    id: 'model:delete-unit',
    title: copy.deleteUnit,
    contexts: ['model-unit'],
    group: 'object',
    danger: true,
    when: (target) => Boolean(target.familyKey && target.unitCode),
  },
  {
    id: 'group:rename',
    title: copy.rename,
    contexts: ['group'],
    group: 'object',
    kbd: 'F2',
    when: (target) => Boolean(target.groupKey),
  },
  {
    id: 'group:rename-key',
    title: copy.renameKey,
    contexts: ['group'],
    group: 'object',
    when: (target) => Boolean(target.groupKey),
  },
  {
    id: 'group:add-child',
    title: copy.addChild,
    contexts: ['group'],
    group: 'object',
    kbd: 'Ins',
    when: (target) => Boolean(target.groupKey),
  },
  {
    id: 'group:add-sibling',
    title: copy.addSibling,
    contexts: ['group'],
    group: 'object',
    when: (target) => Boolean(target.groupKey),
  },
  {
    id: 'group:duplicate',
    title: copy.duplicate,
    contexts: ['group'],
    group: 'object',
    when: (target) => Boolean(target.groupKey),
  },
  {
    id: 'group:delete',
    title: copy.deleteMenu,
    contexts: ['group'],
    group: 'object',
    danger: true,
    kbd: 'Del',
    when: (target) => Boolean(target.groupKey),
  },
  {
    id: 'attr:add-type',
    title: copy.addToType,
    contexts: ['attribute'],
    group: 'object',
    when: (target) => Boolean(target.attributeKey),
  },
  {
    id: 'attr:add-group',
    title: copy.addToGroup,
    contexts: ['attribute'],
    group: 'object',
    when: (target) => Boolean(target.attributeKey),
  },
  {
    id: 'attr:rename',
    title: copy.renameAttribute,
    contexts: ['attribute'],
    group: 'object',
    when: (target) => Boolean(target.attributeKey),
  },
  {
    id: 'attr:rename-key',
    title: copy.renameKey,
    contexts: ['attribute'],
    group: 'object',
    when: (target) => Boolean(target.attributeKey) && !target.standard,
  },
  {
    id: 'attr:delete',
    title: copy.deleteMenu,
    contexts: ['attribute'],
    group: 'object',
    danger: true,
    when: (target) => Boolean(target.attributeKey) && !target.standard,
  },
  {
    id: 'attr:new',
    title: copy.newAttribute,
    contexts: ['attribute'],
    group: 'object',
  },
  {
    id: 'link:open-peer',
    title: copy.open,
    contexts: ['link'],
    group: 'object',
    when: (target) => Boolean(target.peerId),
  },
  {
    id: 'link:edit',
    title: copy.editLink,
    contexts: ['link'],
    group: 'object',
    when: (target) => Boolean(target.linkId),
  },
  {
    id: 'link:remove',
    title: copy.removeLink,
    contexts: ['link'],
    group: 'object',
    danger: true,
    when: (target) => Boolean(target.linkId),
  },
  {
    id: 'layout:split-left',
    title: copy.splitLeft,
    contexts: ['dock-view'],
    group: 'views',
    palette: false,
  },
  {
    id: 'layout:split-right',
    title: copy.splitRight,
    contexts: ['dock-view'],
    group: 'views',
    palette: false,
  },
  {
    id: 'layout:split-top',
    title: copy.splitTop,
    contexts: ['dock-view'],
    group: 'views',
    palette: false,
  },
  {
    id: 'layout:split-bottom',
    title: copy.splitBottom,
    contexts: ['dock-view'],
    group: 'views',
    palette: false,
  },
  {
    id: 'layout:move_view_left',
    title: copy.moveLeft,
    contexts: ['dock-view'],
    group: 'views',
    kbd: chordForCommand('layout:move_view_left'),
  },
  {
    id: 'layout:move_view_right',
    title: copy.moveRight,
    contexts: ['dock-view'],
    group: 'views',
    kbd: chordForCommand('layout:move_view_right'),
  },
  {
    id: 'layout:move_view_up',
    title: copy.moveUp,
    contexts: ['dock-view'],
    group: 'views',
    kbd: chordForCommand('layout:move_view_up'),
  },
  {
    id: 'layout:move_view_down',
    title: copy.moveDown,
    contexts: ['dock-view'],
    group: 'views',
    kbd: chordForCommand('layout:move_view_down'),
  },
  {
    id: 'layout:zoom',
    title: copy.maximize,
    contexts: ['dock-view'],
    group: 'views',
    kbd: chordForCommand('layout:zoom'),
  },
  {
    id: 'layout:close_view',
    title: copy.closeView,
    contexts: ['dock-view'],
    group: 'views',
    kbd: chordForCommand('layout:close_view'),
  },
  {
    id: 'layout:add-view',
    title: copy.addView,
    contexts: ['dock-view'],
    group: 'views',
    // No chord: Ctrl+K is palette:search. Open Add view from the palette / dock menu.
  },
  {
    id: 'workspace:new',
    title: copy.newWorkspace,
    contexts: ['tab'],
    group: 'views',
  },
  {
    id: 'workspace:copy-share',
    title: copy.copyShare,
    contexts: ['tab'],
    group: 'views',
    when: (target) => Boolean(target.workspaceId),
  },
  {
    id: 'workspace:toggle-lock',
    title: (target) => (target.locked ? copy.unlockWorkspace : copy.lockWorkspace),
    contexts: ['tab'],
    group: 'views',
    when: (target) => Boolean(target.workspaceId),
  },
  {
    id: 'layout:reset_layout',
    title: copy.resetLayout,
    contexts: ['tab', 'empty'],
    group: 'views',
  },
  {
    id: 'table:mass-edit',
    title: copy.massEdit,
    contexts: ['row', 'cell', 'table-header', 'empty'],
    group: 'views',
    keywords: ['status', 'ok', 'mass'],
  },
  {
    id: 'table:filter-by',
    title: copy.filterBy,
    contexts: ['row', 'cell', 'table-header', 'empty'],
    group: 'views',
    keywords: ['filter'],
  },
  {
    id: 'table:export',
    title: copy.export,
    contexts: ['row', 'cell', 'table-header', 'empty'],
    group: 'views',
  },
  {
    id: 'table:export-columns',
    title: copy.exportColumns,
    contexts: ['row', 'cell', 'table-header', 'empty'],
    group: 'views',
  },
  {
    id: 'table:select',
    title: copy.selectAll,
    contexts: ['row', 'cell'],
    group: 'object',
    when: (target) => Boolean(target.objectId),
    palette: false,
  },
]

export function commandsForContext(context: MenuContext, target: CommandTarget = {}): RegistryCommand[] {
  return registryCommands.filter((command) => {
    if (!command.contexts.includes(context)) return false
    if (command.when && !command.when(target)) return false
    return true
  })
}

export function paletteCommandsFromRegistry(target: CommandTarget = {}): RegistryCommand[] {
  return registryCommands.filter((command) => {
    if (command.palette === false) return false
    if (command.when && !command.when(target)) return false
    return true
  })
}
