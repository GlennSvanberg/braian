import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { api, catalogName, type Json } from './api'
import { Button } from './components/ui/button'
import { Input } from './components/ui/input'
import { assortmentDisplayName } from './demo-labels'
import { searchDocuments, structureCaption } from './graph-scene'
import { copy } from './i18n'
import { publishProposal } from './proposal-bus'

type AssortmentSession = {
  group: string | null
  listType: string | null
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

function filterOf(query: Json) {
  const parts = []
  if (query.type) parts.push(`type_key:=${String(query.type)}`)
  if (query.group) parts.push(`groups:=${String(query.group)}`)
  return parts.join(' && ')
}

export function AssortmentList({ session }: { session: AssortmentSession }) {
  const list = useQuery({
    queryKey: ['assortments'],
    queryFn: () => api<{ assortments: Json[] }>('/v1/assortments'),
  })
  const defs = useQuery({ queryKey: ['definitions'], queryFn: () => api<Json>('/v1/definitions') })
  const items = list.data?.assortments ?? []
  const [openId, setOpenId] = useState<string | null>(null)
  const [name, setName] = useState('')
  const open = items.find((item) => String(item.id) === openId)
  const query = (open?.query as Json) ?? {}
  const filter = open ? filterOf(query) : ''
  const matches = useQuery({
    queryKey: ['assortment-matches', openId, filter],
    queryFn: () => api<Json>(`/v1/search?${new URLSearchParams({ filter, per_page: '20' })}`),
    enabled: Boolean(open),
  })
  const hits = searchDocuments(matches.data ?? {})
  const found = Number(matches.data?.found ?? hits.length)

  const save = async () => {
    const title = name.trim()
    if (!title) return
    if (!session.group && !session.listType) {
      session.push(copy.saveAssortmentNeeds)
      return
    }
    const result = await api<Json>('/v1/operations/assortment.save', {
      method: 'POST',
      body: JSON.stringify({
        name: title,
        query: {
          ...(session.listType ? { type: session.listType } : {}),
          ...(session.group ? { group: session.group } : {}),
        },
      }),
    })
    const id = String(result.proposal_id ?? result.id ?? '')
    if (id) {
      publishProposal(id)
      session.setProposalId(id)
    }
    session.push(copy.proposed)
    setName('')
  }

  return (
    <div className="assortment-list" data-testid="assortment-list">
      <form
        className="assortment-save"
        onSubmit={(event) => {
          event.preventDefault()
          void save()
        }}
      >
        <Input
          data-testid="assortment-name"
          value={name}
          placeholder={copy.assortment}
          onChange={(event) => setName(event.target.value)}
        />
        <Button type="submit">{copy.saveAssortment}</Button>
      </form>
      {items.length === 0 ? <p className="hint">{copy.noAssortments}</p> : null}
      <div className="assortment-picks">
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
            {query.type ? <span className="filter-chip">{String(query.type)}</span> : null}
            {query.group ? <span className="filter-chip">{groupChip(String(query.group), defs.data)}</span> : null}
            {query.text ? <span className="filter-chip">{String(query.text)}</span> : null}
          </div>
          <p className="hint">
            {found} {copy.matchingObjects}
          </p>
          <div className="assortment-hits">
            {hits.map((hit) => {
              const id = String(hit.id ?? '')
              const key = String(hit.key ?? id)
              const label = catalogName(hit) || key
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
