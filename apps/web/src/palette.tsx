import { useEffect, useMemo, useState, type KeyboardEvent as ReactKeyboardEvent } from 'react'
import { useQuery } from '@tanstack/react-query'
import { Check, CornerDownLeft, Sparkles } from 'lucide-react'
import { MetaChip, optionLabel, statusTone, typeLabel, type ChipTone } from './chips'
import { toast } from 'sonner'
import { ApiError, api, textValue, type Json } from './api'
import { streamAgent, type AgentEvent } from './agent-stream'
import { Command, CommandInput, CommandItem, CommandList } from './components/ui/command'
import { Kbd } from './components/ui/kbd'
import { workspaceDisplayName, viewDisplayTitle } from './demo-labels'
import { copy, uiLanguage } from './i18n'
import { mergeFrecency, readFrecency, touch, writeFrecency } from './frecency'
import { withModelCommand } from './model-command'
import { askIndex, visibleRows, groupTitle, type PaletteGroup, type PaletteItem } from './palette-model'
import { publishProposal } from './proposal-bus'
import { stepText } from './steps'
import { createWorkspaceFromTemplate, workspaceTemplates } from './templates'
import { applyUiResult, dispatchUi, uiOperations } from './ui-bus'

type SessionLike = {
  objectId: string | null
  setObjectId: (value: string | null) => void
  setProposalId: (value: string | null) => void
  push: (line: string) => void
  openWorkspace: (id: string) => void
}

type Turn = { role: 'user' | 'assistant'; text: string }
type Step = { id: string; text: string; state: 'run' | 'done' }
type ProposalPreview = { id: string; title: string; rows: Json[]; lines: string[]; source?: string; error?: string }

export function CommandPalette({
  session,
  workspaces,
  views,
  ai,
  onClose,
}: {
  session: SessionLike
  workspaces: Array<{ id: string; name: string; share_token?: string }>
  views: Array<{ id: string; title: string; type: string }>
  ai?: boolean
  onClose: () => void
}) {
  const [query, setQuery] = useState('')
  const [mode, setMode] = useState<'list' | 'ai'>(ai ? 'ai' : 'list')
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
    queryFn: () => api<Json>(`/v1/search?q=${encodeURIComponent(query)}&per_page=8`),
    enabled: mode === 'list' && query.trim().length > 0 && stack.length === 0,
  })
  const remoteFrecency = useQuery({
    queryKey: ['frecency'],
    queryFn: () => api<{ entries: Record<string, { uses: number; last: number }> }>('/v1/frecency'),
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

  const defs = useQuery({ queryKey: ['definitions'], queryFn: () => api<Json>('/v1/definitions'), staleTime: 60_000 })
  const catalog = useMemo(() => buildCatalog(workspaces, views, session.objectId), [workspaces, views, session.objectId])
  const hits: PaletteItem[] = ((found.data?.hits as Json[] | undefined) ?? []).map((hit) => {
    const row = (hit.document as Json) ?? hit
    const type = String(row.type_key ?? row.type ?? '')
    const status = textValue(row.a_status)
    const version = String(row.version ?? '')
    const name = textValue(row.a_namn__sv)
    return {
      id: `hit:${String(row.id)}`,
      sourceId: String(row.id),
      group: 'search' as const,
      title: `${String(row.key ?? '')} · ${name || type}`,
      subtitle: 'Open the object',
      chips: [
        ...(type ? [{ text: typeLabel(defs.data, type, uiLanguage), tone: 'neutral' as const }] : []),
        ...(status ? [{ text: optionLabel(defs.data, 'status', status, uiLanguage), tone: statusTone(status) }] : []),
        ...(version ? [{ text: version, tone: 'neutral' as const }] : []),
      ],
    }
  })
  const rootRows = withModelCommand(query, visibleRows(query, catalog, hits, frecency))
  const rows = stack.at(-1)?.rows ?? rootRows
  const activeRow = active == null ? undefined : rows[active]

  useEffect(() => {
    if (stack.length > 0 || mode !== 'list') return
    const index = rows.findIndex((row) => row.sourceId === 'model:command')
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
      void api('/v1/frecency', { method: 'POST', body: JSON.stringify({ id: remembered }) }).catch(() => undefined)
    }
    if (id === 'model:command') {
      void api<Json>('/v1/operations/model.command', {
        method: 'POST',
        body: JSON.stringify({ text: query }),
      })
        .then((result) => {
          const proposalId = String(result.proposal_id ?? result.id ?? '')
          if (!proposalId) return
          publishProposal(proposalId)
          session.setProposalId(proposalId)
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
    if (id.startsWith('view:')) {
      dispatchUi('ui.open_view', { id: id.slice(5), zone: 'center' })
      onClose()
      return
    }
    if (id.startsWith('template:')) {
      const template = workspaceTemplates.find((item) => item.id === id.slice('template:'.length))
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
    if (id === 'theme:light' || id === 'theme:dark') {
      dispatchUi('ui.set_theme', { theme: id.endsWith('light') ? 'light' : 'dark' })
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
      await streamAgent(messages, (event) => applyEvent(event, setSteps, setAnswer, setProposal, session))
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
  const nothingElse = rows.length === 1 && rows[0]?.group === 'ask'
  const selected = nothingElse ? 0 : active

  return (
    <div className="palette" data-testid="palette" onMouseDown={onClose}>
      <Command
        className={mode === 'ai' ? 'ai-panel' : undefined}
        shouldFilter={false}
        loop={false}
        value={selected == null ? '' : rows[selected]?.id ?? ''}
        onMouseDown={(event) => event.stopPropagation()}
        onKeyDownCapture={onKeyDown}
      >
        <CommandInput
          data-testid="palette-input"
          autoFocus
          value={query}
          placeholder={mode === 'ai' ? 'Ask AI' : 'Search objects, open views, or ask the agent'}
          onValueChange={(value) => {
            setQuery(value)
            setActive(null)
          }}
        />
        {mode === 'ai' ? (
          <div className="toolbar">
            <span className="chip"><Sparkles aria-hidden="true" size={12} /> AI</span>
            <span className="spacer" />
            <button type="button" className="icon-btn" onClick={onClose}>Close</button>
          </div>
        ) : null}
        {stack.length > 0 ? <div className="crumb">{stack.map((crumb) => crumb.label).join(' / ')}</div> : null}
        {mode === 'ai' ? (
          <div className="palette-list" data-testid="ai-log">
            {turns.map((turn, index) => (
              <button key={`${turn.role}-${index}`} type="button" className="ai-turn" onClick={() => { setQuery(turn.text); setEditing(index) }}>
                {turn.text}
              </button>
            ))}
            {running ? <div className="progress" data-testid="ai-progress"><div className="progress-bar" /></div> : null}
            {steps.map((step) => (
              <div key={step.id} className={step.state === 'done' ? 'ai-step done' : 'ai-step'} data-testid="ai-step">
                {step.state === 'done' ? <Check size={14} /> : <span className="chip">…</span>}
                {step.text}
              </div>
            ))}
            {answer ? <p className="ai-turn" data-testid="ai-answer">{answer}</p> : null}
            {proposal ? (
              <div data-testid="palette-proposal">
                <strong>{proposal.title}</strong>
                {proposal.source ? <span className="chip">{proposal.source}</span> : null}
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
                  <button type="button" onClick={() => void api(`/v1/proposals/${proposal.id}/discard`, { method: 'POST' }).then(() => setProposal(null))}>Cancel</button>
                  <button type="button" className="primary" onClick={() => void api(`/v1/proposals/${proposal.id}/accept`, { method: 'POST', body: '{}' }).then(() => { session.setProposalId(proposal.id); setProposal(null); session.push('Confirm') })}>Confirm</button>
                </div>
              </div>
            ) : null}
          </div>
        ) : (
            <CommandList id="palette-list">
              {groups.map((group) => (
                <div key={group.name} className="palette-group">
                  {group.name !== 'ask' ? <div>{groupTitle[group.name]}</div> : null}
                  {group.rows.map((row) => {
                    const index = rows.indexOf(row)
                    return (
                      <CommandItem
                        key={row.id}
                        value={row.id}
                        className={index === selected ? `hit on${row.group === 'ask' ? ' ask' : ''}` : `hit${row.group === 'ask' ? ' ask' : ''}`}
                        onMouseEnter={() => setActive(index)}
                        onSelect={() => run(row)}
                      >
                        <span className="hit-copy">
                          <span className="hit-title">{highlight(row.title, query)}</span>
                          <small>{row.subtitle}</small>
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
            onMouseEnter={() => setActive(rows.indexOf(askRow))}
            onSelect={() => run(askRow)}
          >
            <span className="hit-copy">
              <span className="hit-title">{highlight(askRow.title, query)}</span>
              <small>{askRow.subtitle}</small>
            </span>
            {askRow.kbd ? <Kbd>{askRow.kbd}</Kbd> : null}
          </CommandItem>
        ) : null}
        <div className="palette-foot">
          <span>{activeRow ? activeRow.subtitle : '↵ run'}</span>
          <span><CornerDownLeft size={12} aria-hidden="true" /> run · Ctrl+↵ actions · Tab ask AI · Esc close</span>
        </div>
      </Command>
    </div>
  )
}

function buildCatalog(
  workspaces: Array<{ id: string; name: string; share_token?: string }>,
  views: Array<{ id: string; title: string; type: string }>,
  objectId: string | null,
): PaletteItem[] {
  const items: PaletteItem[] = [
    ...workspaces.map((workspace, index) => ({
      id: `workspace:${workspace.id}`,
      group: 'views' as const,
      title: workspaceDisplayName(workspace.name, workspace.share_token),
      subtitle: 'Open workspace',
      kbd: index < 9 ? `Ctrl+${index + 1}` : undefined,
      shortcut: index < 9 ? `Ctrl+${index + 1}` : undefined,
    })),
    ...views.map((view) => ({
      id: `view:${view.id}`,
      group: 'views' as const,
      title: viewDisplayTitle(view.title),
      subtitle: 'Open view',
    })),
    ...workspaceTemplates.map((template) => ({
      id: `template:${template.id}`,
      group: 'views' as const,
      title: template.name,
      subtitle: `Template · ${template.summary}`,
    })),
    { id: 'import:file', group: 'views', title: copy.importFile, subtitle: 'Drop an Excel or CSV file' },
    { id: 'theme:light', group: 'views', title: 'Light', subtitle: 'Switch theme', shortcut: undefined },
    { id: 'theme:dark', group: 'views', title: 'Dark', subtitle: 'Switch theme' },
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
  ]
  if (objectId) {
    items.push(
      { id: 'object:open', group: 'object', title: 'Open detail', subtitle: 'On the selected object', kbd: 'Enter' },
      { id: 'object:right', group: 'object', title: 'Open to the right', subtitle: 'Split beside the table' },
      { id: 'object:history', group: 'object', title: 'History', subtitle: 'As it was, and restore' },
      { id: 'object:graph', group: 'object', title: 'Graph', subtitle: 'Relations and where values come from' },
      { id: 'object:copy', group: 'object', title: 'Copy id', subtitle: objectId },
    )
  }
  return items
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
) {
  if (event.type === 'step') {
    const text = event.op ? stepText(event.op, event.args) : event.text ?? ''
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
    else if (event.name) {
      const op = uiOperations.find((name) => name.replaceAll('.', '_') === event.name) ?? event.name
      if (op.startsWith('ui.')) dispatchUi(op, (result.args as Json) ?? {})
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
  const body = await api<Json>(`/v1/proposals/${id}`).catch(() => null)
  const rows = ((body?.rows as Json[]) ?? [])
  const lines = ((body?.consequence as Json | undefined)?.lines as unknown[] | undefined)?.map(String) ?? []
  setProposal({
    id,
    title: String(body?.title ?? 'Proposal'),
    rows,
    lines,
    source: body?.source ? String(body.source) : undefined,
    error: rows.some((row) => row.conflict) ? 'Conflict' : undefined,
  })
  session.setProposalId(id)
  if (lines.length > 0) publishProposal(id)
}
