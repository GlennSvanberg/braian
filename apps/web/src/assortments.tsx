import { useEffect, useState } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { api, catalogName, type Json } from './api'
import { formatCount } from './format'
import {
  currentSelectionLabels,
  labelCodes,
  NEW_SELECTION_EVENT,
  assortmentSearchFilter,
  requestNewSelection,
  selectionMatchesType,
} from './assortment-filter'
import { Button } from './components/ui/button'
import { openSharedContextMenu } from './shared-context-menu'
import { Dialog, DialogContent, DialogTitle } from './components/ui/dialog'
import { Input } from './components/ui/input'
import { assortmentDisplayName } from './demo-labels'
import { searchDocuments, structureCaption } from './graph-scene'
import { copy, uiLanguage } from './i18n'
import { publishProposal } from './proposal-bus'
import { typeNames } from './type-labels'

type SelectionSession = {
  group: string | null
  workspaceType?: string | null
  listType?: string | null
  setGroup: (value: string | null) => void
  setObjectId: (value: string | null) => void
  setProposalId: (value: string | null) => void
  push: (line: string) => void
}

function groupChip(group: string, defs?: Json) {
  const splitAt = group.indexOf(':')
  const key = splitAt >= 0 ? group.slice(splitAt + 1) : group
  const structure = splitAt > 0 ? group.slice(0, splitAt) : undefined
  return structureCaption(key, defs, structure)
}

function selectionQuery(session: SelectionSession): Json {
  const labels = currentSelectionLabels()
  return {
    ...(session.group ? { group: session.group } : {}),
    ...(labels.length === 1 ? { labels: labels[0] } : {}),
    ...(labels.length > 1 ? { labels } : {}),
  }
}

export function NewSelectionDialog({ session }: { session: SelectionSession }) {
  const defs = useQuery({ queryKey: ['definitions'], queryFn: () => api<Json>('/v1/definitions') })
  const types = typeNames(defs.data, uiLanguage)
  const firstType = types[0]?.key ?? ''
  const workspaceType = session.workspaceType || session.listType || ''
  const [open, setOpen] = useState(false)
  const [name, setName] = useState('')
  const [typeKey, setTypeKey] = useState(workspaceType)
  const queryClient = useQueryClient()

  useEffect(() => {
    if (open && !typeKey && firstType) setTypeKey(firstType)
  }, [open, typeKey, firstType])

  useEffect(() => {
    const onOpen = () => {
      setName('')
      setTypeKey(workspaceType || firstType)
      setOpen(true)
    }
    window.addEventListener(NEW_SELECTION_EVENT, onOpen)
    return () => window.removeEventListener(NEW_SELECTION_EVENT, onOpen)
  }, [workspaceType, firstType])

  const save = async () => {
    const title = name.trim()
    const chosen = typeKey || workspaceType
    if (!title || !chosen) return
    const result = await api<Json>('/v1/operations/assortment.save', {
      method: 'POST',
      body: JSON.stringify({ name: title, type: chosen, query: selectionQuery(session) }),
    })
    const id = String(result.proposal_id ?? result.id ?? '')
    if (id) {
      publishProposal(id)
      session.setProposalId(id)
    }
    session.push(copy.proposed)
    setOpen(false)
    setName('')
    void queryClient.invalidateQueries({ queryKey: ['assortments'] })
  }

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogContent data-testid="new-selection" aria-describedby={undefined}>
        <DialogTitle>{copy.newSelection}</DialogTitle>
        <form
          onSubmit={(event) => {
            event.preventDefault()
            void save()
          }}
        >
          <Input
            data-testid="assortment-name"
            value={name}
            autoFocus
            aria-label={copy.assortment}
            placeholder={copy.assortment}
            onChange={(event) => setName(event.target.value)}
          />
          <label className="field">
            <span>{copy.type}</span>
            <select
              aria-label={copy.type}
              data-testid="selection-type"
              value={typeKey}
              onChange={(event) => setTypeKey(event.target.value)}
            >
              {types.map((type) => (
                <option key={type.key} value={type.key}>{type.plural || type.key}</option>
              ))}
            </select>
          </label>
          <div className="dialog-actions">
            <Button type="submit">{copy.saveAssortment}</Button>
          </div>
        </form>
      </DialogContent>
    </Dialog>
  )
}

export function AssortmentList({ session }: { session: SelectionSession }) {
  const typeKey = session.workspaceType || session.listType || ''
  const defs = useQuery({ queryKey: ['definitions'], queryFn: () => api<Json>('/v1/definitions') })
  const typeId = typeNames(defs.data, uiLanguage).find((type) => type.key === typeKey)?.id ?? ''
  const list = useQuery({
    queryKey: ['assortments', typeKey],
    queryFn: () => api<{ assortments: Json[] }>(`/v1/assortments?type=${encodeURIComponent(typeKey)}`),
    enabled: Boolean(typeKey),
  })
  const items = (list.data?.assortments ?? []).filter((item) => selectionMatchesType(item, typeKey, typeId))
  const [openId, setOpenId] = useState<string | null>(null)
  const open = items.find((item) => String(item.id) === openId)
  const query = (open?.query as Json) ?? {}
  const filter = open ? assortmentSearchFilter(query, String(open.type ?? typeKey), defs.data) : ''
  const matches = useQuery({
    queryKey: ['assortment-matches', openId, filter],
    queryFn: () => api<Json>(`/v1/search?${new URLSearchParams({ filter, per_page: '20' })}`),
    enabled: Boolean(open && filter),
  })
  const hits = searchDocuments(matches.data ?? {})
  const found = Number(matches.data?.found ?? hits.length)

  return (
    <div className="assortment-list" data-testid="assortment-list">
      <div className="toolbar">
        <Button type="button" data-testid="new-selection-button" onClick={() => requestNewSelection()}>
          {copy.newSelection}
        </Button>
      </div>
      {items.length === 0 ? <p className="hint">{copy.noAssortments}</p> : null}
      <div
        className="assortment-picks"
        tabIndex={0}
        onContextMenu={(event) => {
          event.preventDefault()
          openSharedContextMenu({ context: 'assortment', x: event.clientX, y: event.clientY, target: {} })
        }}
      >
        {items.map((item) => {
          const id = String(item.id)
          const selected = id === openId
          return (
            <button
              key={id}
              type="button"
              data-testid="assortment-row"
              className={selected ? 'on' : ''}
              onClick={() => {
                setOpenId(id)
                const next = (item.query as Json) ?? {}
                session.setGroup(next.group ? String(next.group) : null)
              }}
            >
              {assortmentDisplayName(String(item.name ?? ''))}
            </button>
          )
        })}
      </div>
      {open ? (
        <div data-testid="assortment-open">
          <div className="assortment-query">
            {open.type ? <span className="filter-chip">{String(open.type)}</span> : null}
            {query.group ? <span className="filter-chip">{groupChip(String(query.group), defs.data)}</span> : null}
            {labelCodes(query.labels).map((code) => (
              <span key={code} className="filter-chip">{code}</span>
            ))}
          </div>
          <p className="hint">
            {formatCount(found)} {copy.matchingObjects}
          </p>
          <div className="assortment-hits">
            {hits.map((hit) => {
              const id = String(hit.id ?? '')
              const key = String(hit.key ?? id)
              const label = catalogName(hit, defs.data) || key
              return (
                <button key={id || key} type="button" className="row" onClick={() => session.setObjectId(id)}>
                  <span>{key}</span>
                  <span>{label}</span>
                </button>
              )
            })}
          </div>
        </div>
      ) : null}
    </div>
  )
}
