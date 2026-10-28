import { useEffect, useMemo, useRef, useState } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { api, textValue, type Json } from './api'
import { formatCount } from './format'
import { copy, uiLanguage } from './i18n'
import { openSharedContextMenu } from './shared-context-menu'
import { registerCommandRunner } from './command-registry'

type LinkItem = {
  id: string
  peer_id: string
  peer_key: string
  peer_type: string
  has_values: boolean
}

type LinkSide = {
  link_type: string
  label: string
  direction: 'out' | 'in'
  links: LinkItem[]
}

type Menu = { id: string; peer: string; hasValues: boolean; x: number; y: number }

let focused: { id: string; peer: string; hasValues: boolean } | null = null

export function LinkSections({
  objectId,
  typeKey,
  defs,
  links,
  onOpen,
}: {
  objectId: string
  typeKey: string
  defs?: Json
  links: LinkSide[]
  onOpen: (id: string) => void
}) {
  const queryClient = useQueryClient()
  const [adding, setAdding] = useState<LinkSide | null>(null)
  const [confirm, setConfirm] = useState<{ id: string; peer: string } | null>(null)
  const [typed, setTyped] = useState('')
  const sections = useMemo(() => sectionsFor(defs, typeKey, links), [defs, typeKey, links])
  const onOpenRef = useRef(onOpen)
  onOpenRef.current = onOpen
  const sectionsRef = useRef(sections)
  sectionsRef.current = sections
  const removeRef = useRef<(id: string, peer: string, hasValues: boolean) => void>(() => undefined)
  const refresh = () => {
    void queryClient.invalidateQueries({ queryKey: ['object', objectId] })
    void queryClient.invalidateQueries({ queryKey: ['objects'] })
  }
  const remove = (id: string, peer: string, hasValues: boolean) => {
    if (hasValues) {
      setConfirm({ id, peer })
      setTyped('')
      return
    }
    void api(`/v1/links/${id}`, { method: 'DELETE' }).then(refresh).catch(() => undefined)
  }
  removeRef.current = remove
  const openLinkMenu = (link: LinkItem, x: number, y: number) => {
    focused = { id: link.id, peer: link.peer_key, hasValues: link.has_values }
    openSharedContextMenu({
      context: 'link',
      x,
      y,
      target: {
        linkId: link.id,
        peerId: link.peer_id,
        objectId: link.peer_id,
        hasValues: link.has_values,
      },
    })
  }
  useEffect(() => {
    const stops = [
      registerCommandRunner('link:open-peer', (target) => {
        if (target.peerId) onOpenRef.current(String(target.peerId))
      }),
      registerCommandRunner('link:edit', (target) => {
        if (target.linkId) onOpenRef.current(String(target.linkId))
      }),
      registerCommandRunner('link:remove', (target) => {
        if (!target.linkId) return
        const id = String(target.linkId)
        const peer = peerKey(sectionsRef.current, id)
        removeRef.current(id, peer, Boolean(target.hasValues))
      }),
    ]
    return () => stops.forEach((stop) => stop())
  }, [])
  useEffect(() => {
    const add = (event: Event) => {
      const detail = (event as CustomEvent<{ linkType?: string }>).detail ?? {}
      const section = sections.find((item) => !detail.linkType || item.link_type === detail.linkType) ?? sections[0]
      if (section) setAdding(section)
    }
    const drop = () => {
      if (!focused) return
      remove(focused.id, focused.peer, focused.hasValues)
    }
    window.addEventListener('braian-add-link', add)
    window.addEventListener('braian-remove-link', drop)
    return () => {
      window.removeEventListener('braian-add-link', add)
      window.removeEventListener('braian-remove-link', drop)
    }
  }, [sections])
  return (
    <>
      {sections.map((section) => (
        <section
          key={`${section.link_type}:${section.direction}`}
          className="field-card"
          data-testid="link-section"
          aria-label={section.label}
        >
          <h3>{section.label} ({formatCount(section.links.length)})</h3>
          {section.links.map((link) => (
            <div
              key={link.id}
              className="row"
              role="row"
              tabIndex={0}
              data-testid="link-row"
              data-peer={link.peer_key}
              onFocus={() => {
                focused = { id: link.id, peer: link.peer_key, hasValues: link.has_values }
              }}
              onContextMenu={(event) => {
                event.preventDefault()
                openLinkMenu(link, event.clientX, event.clientY)
              }}
              onKeyDown={(event) => {
                if (event.key === 'Enter' && (event.ctrlKey || event.metaKey)) {
                  event.preventDefault()
                  const rect = event.currentTarget.getBoundingClientRect()
                  openLinkMenu(link, rect.left, rect.bottom)
                }
                if (event.key === 'Enter' && !event.ctrlKey && !event.metaKey) {
                  event.preventDefault()
                  onOpen(link.peer_id)
                }
              }}
            >
              <span>{link.peer_key}</span>
            </div>
          ))}
          <button
            type="button"
            data-testid="add-link"
            onClick={() => setAdding(section)}
          >
            {copy.addLink}
          </button>
        </section>
      ))}
      {adding ? (
        <LinkPicker
          section={adding}
          objectId={objectId}
          onClose={() => setAdding(null)}
          onCreated={() => {
            setAdding(null)
            refresh()
          }}
        />
      ) : null}
      {confirm ? (
        <div className="overlay" data-testid="link-confirm">
          <form
            className="dialog"
            role="dialog"
            aria-label={copy.removeLink}
            onSubmit={(event) => {
              event.preventDefault()
              if (typed.trim() !== confirm.peer) return
              void api(`/v1/links/${confirm.id}`, { method: 'DELETE' }).then(() => {
                setConfirm(null)
                refresh()
              }).catch(() => undefined)
            }}
          >
            <p>{copy.confirmRemoveLink}</p>
            <input aria-label={copy.confirmRemoveLink} value={typed} onChange={(event) => setTyped(event.target.value)} autoFocus />
            <div className="toolbar">
              <button type="button" onClick={() => setConfirm(null)}>{copy.cancel}</button>
              <button type="submit" className="primary" disabled={typed.trim() !== confirm.peer}>{copy.removeLink}</button>
            </div>
          </form>
        </div>
      ) : null}
    </>
  )
}

function LinkPicker({
  section,
  objectId,
  onClose,
  onCreated,
}: {
  section: LinkSide
  objectId: string
  onClose: () => void
  onCreated: () => void
}) {
  const [query, setQuery] = useState('')
  const [active, setActive] = useState(0)
  const found = useQuery({
    queryKey: ['link-search', query],
    queryFn: () => api<Json>(`/v1/search?q=${encodeURIComponent(query)}&per_page=8&lang=${uiLanguage}`),
    enabled: query.trim().length > 0,
  })
  const hits = (((found.data?.hits as Json[]) ?? [])
    .map((hit) => (hit.document as Json) ?? hit)
    .filter((row) => String(row.id ?? '') !== objectId))
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') onClose()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [onClose])
  const choose = (row: Json) => {
    const peer = String(row.id ?? '')
    if (!peer) return
    const body = section.direction === 'out'
      ? { type: section.link_type, from_id: objectId, to_id: peer, values: {} }
      : { type: section.link_type, from_id: peer, to_id: objectId, values: {} }
    void api('/v1/links', { method: 'POST', body: JSON.stringify(body) }).then(onCreated).catch(() => undefined)
  }
  return (
    <div className="menu" role="dialog" aria-label={copy.addLink} data-testid="link-picker">
      <input
        data-testid="link-search"
        autoFocus
        aria-label={copy.addLink}
        value={query}
        onChange={(event) => { setQuery(event.target.value); setActive(0) }}
        onKeyDown={(event) => {
          if (event.key === 'ArrowDown') {
            event.preventDefault()
            setActive((index) => Math.min(hits.length - 1, index + 1))
          }
          if (event.key === 'ArrowUp') {
            event.preventDefault()
            setActive((index) => Math.max(0, index - 1))
          }
          if (event.key === 'Enter') {
            event.preventDefault()
            const row = hits[active]
            if (row) choose(row)
          }
        }}
      />
      <div role="listbox">
        {hits.map((row, index) => (
          <button
            key={String(row.id)}
            type="button"
            role="option"
            aria-selected={index === active}
            className={index === active ? 'on' : ''}
            onClick={() => choose(row)}
          >
            {String(row.key ?? '')} {textValue(row.name) || textValue(row.a_name)}
          </button>
        ))}
      </div>
    </div>
  )
}

export function linkChoices(defs: Json | undefined, typeKey: string) {
  return sectionsFor(defs, typeKey, []).map((section) => ({
    key: section.link_type,
    label: section.label,
  }))
}

function sectionsFor(defs: Json | undefined, typeKey: string, links: LinkSide[]): LinkSide[] {
  const types = ((defs?.types as Json[]) ?? []).filter((item) => item.kind === 'link')
  const mine = types.filter((item) => {
    const spec = item.link as Json | undefined
    return idList(defs, spec?.from_types).includes(typeKey) || idList(defs, spec?.to_types).includes(typeKey)
  })
  const rows: LinkSide[] = []
  for (const item of mine) {
    const key = String(item.key ?? '')
    const spec = item.link as Json | undefined
    if (idList(defs, spec?.from_types).includes(typeKey)) {
      rows.push(existing(links, key, 'out', labelOf(spec?.to_label, key)))
    }
    if (idList(defs, spec?.to_types).includes(typeKey)) {
      rows.push(existing(links, key, 'in', labelOf(spec?.from_label, key)))
    }
  }
  return rows
}

function existing(links: LinkSide[], linkType: string, direction: 'out' | 'in', label: string): LinkSide {
  const found = links.find((item) => item.link_type === linkType && item.direction === direction)
  return found ? { ...found, label } : { link_type: linkType, label, direction, links: [] }
}

function idList(defs: Json | undefined, ids: unknown) {
  const types = (defs?.types as Json[]) ?? []
  return ((ids as unknown[]) ?? []).map((id) => {
    const match = types.find((item) => String(item.id) === String(id))
    return String(match?.key ?? id)
  })
}

function labelOf(label: unknown, fallback: string) {
  const map = (label as Json | undefined)?.labels
  return textValue(map as Json, uiLanguage) || textValue(map as Json, 'en') || fallback
}

function peerKey(sections: LinkSide[], id: string) {
  for (const section of sections) {
    const link = section.links.find((item) => item.id === id)
    if (link) return link.peer_key
  }
  return ''
}
