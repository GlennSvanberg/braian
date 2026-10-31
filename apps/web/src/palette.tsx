import { useEffect, useMemo, useState, type KeyboardEvent as ReactKeyboardEvent , useLayoutEffect, useRef} from 'react'
import { useQuery } from '@tanstack/react-query'
import { Check } from 'lucide-react'
import { MetaChip, optionLabel, statusTone, typeLabel, type ChipTone } from './chips'
import { toast } from 'sonner'
import { ApiError, api, catalogName, operationId, textValue, type Json } from './api'
import { readIndexed } from './index-fields'
import { streamAgent, type AgentEvent } from './agent-stream'
import { renderAgentAnswer } from './agent-answer'
import { Command, CommandInput, CommandItem, CommandList } from './components/ui/command'
import { Kbd } from './components/ui/kbd'
import { emitSavedSelection, requestNewSelection, selectionMatchesType } from './assortment-filter'
import { workspaceDisplayName } from './demo-labels'
import { sourceLabel, suggestedTitle } from './field-diff'
import { copy, uiLanguage } from './i18n'
import { mergeFrecency, readFrecency, touch, writeFrecency } from './frecency'
import { withModelCommand } from './model-command'
import { askIndex, filterPaletteRows, visibleRows, groupTitle, type PaletteGroup, type PaletteItem } from './palette-model'
import { addViewRootItem, addViewSubmenuRows, isAddViewRoot, parseAddViewChoice } from './palette-add-view'
import { isNewFromTemplateRoot, newFromTemplateRootItem, newFromTemplateSubmenuRows } from './palette-new-from-template'
import { publishProposal } from './proposal-bus'
import { modelFocus, openModelDialog, requestDiscardDraft, requestReview, stageModel } from './model-stage'
import { isStandardAttribute, queueViewType, requestAttributeAction } from './attribute-actions'
import { stepText } from './steps'
import { createWorkspaceFromTemplate, workspaceTemplates } from './templates'
import {
  commandPlaceholder,
  displayViewTitle,
  openObjectLabel,
  searchGroupTitle,
  singularOf,
  typeNames,
  typeViewPaletteItems,
} from './type-labels'
import { applyUiResult, dispatchUi, uiOperations } from './ui-bus'
import { readUiContext, type UiColumn } from './ui-context'
import { runCommand } from './command-registry'
import { FieldsMode, type FieldsPaletteOpen } from './fields/fields-mode'
import { readFieldsHost, subscribeFieldsHost } from './fields-host'
import { makeViewColumn, parseColumnId } from './fields/columns'

type SessionLike = {
  objectId: string | null
  listType?: string | null
  workspaceType?: string | null
  group?: string | null
  setObjectId: (value: string | null) => void
  setProposalId: (value: string | null) => void
  push: (line: string) => void
  openWorkspace: (id: string) => void
  openTypeView?: (typeId: string) => void
}

type Turn = { role: 'user' | 'assistant'; text: string }
type Step = { id: string; text: string; state: 'run' | 'done' }
type ProposalPreview = {
  id: string
  title: string
  rows: Json[]
  lines: string[]
  source?: string
  actor_name?: unknown
  behalf_name?: unknown
  error?: string
}

export function CommandPalette({
  session,
  workspaces,
  views,
  ai,
  feedbackEnabled,
  onSendFeedback,
  theme = 'dark',
  onSignOut,
  focusViewId,
  onPrepareAddView,
  startAddView,
  startNewFromTemplate,
  onClose,
  fieldsMode,
  fieldsEditColumnId,
  onFieldsConsumed,
}: {
  session: SessionLike
  workspaces: Array<{ id: string; name: string; share_token?: string | null }>
  views: Array<{ id: string; title: string; type: string; state?: Json }>
  ai?: boolean
  feedbackEnabled?: boolean
  onSendFeedback?: () => void
  theme?: 'dark' | 'light'
  onSignOut?: () => void
  focusViewId?: string | null
  onPrepareAddView?: () => void
  startAddView?: boolean
  startNewFromTemplate?: boolean
  onClose: () => void
  fieldsMode?: boolean
  fieldsEditColumnId?: string | null
  onFieldsConsumed?: () => void
}) {
  const [query, setQuery] = useState('')
  const [mode, setMode] = useState<'list' | 'ai' | 'fields'>(fieldsMode || ai ? (fieldsMode ? 'fields' : 'ai') : 'list')
  const inputRef = useRef<HTMLInputElement>(null)
  // Focus before paint so the first key after Ctrl+K is not lost to a microtask race.
  useLayoutEffect(() => {
    if (mode === 'fields') return
    inputRef.current?.focus()
  }, [mode])
  const [fieldsHost, setFieldsHost] = useState(() => readFieldsHost())
  useEffect(() => subscribeFieldsHost(() => setFieldsHost(readFieldsHost())), [])
  useEffect(() => {
    if (fieldsMode) setMode('fields')
  }, [fieldsMode])
  useEffect(() => {
    if (mode === 'fields' && fieldsEditColumnId) onFieldsConsumed?.()
  }, [mode, fieldsEditColumnId, onFieldsConsumed])
  const [active, setActive] = useState<number | null>(null)
  const [stack, setStack] = useState<Array<{ label: string; rows: PaletteItem[] }>>([])
  const [turns, setTurns] = useState<Turn[]>([])
  const [steps, setSteps] = useState<Step[]>([])
  const [answer, setAnswer] = useState('')
  const [running, setRunning] = useState(false)
  const [proposal, setProposal] = useState<ProposalPreview | null>(null)
  const [editing, setEditing] = useState<number | null>(null)
  const found = useQuery({
    queryKey: ['palette', query],
    queryFn: () => api.get('/v1/search', { params: { query: { q: query, per_page: 8, lang: uiLanguage } } }),
    enabled: mode === 'list' && query.trim().length > 0 && stack.length === 0,
  })
  const remoteFrecency = useQuery({
    queryKey: ['frecency'],
    queryFn: () => api.get('/v1/frecency'),
    staleTime: 30_000,
  })
  const [frecencyTick, setFrecencyTick] = useState(0)
  useEffect(() => {
    const entries = remoteFrecency.data?.entries
    if (!entries) return
    writeFrecency(mergeFrecency(readFrecency(), entries))
    setFrecencyTick((value) => value + 1)
  }, [remoteFrecency.data])
  const frecency = useMemo(() => readFrecency(), [query, turns.length, frecencyTick])

  const defs = useQuery({ queryKey: ['definitions'], queryFn: () => api.get('/v1/definitions'), staleTime: 60_000 })
  const workspaceType = session.workspaceType || session.listType || ''
  const selections = useQuery({
    queryKey: ['assortments', workspaceType],
    queryFn: () => api.get('/v1/assortments', { params: { query: { type: workspaceType } } }),
    enabled: Boolean(workspaceType),
  })
  const selectedObject = useQuery({
    queryKey: ['object', session.objectId],
    queryFn: () => api.get('/v1/objects/{id}', { params: { path: { id: session.objectId ?? '' } } }),
    enabled: Boolean(session.objectId),
    staleTime: 30_000,
  })
  const types = useMemo(() => typeNames(defs.data, uiLanguage), [defs.data])
  const workspaceTypeId = types.find((type) => type.key === workspaceType)?.id ?? ''
  const selectionItems = ((selections.data?.assortments ?? []) as Json[]).filter((item) =>
    selectionMatchesType(item, workspaceType, workspaceTypeId),
  )
  const suggestions = useQuery({
    queryKey: ['proposals'],
    queryFn: () => api.get('/v1/proposals'),
  })
  const suggestionTotal = Number(((suggestions.data?.pending as Json | undefined)?.total) ?? 0)
  const selectedKey = String(selectedObject.data?.type ?? '')
  const selectedSingular = singularOf(defs.data, selectedKey, uiLanguage) || (types.length === 1 ? types[0]?.singular ?? '' : '')
  // Columns of the open table, read once when the palette opens.
  const tableColumns = useMemo(() => readUiContext().columns, [])
  const catalog = useMemo(
    () => [
      ...buildCatalog(workspaces, views, session.objectId, types, selectedSingular, selectedKey, suggestionTotal, feedbackEnabled, theme),
      {
        id: 'selections',
        group: 'views' as const,
        title: `${copy.assortments} ›`,
        subtitle: 'Saved selections for this type',
        keywords: ['assortment', 'selection'],
      },
      {
        id: 'selection:new',
        group: 'views' as const,
        title: copy.newSelection,
        subtitle: copy.assortments,
        keywords: ['assortment', 'selection'],
      },
      ...columnEntries(tableColumns, readUiContext().activeColumn),
      ...(session.group
        ? [{
            id: 'model:delete-group',
            group: 'object' as const,
            title: copy.deleteMenu,
            subtitle: 'Delete the selected group',
            keywords: ['delete', 'remove'],
          }]
        : []),
      { id: 'model:new-type', group: 'object' as const, title: copy.newType, subtitle: copy.dataModel, keywords: ['create', 'type'] },
      { id: 'model:new-link-type', group: 'object' as const, title: copy.newLinkType, subtitle: copy.dataModel, keywords: ['create', 'link', 'type'] },
      { id: 'link:to', group: 'object' as const, title: `${copy.linkTo} ›`, subtitle: copy.addLink, keywords: ['link', 'relation'] },
      { id: 'link:add', group: 'object' as const, title: copy.addLink, subtitle: copy.linkTypes, keywords: ['link', 'add'] },
      { id: 'link:remove', group: 'object' as const, title: copy.removeLink, subtitle: copy.linkTypes, keywords: ['link', 'remove'] },
      { id: 'model:delete-link-type', group: 'object' as const, title: copy.deleteLinkType, subtitle: copy.dataModel, keywords: ['delete', 'link', 'type'] },
      { id: 'model:new-list', group: 'object' as const, title: copy.newValueList, subtitle: copy.dataModel, keywords: ['create', 'list', 'option'] },
      { id: 'model:new-family', group: 'object' as const, title: copy.newUnitFamily, subtitle: copy.dataModel, keywords: ['create', 'unit'] },
      { id: 'model:new-tree', group: 'object' as const, title: copy.newTree, subtitle: copy.dataModel, keywords: ['create', 'structure', 'tree'] },
      { id: 'model:review', group: 'object' as const, title: copy.reviewAndSave, subtitle: copy.dataModel, keywords: ['draft', 'save'], kbd: 'Ctrl+S' },
      { id: 'model:discard', group: 'object' as const, title: copy.discardDraft, subtitle: copy.dataModel, keywords: ['draft'] },
      { id: 'model:undo', group: 'object' as const, title: copy.undoModel, subtitle: copy.dataModel, keywords: ['undo'] },
      { id: 'model:delete-type', group: 'object' as const, title: copy.deleteType, subtitle: copy.dataModel, keywords: ['delete', 'type'] },
      ...(focusedStandard(defs.data)
        ? []
        : [{ id: 'model:delete-attribute', group: 'object' as const, title: copy.deleteAttribute, subtitle: copy.dataModel, keywords: ['delete', 'attribute'] }]),
      { id: 'model:delete-structure', group: 'object' as const, title: copy.deleteStructure, subtitle: copy.dataModel, keywords: ['delete', 'structure', 'tree'] },
      { id: 'model:delete-list', group: 'object' as const, title: copy.deleteValueList, subtitle: copy.dataModel, keywords: ['delete', 'list'] },
      { id: 'model:delete-option', group: 'object' as const, title: copy.deleteOption, subtitle: copy.dataModel, keywords: ['delete', 'option'] },
      { id: 'model:delete-family', group: 'object' as const, title: copy.deleteUnitFamily, subtitle: copy.dataModel, keywords: ['delete', 'unit'] },
      { id: 'model:delete-unit', group: 'object' as const, title: copy.deleteUnit, subtitle: copy.dataModel, keywords: ['delete', 'unit'] },
      { id: 'attr:add-type', group: 'object' as const, title: copy.addToType, subtitle: copy.attributeDefinitions, keywords: ['attribute', 'type', 'attach'] },
      { id: 'attr:add-group', group: 'object' as const, title: copy.addToGroup, subtitle: copy.attributeDefinitions, keywords: ['attribute', 'group', 'attach'] },
      { id: 'attr:rename', group: 'object' as const, title: copy.renameAttribute, subtitle: copy.attributeDefinitions, keywords: ['attribute', 'label', 'rename'] },
      ...(focusedStandard(defs.data)
        ? []
        : [
            { id: 'attr:rename-key', group: 'object' as const, title: copy.renameKey, subtitle: copy.attributeDefinitions, keywords: ['attribute', 'key', 'rename'] },
            { id: 'attr:delete', group: 'object' as const, title: copy.deleteMenu, subtitle: copy.attributeDefinitions, keywords: ['attribute', 'delete'] },
          ]),
      { id: 'attr:new', group: 'object' as const, title: copy.newAttribute, subtitle: copy.attributeDefinitions, keywords: ['attribute', 'create'] },
    ],
    [workspaces, views, session.objectId, session.group, types, selectedSingular, selectedKey, suggestionTotal, feedbackEnabled, theme, tableColumns, defs.data],
  )
  const hits: PaletteItem[] = ((found.data?.hits as Json[] | undefined) ?? []).map((hit) => {
    const row = (hit.document as Json) ?? hit
    const type = String(row.type_key ?? row.type ?? '')
    const status = readIndexed(row, defs.data, 'status') || textValue(row.a_status)
    const version = String(row.version ?? '')
    const name = catalogName(row, defs.data)
    return {
      id: `hit:${String(row.id)}`,
      sourceId: String(row.id),
      group: 'search' as const,
      title: `${String(row.key ?? '')} · ${name || type}`,
      subtitle: openObjectLabel(type ? typeLabel(defs.data, type, uiLanguage) : ''),
      typeKey: type,
      chips: [
        ...(type ? [{ text: typeLabel(defs.data, type, uiLanguage), tone: 'neutral' as const }] : []),
        ...(status ? [{ text: optionLabel(defs.data, 'status', status, uiLanguage), tone: statusTone(status) }] : []),
        ...(version ? [{ text: version, tone: 'neutral' as const }] : []),
      ],
    }
  })
  useEffect(() => {
    if (!startAddView) return
    onPrepareAddView?.()
    setStack([{
      label: `${copy.addView} ›`,
      rows: addViewSubmenuRows(views, types, selectedKey),
    }])
    setActive(0)
    // Mount-only: right-click Add view opens the submenu immediately.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [startAddView])
  useEffect(() => {
    if (!startNewFromTemplate) return
    setStack([{
      label: `${copy.newWorkspace} ›`,
      rows: newFromTemplateSubmenuRows(types, workspaces),
    }])
    setActive(0)
    // Mount-only: tab menu "New from template" opens the submenu immediately.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [startNewFromTemplate])

  const rootRows = withModelCommand(query, visibleRows(query, catalog, hits, frecency))
  const stackRows = stack.at(-1)?.rows
  const rows = stackRows ? filterPaletteRows(stackRows, query) : rootRows
  const activeRow = active == null ? undefined : rows[active]

  useEffect(() => {
    if (stack.length > 0 || mode !== 'list') return
    const index = rows.findIndex((row) => row.sourceId === 'model:command' || row.sourceId === 'assortment:save')
    if (index >= 0) setActive(index)
  }, [query, mode, stack.length])
  useEffect(() => {
    document.querySelector<HTMLElement>('#palette-list .hit.on')?.scrollIntoView({ block: 'nearest' })
  }, [active, rows.length])

  const run = (item: PaletteItem) => {
    const id = item.sourceId ?? item.id
    const remembered = item.group === 'best' || item.id.startsWith('best:') ? id : item.id
    if (id !== 'ask') {
      touch(remembered)
      void api.post('/v1/frecency', { body: { id: remembered } }).catch(() => undefined)
    }
    if (isAddViewRoot(id)) {
      onPrepareAddView?.()
      setStack((current) => [...current, {
        label: item.title,
        rows: addViewSubmenuRows(views, types, selectedKey),
      }])
      setQuery('')
      setActive(0)
      return
    }
    if (isNewFromTemplateRoot(id)) {
      setStack((current) => [...current, {
        label: item.title,
        rows: newFromTemplateSubmenuRows(types, workspaces),
      }])
      setQuery('')
      setActive(0)
      return
    }
    const addChoice = parseAddViewChoice(id)
    if (addChoice) {
      if (addChoice.kind === 'type') {
        onPrepareAddView?.()
        session.openTypeView?.(addChoice.value)
      } else {
        const target = focusViewId || undefined
        dispatchUi('ui.open_view', {
          id: addChoice.value,
          zone: 'center',
          ...(target ? { target } : {}),
        })
      }
      onClose()
      return
    }
    if (id === 'filter:by' || id === 'export:columns') {
      const type = session.workspaceType || session.listType || ''
      const mode = id === 'filter:by' ? 'filter' : 'export'
      void api.get('/v1/types/{id}/attribute-paths', { params: { path: { id: type } } })
        .then((body) => {
          setStack((current) => [...current, {
            label: item.title,
            rows: (body.paths ?? []).map((path) => ({
              id: `path:${mode}:${path.text}`,
              sourceId: `path:${mode}:${path.text}`,
              group: 'views' as const,
              title: path.label,
              subtitle: path.text,
            })),
          }])
          setQuery('')
          setActive(0)
        })
        .catch(() => undefined)
      return
    }
    if (id.startsWith('path:filter:')) {
      window.dispatchEvent(new CustomEvent('braian-path-filter', { detail: { text: id.slice('path:filter:'.length), label: item.title } }))
      onClose()
      return
    }
    if (id.startsWith('path:export:')) {
      window.dispatchEvent(new CustomEvent('braian-export-column', { detail: { text: id.slice('path:export:'.length), label: item.title } }))
      onClose()
      return
    }
    if (id === 'columns:hide') {
      setStack((current) => [...current, { label: item.title, rows: columnRows(tableColumns, true) }])
      setQuery('')
      setActive(0)
      return
    }
    if (id === 'fields:select') {
      setMode('fields')
      return
    }
    if (id === 'fields:edit') {
      setMode('fields')
      return
    }
    if (id.startsWith('column:hide:') || id.startsWith('column:show:')) {
      const column = id.slice('column:hide:'.length)
      dispatchUi(id.startsWith('column:hide:') ? 'ui.hide_column' : 'ui.show_column', { column })
      onClose()
      return
    }
    if (id === 'column:move-left' || id === 'column:move-right') {
      const column = readUiContext().activeColumn
      if (column) {
        dispatchUi('ui.move_column', {
          column,
          direction: id === 'column:move-left' ? 'left' : 'right',
        })
      }
      onClose()
      return
    }
    if (id === 'model:new-type' || id === 'model:new-list' || id === 'model:new-family' || id === 'model:new-tree' || id === 'model:new-link-type') {
      const kind = id === 'model:new-type' ? 'type' : id === 'model:new-list' ? 'list' : id === 'model:new-family' ? 'family' : id === 'model:new-link-type' ? 'link' : 'tree'
      openModelDialog(kind)
      onClose()
      return
    }
    if (id === 'link:to') {
      const rows = ((defs.data?.types as Json[]) ?? [])
        .filter((item) => item.kind === 'link')
        .map((item) => ({
          id: `link:type:${String(item.key)}`,
          sourceId: `link:type:${String(item.key)}`,
          group: 'object' as const,
          title: textValue(item.labels, uiLanguage) || textValue(item.singular, uiLanguage) || String(item.key),
          subtitle: copy.linkTo,
        }))
      setStack((current) => [...current, { label: copy.linkTo, rows }])
      setQuery('')
      setActive(0)
      return
    }
    if (id.startsWith('link:type:')) {
      window.dispatchEvent(new CustomEvent('braian-add-link', { detail: { linkType: id.slice('link:type:'.length) } }))
      onClose()
      return
    }
    if (id === 'link:add') {
      window.dispatchEvent(new CustomEvent('braian-add-link', { detail: {} }))
      onClose()
      return
    }
    if (id === 'link:remove') {
      window.dispatchEvent(new Event('braian-remove-link'))
      onClose()
      return
    }
    if (id === 'model:delete-link-type') {
      const target = modelFocus()
      if (target?.kind === 'type') {
        void stageModel('model.delete_link_type', { key: target.key }).catch(() => undefined)
      }
      onClose()
      return
    }
    if (id === 'model:review') {
      requestReview()
      onClose()
      return
    }
    if (id === 'model:discard') {
      requestDiscardDraft()
      onClose()
      return
    }
    if (id === 'model:undo') {
      void api.post('/v1/operations/{name}', { params: { path: { name: "model.undo" } }, body: {} }).catch((reason: unknown) => {
        session.push(reason instanceof Error ? reason.message : copy.validation)
      })
      onClose()
      return
    }
    if (id.startsWith('model:delete-') && id !== 'model:delete-group') {
      const target = modelFocus()
      const runDelete = (name: string, args: Json) => {
        void stageModel(name, args).catch((reason: unknown) => session.push(reason instanceof Error ? reason.message : copy.validation))
      }
      if (id === 'model:delete-type' && target?.kind === 'type') runDelete('model.delete_type', { type: target.key })
      if (id === 'model:delete-attribute' && target?.kind === 'attribute') runDelete('model.delete_attribute', { attribute: target.key })
      if (id === 'model:delete-structure' && target?.kind === 'structure') runDelete('model.delete_structure', { structure: target.key })
      if (id === 'model:delete-list' && target?.kind === 'value_list') runDelete('model.delete_value_list', { value_list: target.key })
      if (id === 'model:delete-option' && target?.kind === 'option') runDelete('model.delete_value_option', { value_list: target.list, option: target.code })
      if (id === 'model:delete-family' && target?.kind === 'unit_family') runDelete('model.delete_unit_family', { unit_family: target.key })
      if (id === 'model:delete-unit' && target?.kind === 'unit') runDelete('model.delete_unit', { unit_family: target.family, unit: target.code })
      onClose()
      return
    }
    if (id === 'model:delete-group' && session.group) {
      const [structure, group] = session.group.split(':')
      if (structure && group) {
        void api.post('/v1/operations/{name}', { params: { path: { name: "model.delete_group" } }, body: { structure, group } }).then((result) => {
          const proposal = operationId(result)
          if (proposal) {
            void import('./proposal-bus').then(({ publishProposal }) => publishProposal(proposal))
          }
        }).catch(() => undefined)
      }
      onClose()
      return
    }
    if (id === 'selections') {
      setStack((current) => [...current, {
        label: copy.assortments,
        rows: [
          ...selectionItems.map((item) => ({
            id: `selection:open:${String(item.id)}`,
            sourceId: `selection:open:${String(item.id)}`,
            group: 'views' as const,
            title: String(item.name ?? ''),
            subtitle: copy.assortments,
          })),
          { id: 'selection:new', sourceId: 'selection:new', group: 'views' as const, title: copy.newSelection, subtitle: copy.assortments },
        ],
      }])
      setQuery('')
      setActive(0)
      return
    }
    if (id === 'selection:new') {
      requestNewSelection()
      onClose()
      return
    }
    if (id.startsWith('selection:open:')) {
      const item = selectionItems.find((row) => String(row.id) === id.slice('selection:open:'.length))
      if (item) emitSavedSelection(item)
      onClose()
      return
    }
    if (id === 'assortment:save') {
      const name = query.replace(/^save (?:assortment|selection)\s+/i, '').trim()
      const type = session.workspaceType || session.listType || ''
      void api.post('/v1/operations/{name}', { params: { path: { name: "assortment.save" } }, body: {
          name,
          ...(type ? { type } : {}),
          query: {
            ...(session.group ? { group: session.group } : {}),
          },
        } })
        .then((result) => {
          const proposalId = operationId(result)
          if (!proposalId) return
          publishProposal(proposalId)
          session.setProposalId(proposalId)
          onClose()
        })
        .catch((reason: unknown) => {
          session.push(reason instanceof Error ? reason.message : copy.validation)
        })
      return
    }
    if (id === 'goto:attribute-definitions') {
      const here = views.find((view) => view.type === 'attribute_definitions')
      if (here) {
        dispatchUi('ui.open_view', { id: here.id, zone: 'center' })
      } else {
        queueViewType('attribute_definitions')
        const model = workspaces.find((workspace) => workspace.share_token === 'braian-model')
        if (model) {
          dispatchUi('ui.open_workspace', { id: model.id })
          session.openWorkspace(model.id)
        }
      }
      onClose()
      return
    }
    if (id === 'attr:add-type' || id === 'attr:add-group' || id === 'attr:rename' || id === 'attr:new') {
      const action = id === 'attr:add-type' ? 'add-type' : id === 'attr:add-group' ? 'add-group' : id === 'attr:rename' ? 'rename' : 'new'
      requestAttributeAction(action)
      onClose()
      return
    }
    if (id === 'attr:delete') {
      requestAttributeAction('delete')
      onClose()
      return
    }
    if (id === 'model:rename-key' || id === 'attr:rename-key') {
      if (requestAttributeAction('rename-key')) {
        onClose()
        return
      }
      setQuery('rename attribute key ')
      setActive(0)
      return
    }
    if (id === 'object:move-group') {
      window.dispatchEvent(new Event('braian-move-group'))
      onClose()
      return
    }
    if (id === 'model:allowed-trees') {
      window.dispatchEvent(new Event('braian-allowed-trees'))
      onClose()
      return
    }
    if (id === 'model:command') {
      const deleting = /^delete\b/i.test(query.trim())
      const call = deleting
        ? api.post('/v1/operations/{name}', { params: { path: { name: "model.command" } }, body: { text: query } })
        : stageModel('model.command', { text: query })
      void call
        .then((result) => {
          const proposalId = operationId(result)
          if (proposalId) {
            publishProposal(proposalId)
            session.setProposalId(proposalId)
          }
          onClose()
        })
        .catch((reason: unknown) => {
          session.push(reason instanceof Error ? reason.message : 'The draft has no steps')
        })
      return
    }
    if (item.shortcut) toast(`Next time: ${item.shortcut}`)
    if (id === 'ask' || item.group === 'ask') {
      void ask(query)
      return
    }
    if (id.startsWith('hit:') || item.group === 'search') {
      const objectId = id.startsWith('hit:') ? id.slice(4) : id
      session.setObjectId(objectId)
      dispatchUi('ui.select', { id: objectId })
      onClose()
      return
    }
    if (id === 'suggestions:review') {
      const review = workspaces.find((workspace) => workspace.share_token === 'braian-review')
      if (review) {
        dispatchUi('ui.open_workspace', { id: review.id })
        session.openWorkspace(review.id)
      }
      onClose()
      return
    }
    if (id === 'workspace:delete') {
      window.dispatchEvent(new Event('braian-workspace-delete'))
      onClose()
      return
    }
    if (id.startsWith('workspace:')) {
      const workspaceId = id.slice('workspace:'.length)
      dispatchUi('ui.open_workspace', { id: workspaceId })
      session.openWorkspace(workspaceId)
      onClose()
      return
    }
    if (id.startsWith('layout:')) {
      const rest = id.slice('layout:'.length)
      if (rest.startsWith('focus:')) dispatchUi('focus', { direction: rest.slice('focus:'.length) })
      else dispatchUi(rest, {})
      onClose()
      return
    }
    if (id.startsWith('type-view:')) {
      session.openTypeView?.(id.slice('type-view:'.length))
      onClose()
      return
    }
    if (id.startsWith('view:')) {
      dispatchUi('ui.open_view', { id: id.slice(5), zone: 'center' })
      onClose()
      return
    }
    if (id === 'feedback:send') {
      onSendFeedback?.()
      onClose()
      return
    }
    if (id.startsWith('template:')) {
      const template = workspaceTemplates(types).find((item) => item.id === id.slice('template:'.length))
      if (!template) return
      void createWorkspaceFromTemplate(template).then((workspaceId) => {
        dispatchUi('ui.open_template', { template: template.id, id: workspaceId })
        session.openWorkspace(workspaceId)
        session.push(`Workspace ${template.name}`)
      })
      onClose()
      return
    }
    if (id === 'import:file') {
      window.dispatchEvent(new Event('braian-import-open'))
      onClose()
      return
    }
    if (id === 'object:open') {
      dispatchUi('ui.open_view', { title: 'Detail', zone: 'center' })
      onClose()
      return
    }
    if (id === 'object:right') {
      dispatchUi('ui.place', { title: 'Detail', zone: 'right' })
      onClose()
      return
    }
    if (id === 'object:history') {
      dispatchUi('ui.open_view', { title: 'History', zone: 'center' })
      onClose()
      return
    }
    if (id === 'object:graph') {
      dispatchUi('ui.open_view', { title: 'Graph', zone: 'center' })
      onClose()
      return
    }
    if (id === 'object:copy' && session.objectId) {
      void navigator.clipboard?.writeText(session.objectId)
      session.push(session.objectId)
      onClose()
      return
    }
    if (id === 'theme:toggle') {
      dispatchUi('ui.set_theme', { theme: theme === 'dark' ? 'light' : 'dark' })
      onClose()
      return
    }
    if (id === 'agent:open') {
      setQuery('')
      setActive(null)
      setMode('ai')
      return
    }
    if (id === 'session:sign-out') {
      onSignOut?.()
      onClose()
    }
  }

  const secondaryOf = (item: PaletteItem | undefined): PaletteItem[] => {
    if (!item) return []
    const id = item.sourceId ?? item.id
    if (item.group === 'search' || id.startsWith('hit:')) {
      const objectId = id.startsWith('hit:') ? id.slice(4) : id
      return [
        { id: `open:${objectId}`, sourceId: objectId, group: 'object', title: 'Open', subtitle: 'Select this object', kbd: 'Enter' },
        { id: `right:${objectId}`, sourceId: objectId, group: 'object', title: 'Open to the right', subtitle: 'Split the pane', kbd: 'Ctrl+Enter' },
        { id: `copy:${objectId}`, sourceId: objectId, group: 'object', title: 'Copy id', subtitle: objectId },
      ]
    }
    if (id.startsWith('view:') || id.startsWith('workspace:')) {
      return [
        ...(['left', 'right', 'top', 'bottom'] as const).map((zone) => ({
          id: `place:${id}:${zone}`,
          sourceId: id,
          group: 'views' as const,
          title: `Place ${zone}`,
          subtitle: 'Dock the view in that zone',
        })),
        {
          id: `place:${id}:center`,
          sourceId: id,
          group: 'views' as const,
          title: 'New tab',
          subtitle: 'Add the view as a tab',
        },
      ]
    }
    return []
  }

  const ask = async (text: string) => {
    const prompt = text.trim()
    if (!prompt || running) return
    const history = editing == null ? [...turns, { role: 'user' as const, text: prompt }] : turns.map((turn, index) => (index === editing ? { ...turn, text: prompt } : turn))
    setTurns(history.filter((turn) => turn.role === 'user' || turn.role === 'assistant'))
    setEditing(null)
    setQuery('')
    setMode('ai')
    setAnswer('')
    setProposal(null)
    setRunning(true)
    setSteps([{ id: 'read', text: stepText('read'), state: 'run' }])
    session.push(`Agent: ${prompt}`)
    const messages = history.map((turn) => ({ role: turn.role, content: turn.text }))
    try {
      await streamAgent(messages, (event) => applyEvent(event, setSteps, setAnswer, setProposal, session, stepHint(event, types)))
      setSteps((current) => current.map((step) => ({ ...step, state: 'done' })))
    } catch (error) {
      const message = error instanceof ApiError || error instanceof Error ? error.message : 'The model did not reply.'
      setAnswer(message)
    } finally {
      setRunning(false)
    }
  }

  const onKeyDown = (event: ReactKeyboardEvent<HTMLDivElement>) => {
    if (event.key === 'Escape') {
      event.preventDefault()
      event.stopPropagation()
      // Opened straight into New from template: Esc closes the palette.
      if (startNewFromTemplate && mode === 'list' && stack.length > 0) {
        onClose()
        return
      }
      if (stack.length > 0 && mode === 'list') {
        setStack((current) => current.slice(0, -1))
        setActive(null)
        return
      }
      onClose()
      return
    }
    if (event.key === 'Backspace' && query === '' && stack.length > 0 && mode === 'list') {
      event.preventDefault()
      setStack((current) => current.slice(0, -1))
      setActive(null)
      return
    }
    if (event.key === 'Tab') {
      event.preventDefault()
      setMode('list')
      setStack([])
      setActive(askIndex(rootRows))
      return
    }
    if ((event.ctrlKey || event.metaKey) && (event.key === 'Enter' || event.key === '.')) {
      event.preventDefault()
      const actions = secondaryOf(activeRow)
      if (actions.length === 0) return
      setStack((current) => [...current, { label: activeRow?.title ?? 'Actions', rows: actions }])
      setActive(0)
      return
    }
    if (event.key === 'ArrowRight' && activeRow && (activeRow.group === 'search' || (activeRow.sourceId ?? '').startsWith('hit:'))) {
      event.preventDefault()
      const actions = secondaryOf(activeRow)
      setStack((current) => [...current, { label: activeRow.title, rows: actions }])
      setActive(0)
      return
    }
    if (!['ArrowDown', 'ArrowUp', 'PageDown', 'PageUp', 'Home', 'End', 'Enter'].includes(event.key)) return
    if (mode === 'ai' && event.key === 'Enter' && (event.target as HTMLElement).tagName === 'INPUT') {
      event.preventDefault()
      void ask(query)
      return
    }
    if (rows.length === 0 || mode === 'ai') return
    event.preventDefault()
    event.stopPropagation()
    if (event.key === 'Enter') {
      const chosen = active == null ? 0 : active
      const item = rows[chosen]
      if (!item) return
      if (item.group === 'search') {
        run(item)
        return
      }
      const actions = secondaryOf(item)
      if (stack.length > 0 && item.id.startsWith('right:')) {
        dispatchUi('ui.place', { id: item.sourceId, zone: 'right' })
        onClose()
        return
      }
      if (stack.length > 0 && item.id.startsWith('place:')) {
        const zone = item.id.split(':').at(-1) ?? 'right'
        dispatchUi('ui.place', { id: item.sourceId, zone })
        onClose()
        return
      }
      if (stack.length > 0 && item.id.startsWith('copy:') && item.sourceId) {
        void navigator.clipboard?.writeText(item.sourceId)
        onClose()
        return
      }
      if (stack.length > 0 && item.id.startsWith('open:') && item.sourceId) {
        session.setObjectId(item.sourceId)
        onClose()
        return
      }
      if (actions.length > 0 && event.ctrlKey) {
        setStack((current) => [...current, { label: item.title, rows: actions }])
        return
      }
      run(item)
      return
    }
    const page = 8
    const current = active ?? -1
    const next = event.key === 'ArrowDown'
      ? Math.min(rows.length - 1, current + 1)
      : event.key === 'ArrowUp'
        ? Math.max(0, current < 0 ? rows.length - 1 : current - 1)
        : event.key === 'PageDown'
          ? Math.min(rows.length - 1, (current < 0 ? 0 : current) + page)
          : event.key === 'PageUp'
            ? Math.max(0, (current < 0 ? 0 : current) - page)
            : event.key === 'Home'
              ? 0
              : rows.length - 1
    setActive(next)
  }

  const groups = groupBlocks(rows.filter((row) => row.group !== 'ask'))
  const askRow = rows.find((row) => row.group === 'ask')
  const userTurns = turns.filter((turn) => turn.role === 'user')
  const question = userTurns.at(-1)?.text ?? ''
  const nothingElse = rows.length === 1 && rows[0]?.group === 'ask'
  const selected = nothingElse ? 0 : active


  if (mode === 'fields') {
    const host = fieldsHost
    const editId = fieldsEditColumnId
    const open: FieldsPaletteOpen = (() => {
      if (editId && host) {
        const col = host.columns.find((item) => item.id === editId)
        if (col) return { kind: 'edit', column: col }
      }
      return { kind: 'browse' }
    })()
    if (!host) {
      return (
        <div className="palette" role="dialog" data-testid="command-palette">
          <p className="muted">Open an object table to select fields.</p>
          <button type="button" onClick={onClose}>Close</button>
        </div>
      )
    }
    return (
      <div className="palette" role="dialog" data-testid="command-palette" data-fields-mode="true">
        <FieldsMode
          open={open}
          columns={host.columns}
          fields={host.fields}
          onClose={onClose}
          onToggleColumn={host.onToggleColumn}
          onShowColumn={host.onShowColumn}
          onReplaceColumn={host.onReplaceColumn}
          peerOptionsForField={host.peerOptionsForField}
          searchPeersForField={host.searchPeersForField}
        />
      </div>
    )
  }

  return (
    <div className="palette" role="dialog" data-testid="command-palette" onMouseDown={onClose}>
      <Command
        className={mode === 'ai' ? 'ai-panel' : undefined}
        shouldFilter={false}
        loop={false}
        disablePointerSelection
        value={selected == null ? '' : rows[selected]?.id ?? ''}
        onMouseDown={(event) => event.stopPropagation()}
        onKeyDownCapture={onKeyDown}
      >
        <div className="palette-query">
          {mode === 'ai' ? <span className="ai-badge" data-testid="ai-badge">AI</span> : null}
          <CommandInput
            ref={inputRef}
            data-testid="palette-input"
            autoFocus
            value={query}
            placeholder={mode === 'ai' ? 'Ask AI' : commandPlaceholder(types)}
            onValueChange={(value) => {
              setQuery(value)
              setActive(null)
            }}
          />
          {mode === 'ai' ? (
            <button type="button" className="icon-btn palette-close" onClick={onClose}>{copy.close}</button>
          ) : null}
        </div>
        {stack.length > 0 ? <div className="crumb">{stack.map((crumb) => crumb.label).join(' / ')}</div> : null}
        {mode === 'ai' ? (
          <div className="ai-log" data-testid="ai-log">
            {userTurns.slice(0, -1).map((turn, index) => (
              <button key={`earlier-${index}`} type="button" className="ai-earlier" onClick={() => { setQuery(turn.text); setEditing(index) }}>
                {turn.text}
              </button>
            ))}
            {question ? <h2 className="ai-question" data-testid="ai-question">{question}</h2> : null}
            {running ? <div className="progress" data-testid="ai-progress"><div className="progress-bar" /></div> : null}
            {steps.length > 0 ? (
              <ol className="ai-steps">
                {steps.map((step) => (
                  <li key={step.id} className={step.state === 'done' ? 'ai-step done' : 'ai-step'} data-testid="ai-step">
                    {step.state === 'done' ? <Check size={14} /> : <span className="ai-step-mark" aria-hidden="true">…</span>}
                    <span>{step.text}</span>
                  </li>
                ))}
              </ol>
            ) : null}
            {answer ? <p className="ai-answer" data-testid="ai-answer">{renderAgentAnswer(answer)}</p> : null}
            {proposal ? (
              <div data-testid="palette-proposal">
                <strong>{proposal.title}</strong>
                {proposal.source ? <span className="meta-chip neutral" data-testid="source-badge">{sourceLabel(proposal)}</span> : null}
                <p>{proposal.rows.length} rows</p>
                {proposal.lines.length > 0 ? (
                  <ul className="proposal-tree">
                    {proposal.lines.map((line) => (
                      <li key={line}>{line}</li>
                    ))}
                  </ul>
                ) : null}
                {proposal.rows.slice(0, 3).map((row) => (
                  <p key={String(row.id)}>{String(row.object_key ?? row.object_id ?? '')}</p>
                ))}
                {proposal.error ? <p className="error">{proposal.error}</p> : null}
                <div className="toolbar">
                  <button type="button" onClick={() => void api.post('/v1/proposals/{id}/discard', { params: { path: { id: proposal.id } } }).then(() => setProposal(null))}>{copy.discardAll}</button>
                  <button type="button" className="primary" onClick={() => void api.post('/v1/proposals/{id}/accept', { params: { path: { id: proposal.id } }, body: {} }).then(() => { session.setProposalId(proposal.id); setProposal(null); session.push(copy.saved) })}>{copy.saveAll}</button>
                </div>
              </div>
            ) : null}
          </div>
        ) : (
            <CommandList id="palette-list">
              {groups.map((group) => (
                <div key={group.name} className="palette-group">
                  {group.name !== 'ask' ? <div>{group.name === 'search' ? searchGroupTitle(hits.map((hit) => hit.typeKey ?? ''), types) : group.name === 'object' && selectedSingular ? `On this ${selectedSingular}` : groupTitle[group.name]}</div> : null}
                  {group.rows.map((row) => {
                    const index = rows.indexOf(row)
                    return (
                      <CommandItem
                        key={row.id}
                        value={row.id}
                        className={index === selected ? `hit on${row.group === 'ask' ? ' ask' : ''}` : `hit${row.group === 'ask' ? ' ask' : ''}`}
                        onPointerDown={() => setActive(index)}
                        onSelect={() => run(row)}
                      >
                        <span className="hit-copy">
                          <span className="hit-title">{highlight(row.title, query)}</span>
                          {row.subtitle ? <small>{row.subtitle}</small> : null}
                        </span>
                        {row.chips && row.chips.length > 0 ? (
                          <span className="hit-chips">
                            {row.chips.map((chip) => (
                              <MetaChip key={chip.text} tone={chip.tone as ChipTone}>{chip.text}</MetaChip>
                            ))}
                          </span>
                        ) : null}
                        {row.kbd ? <Kbd>{row.kbd}</Kbd> : null}
                      </CommandItem>
                    )
                  })}
                </div>
              ))}
            </CommandList>
        )}
        {mode === 'list' && askRow ? (
          <CommandItem
            value={askRow.id}
            className={rows.indexOf(askRow) === selected ? 'hit on ask' : 'hit ask'}
            onPointerDown={() => setActive(rows.indexOf(askRow))}
            onSelect={() => run(askRow)}
          >
            <span className="hit-copy">
              <span className="hit-title">{highlight(askRow.title, query)}</span>
              {askRow.subtitle ? <small>{askRow.subtitle}</small> : null}
            </span>
            {askRow.kbd ? <Kbd>{askRow.kbd}</Kbd> : null}
          </CommandItem>
        ) : null}
        <div className="palette-foot" data-testid="palette-foot">
          <span data-testid="palette-active">{activeRow?.subtitle ?? ''}</span>
          <span><Kbd>Enter</Kbd> run</span>
          <span><Kbd>Ctrl</Kbd>+<Kbd>Enter</Kbd> actions</span>
          <span><Kbd>Tab</Kbd> ask AI</span>
          <span><Kbd>Esc</Kbd> close</span>
        </div>
      </Command>
    </div>
  )
}

function focusedStandard(defs: Json | undefined) {
  const target = modelFocus()
  if (target?.kind !== 'attribute' || !defs) return false
  const row = ((defs.attributes as Json[] | undefined) ?? []).find((item) => String(item.key) === target.key)
  if (!row) return false
  return isStandardAttribute(String(row.id ?? ''), String(row.key ?? ''))
}

function stepHint(event: AgentEvent, types: ReturnType<typeof typeNames>) {
  const key = typeof event.args?.type === 'string' ? event.args.type : typeof event.args?.type_key === 'string' ? event.args.type_key : ''
  const match = key ? types.find((item) => item.key === key) : undefined
  const type = match ?? (types.length === 1 ? types[0] : undefined)
  return type ? { singular: type.singular, plural: type.plural } : undefined
}

function buildCatalog(
  workspaces: Array<{ id: string; name: string; share_token?: string | null }>,
  views: Array<{ id: string; title: string; type: string; state?: Json }>,
  objectId: string | null,
  types: ReturnType<typeof typeNames>,
  selectedSingular: string,
  openTypeKey: string,
  suggestions: number,
  feedbackEnabled?: boolean,
  theme: 'dark' | 'light' = 'dark',
): PaletteItem[] {
  const items: PaletteItem[] = [
    ...(suggestions > 0
      ? [{
          id: 'suggestions:review',
          group: 'views' as const,
          title: suggestedTitle(suggestions),
          subtitle: 'Review agent and import drafts',
          keywords: ['suggested', 'proposal', 'agent', 'import'],
        }]
      : []),
    ...workspaces.map((workspace, index) => ({
      id: `workspace:${workspace.id}`,
      group: 'views' as const,
      title: workspaceDisplayName(workspace.name, workspace.share_token),
      subtitle: 'Open workspace',
      kbd: index < 9 ? `Ctrl+${index + 1}` : undefined,
      shortcut: index < 9 ? `Ctrl+${index + 1}` : undefined,
    })),
    ...typeViewPaletteItems(types),
    ...views.map((view) => ({
      id: `view:${view.id}`,
      group: 'views' as const,
      title: displayViewTitle(view, types, openTypeKey),
      subtitle: 'Open view',
    })),
    newFromTemplateRootItem(),
    { id: 'import:file', group: 'views', title: copy.importFile, subtitle: 'Drop an Excel or CSV file' },
    ...(!views.some((view) => view.type === 'attribute_definitions')
      ? [{
          id: 'goto:attribute-definitions',
          group: 'views' as const,
          title: copy.attributeDefinitions,
          subtitle: copy.dataModel,
          keywords: ['attribute', 'definition', 'model'],
        }]
      : []),
    { id: 'model:rename-key', group: 'views', title: copy.renameKey, subtitle: 'Change a key. External integrations that use the old key will stop working', keywords: ['rename', 'key', 'attribute'] },
    { id: 'object:move-group', group: 'views', title: copy.moveToGroup, subtitle: 'Place the open object in a group', keywords: ['move', 'group', 'place'] },
    { id: 'model:allowed-trees', group: 'views', title: copy.allowedTrees, subtitle: 'Choose the trees a type may use', keywords: ['tree', 'structure', 'type'] },
    {
      id: 'theme:toggle',
      group: 'views',
      title: theme === 'dark' ? copy.lightMode : copy.darkMode,
      subtitle: 'Toggle theme',
      keywords: ['theme', 'toggle', 'light', 'dark', 'mode'],
    },
    { id: 'agent:open', group: 'views', title: copy.openAgent, subtitle: 'Ask the agent', kbd: 'Ctrl+J', keywords: ['agent', 'ai', 'ask'] },
    { id: 'session:sign-out', group: 'views', title: copy.signOut, subtitle: 'End this session', keywords: ['logout', 'log out', 'sign out', 'session'] },
    { id: 'layout:move_view_left', group: 'views', title: 'Move view left', subtitle: 'Move the focused view left', kbd: 'Ctrl+Alt+←', shortcut: 'Ctrl+Alt+Left' },
    { id: 'layout:move_view_right', group: 'views', title: 'Move view right', subtitle: 'Move the focused view right', kbd: 'Ctrl+Alt+→', shortcut: 'Ctrl+Alt+Right' },
    { id: 'layout:move_view_up', group: 'views', title: 'Move view up', subtitle: 'Move the focused view up', kbd: 'Ctrl+Alt+↑', shortcut: 'Ctrl+Alt+Up' },
    { id: 'layout:move_view_down', group: 'views', title: 'Move view down', subtitle: 'Move the focused view down', kbd: 'Ctrl+Alt+↓', shortcut: 'Ctrl+Alt+Down' },
    { id: 'layout:focus:left', group: 'views', title: 'Focus pane left', subtitle: 'Focus the neighbour on the left', kbd: 'Alt+←', shortcut: 'Alt+Left' },
    { id: 'layout:focus:right', group: 'views', title: 'Focus pane right', subtitle: 'Focus the neighbour on the right', kbd: 'Alt+→', shortcut: 'Alt+Right' },
    { id: 'layout:focus:up', group: 'views', title: 'Focus pane up', subtitle: 'Focus the neighbour above', kbd: 'Alt+↑', shortcut: 'Alt+Up' },
    { id: 'layout:focus:down', group: 'views', title: 'Focus pane down', subtitle: 'Focus the neighbour below', kbd: 'Alt+↓', shortcut: 'Alt+Down' },
    { id: 'layout:zoom', group: 'views', title: 'Maximize pane', subtitle: 'Show only the focused pane', kbd: 'Ctrl+Alt+Z', shortcut: 'Ctrl+Alt+Z' },
    { id: 'layout:close_view', group: 'views', title: 'Close view', subtitle: 'Close the focused view', kbd: 'Ctrl+Alt+W', shortcut: 'Ctrl+Alt+W' },
    { id: 'layout:reset_layout', group: 'views', title: 'Reset layout', subtitle: 'Restore the saved workspace layout' },
    { id: 'workspace:delete', group: 'views', title: 'Delete workspace', subtitle: 'Remove the active workspace and its layout', keywords: ['delete', 'remove', 'workspace'] },
    addViewRootItem(),
  ]
  if (feedbackEnabled) {
    items.push({
      id: 'feedback:send',
      group: 'views',
      title: 'Send feedback',
      subtitle: 'Report a bug or share an idea',
      kbd: 'Ctrl+Shift+B',
      shortcut: 'Ctrl+Shift+B',
    })
  }
  if (objectId) {
    items.push(
      { id: 'object:open', group: 'object', title: selectedSingular ? `Open ${selectedSingular}` : 'Open detail', subtitle: selectedSingular ? `On this ${selectedSingular}` : 'On the selected object', kbd: 'Enter' },
      { id: 'object:right', group: 'object', title: 'Open to the right', subtitle: 'Split beside the table' },
      { id: 'object:history', group: 'object', title: 'History', subtitle: 'As it was, and restore' },
      { id: 'object:graph', group: 'object', title: 'Graph', subtitle: 'Links and where values come from' },
      { id: 'object:copy', group: 'object', title: 'Copy id', subtitle: objectId },
    )
  }
  return items
}

// Field and column commands for the open table.
function columnEntries(columns: UiColumn[], activeColumn: string | null): PaletteItem[] {
  const items: PaletteItem[] = [
    { id: 'fields:select', group: 'views', title: `${copy.selectFields} ›`, subtitle: 'Choose and reorder columns', keywords: ['field', 'column', 'select', 'show', 'hide', 'reorder'] },
    { id: 'filter:by', group: 'views', title: `${copy.filterBy} ›`, subtitle: 'Attribute, part or link', keywords: ['filter', 'path', 'unit'] },
    { id: 'export:columns', group: 'views', title: `${copy.exportColumns} ›`, subtitle: 'Add a column to the next export', keywords: ['export', 'path', 'column'] },
  ]
  if (columns.some((column) => !column.hidden)) {
    items.push({ id: 'columns:hide', group: 'views', title: `${copy.hideColumn} ›`, subtitle: 'Pick a column in the open table', keywords: ['column'] })
  }
  const active = activeColumn && columns.some((column) => column.id === activeColumn && !column.hidden)
    ? activeColumn
    : null
  if (active) {
    const header = columns.find((column) => column.id === active)?.header ?? active
    items.push(
      {
        id: 'column:move-left',
        group: 'views',
        title: `${copy.moveColumnLeft} · ${header}`,
        subtitle: header,
        keywords: ['column', 'move', 'left', 'reorder', header],
      },
      {
        id: 'column:move-right',
        group: 'views',
        title: `${copy.moveColumnRight} · ${header}`,
        subtitle: header,
        keywords: ['column', 'move', 'right', 'reorder', header],
      },
    )
  }
  return items
}

function columnRows(columns: UiColumn[], hide: boolean): PaletteItem[] {
  return columns
    .filter((column) => column.hidden !== hide)
    .map((column) => ({
      id: `column:${hide ? 'hide' : 'show'}:${column.id}`,
      group: 'views' as const,
      title: column.header,
      subtitle: hide ? copy.hideColumn : copy.showColumn,
    }))
}

function groupBlocks(rows: PaletteItem[]) {
  const order: PaletteGroup[] = ['recent', 'best', 'object', 'views', 'search', 'ask']
  return order
    .map((name) => ({ name, rows: rows.filter((row) => row.group === name) }))
    .filter((group) => group.rows.length > 0)
}

function highlight(text: string, query: string) {
  const needle = query.trim()
  if (!needle) return text
  const index = text.toLowerCase().indexOf(needle.toLowerCase())
  if (index < 0) return text
  return (
    <>
      {text.slice(0, index)}
      <mark className="mark">{text.slice(index, index + needle.length)}</mark>
      {text.slice(index + needle.length)}
    </>
  )
}

function applyEvent(
  event: AgentEvent,
  setSteps: (value: Step[] | ((current: Step[]) => Step[])) => void,
  setAnswer: (value: string) => void,
  setProposal: (value: ProposalPreview | null) => void,
  session: SessionLike,
  typeHint?: { singular?: string; plural?: string },
) {
  if (event.type === 'step') {
    const text = event.op ? stepText(event.op, event.args, typeHint) : event.text ?? ''
    setSteps((current) => {
      const id = event.id ?? text
      const state = event.state ?? 'run'
      const existing = current.find((step) => step.id === id)
      if (!existing) return [...current.map((step) => ({ ...step, state: 'done' as const })), { id, text, state }]
      return current.map((step) => (step.id === id ? { ...step, text: text || step.text, state } : step))
    })
  }
  if (event.type === 'tool' && event.result) {
    const result = event.result
    if (result.ui === true) applyUiResult(result)
    else     if (event.name) {
      const op = uiOperations.find((name) => name.replaceAll('.', '_') === event.name) ?? event.name
      if (op.startsWith('ui.') && op !== 'ui.get_context' && op !== 'ui.get_selection') {
        dispatchUi(op, (result.args as Json) ?? {})
      }
    }
    const proposalId = String(result.proposal_id ?? (result.needs_approval ? result.id ?? '' : ''))
    if (proposalId) void loadProposal(proposalId, setProposal, session)
  }
  if (event.type === 'proposal' && event.proposal) {
    const id = String(event.proposal.proposal_id ?? event.proposal.id ?? '')
    if (id) void loadProposal(id, setProposal, session)
  }
  if (event.type === 'answer') {
    const text = event.text ?? ''
    setAnswer(text)
    setSteps((current) => current.map((step) => ({ ...step, state: 'done' })))
  }
  if (event.type === 'error') setAnswer(event.text ?? 'The model did not reply.')
}

async function loadProposal(
  id: string,
  setProposal: (value: ProposalPreview | null) => void,
  session: SessionLike,
) {
  const body = await api.get('/v1/proposals/{id}', { params: { path: { id: id } } }).catch(() => null)
  const rows = ((body?.rows as Json[]) ?? [])
  const lines = ((body?.consequence as Json | undefined)?.lines as unknown[] | undefined)?.map(String) ?? []
  setProposal({
    id,
    title: String(body?.title ?? 'Proposal'),
    rows,
    lines,
    source: body?.source ? String(body.source) : undefined,
    actor_name: body?.actor_name,
    behalf_name: body?.behalf_name,
    error: rows.some((row) => row.conflict) ? 'Conflict' : undefined,
  })
  session.setProposalId(id)
  if (lines.length > 0) publishProposal(id)
}
