import { useEffect, useRef, useState, type KeyboardEvent as ReactKeyboardEvent } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { ApiError, api, type Json } from './api'
import { formatCount, formatTimestamp } from './format'
import { copy } from './i18n'
import { attributeKey } from './model-view'
import { onDraftChange, requestDiscardDraft, setModelFocus, stageModel } from './model-stage'

type DialogKind = 'type' | 'list' | 'family' | 'tree' | 'link'

export function ModelDraftHost() {
  const queryClient = useQueryClient()
  const draft = useQuery({ queryKey: ['model-draft'], queryFn: () => api.get('/v1/model/draft') })
  const [review, setReview] = useState(false)
  const [dialog, setDialog] = useState<DialogKind | null>(null)
  const [error, setError] = useState('')
  const opener = useRef<HTMLElement | null>(null)
  const rememberOpener = () => {
    opener.current = document.activeElement instanceof HTMLElement ? document.activeElement : null
  }
  const closeReview = () => {
    setReview(false)
    const target = opener.current
    opener.current = null
    if (target?.isConnected) queueMicrotask(() => target.focus())
  }
  useEffect(() => onDraftChange(() => {
    void queryClient.invalidateQueries({ queryKey: ['model-draft'] })
  }), [queryClient])
  useEffect(() => {
    const open = () => {
      setError('')
      if (Number(draft.data?.count ?? 0) > 0 && draft.data?.conflict !== true) {
        rememberOpener()
        setReview(true)
      }
    }
    const discard = () => {
      void api.delete('/v1/model/draft').then(() => {
        void queryClient.invalidateQueries({ queryKey: ['model-draft'] })
      }).catch(() => undefined)
    }
    const ask = (event: Event) => setDialog((event as CustomEvent<DialogKind>).detail)
    window.addEventListener('braian-review-save', open)
    window.addEventListener('braian-discard-draft', discard)
    window.addEventListener('braian-model-dialog', ask)
    return () => {
      window.removeEventListener('braian-review-save', open)
      window.removeEventListener('braian-discard-draft', discard)
      window.removeEventListener('braian-model-dialog', ask)
    }
  }, [draft.data, queryClient])
  const count = Number(draft.data?.count ?? 0)
  const conflict = draft.data?.conflict === true
  const refresh = () => {
    void queryClient.invalidateQueries({ queryKey: ['model-draft'] })
    void queryClient.invalidateQueries({ queryKey: ['definitions'] })
  }
  return (
    <>
      {count > 0 ? (
        <div className="draft-bar" data-testid="unsaved-changes" role="status">
          <span>{formatCount(count)} {copy.unsavedChanges}</span>
          {draft.data?.updated_at ? <time dateTime={String(draft.data.updated_at)}>{formatTimestamp(draft.data.updated_at)}</time> : null}
          {conflict ? <span>{copy.draftConflict} {draft.data?.updated_at ? formatTimestamp(draft.data.updated_at) : ''}</span> : null}
          {conflict ? (
            <button
              type="button"
              onClick={() => {
                void api.post('/v1/model/draft/rebase').then(refresh).catch((reason: unknown) => {
                  setError(reason instanceof ApiError ? reason.message : copy.validation)
                })
              }}
            >
              {copy.rebaseDraft}
            </button>
          ) : (
            <button type="button" data-testid="review-save" onClick={() => { rememberOpener(); setReview(true) }}>{copy.reviewAndSave}</button>
          )}
          <button type="button" onClick={() => requestDiscardDraft()}>{copy.discardDraft}</button>
          {error ? <span className="error">{error}</span> : null}
        </div>
      ) : null}
      {review && draft.data ? (
        <ReviewDialog
          body={draft.data}
          onClose={closeReview}
          onSaved={() => {
            setReview(false)
            refresh()
          }}
        />
      ) : null}
      {dialog ? <CreateDialog kind={dialog} onClose={() => setDialog(null)} /> : null}
    </>
  )
}

function ReviewDialog({ body, onClose, onSaved }: { body: Json; onClose: () => void; onSaved: () => void }) {
  const consequence = (body.consequence as Json | undefined) ?? {}
  const lines = (body.lines as string[] | undefined) ?? ((consequence.lines as string[] | undefined) ?? [])
  const warnings = (body.warnings as string[] | undefined) ?? []
  const destructive = consequence.destructive === true
  const confirmName = String(consequence.confirm_name ?? '')
  const [typed, setTyped] = useState('')
  const [error, setError] = useState('')
  const [busy, setBusy] = useState(false)
  const nameMatches = !destructive || typed === confirmName
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return
      event.preventDefault()
      event.stopPropagation()
      onClose()
    }
    window.addEventListener('keydown', onKey, true)
    return () => window.removeEventListener('keydown', onKey, true)
  }, [onClose])
  const onDialogKey = (event: ReactKeyboardEvent<HTMLDivElement>) => {
    if (event.key !== 'Escape') return
    event.preventDefault()
    event.stopPropagation()
    onClose()
  }
  const save = () => {
    setBusy(true)
    void api.post('/v1/model/draft/save', { body: destructive ? { confirm_name: typed } : {} })
      .then(() => onSaved())
      .catch((reason: unknown) => setError(reason instanceof ApiError ? reason.message : copy.validation))
      .finally(() => setBusy(false))
  }
  return (
    <div className="overlay" data-testid="review-dialog">
      <div className="dialog consequence" role="dialog" aria-labelledby="review-title" onMouseDown={(event) => event.stopPropagation()} onKeyDown={onDialogKey}>
        <div className="toolbar">
          <strong id="review-title">{copy.reviewAndSave}</strong>
        </div>
        <p className="hint">{copy.reviewChange}</p>
        {body.error ? <p className="error">{String(body.error)}</p> : null}
        <ul className="proposal-tree" data-testid="review-lines">
          {lines.map((line) => <li key={line}>{line}</li>)}
        </ul>
        {warnings.map((warning) => <p key={warning} className="error">{warning}</p>)}
        {destructive ? (
          <label className="model-field">
            {copy.typeNameToConfirm}
            <input
              data-testid="review-confirm-name"
              autoFocus
              value={typed}
              aria-label={copy.typeNameToConfirm}
              onChange={(event) => setTyped(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === 'Enter' && nameMatches && !busy) {
                  event.preventDefault()
                  save()
                }
              }}
            />
          </label>
        ) : null}
        {error ? <p className="error">{error}</p> : null}
        <div className="toolbar">
          <button type="button" onClick={onClose}>{copy.cancel}</button>
          <button type="button" className="primary" data-testid="confirm-review" disabled={busy || !nameMatches} onClick={save}>{copy.confirm}</button>
        </div>
      </div>
    </div>
  )
}

function CreateDialog({ kind, onClose }: { kind: DialogKind; onClose: () => void }) {
  const [label, setLabel] = useState('')
  const [extra, setExtra] = useState('')
  const [fromType, setFromType] = useState('')
  const [toType, setToType] = useState('')
  const [error, setError] = useState('')
  const defs = useQuery({ queryKey: ['definitions'], queryFn: () => api.get('/v1/definitions'), enabled: kind === 'link' })
  const entities = ((defs.data?.types as Json[] | undefined) ?? [])
    .filter((item) => item.kind !== 'link')
    .map((item) => String(item.key ?? ''))
    .filter(Boolean)
  useEffect(() => {
    if (kind !== 'link') return
    if (!fromType && entities[0]) setFromType(entities[0])
    if (!toType && entities[0]) setToType(entities[0])
  }, [kind, fromType, toType, entities])
  const title = kind === 'link' ? copy.newLinkType : kind === 'type' ? copy.newType : kind === 'list' ? copy.newValueList : kind === 'family' ? copy.newUnitFamily : copy.newTree
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.preventDefault()
        onClose()
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [onClose])
  const submit = () => {
    const name = label.trim()
    const key = attributeKey(name)
    if (!key) return
    const side = attributeKey(name)
    if (kind === 'link') setModelFocus({ kind: 'type', key })
    const work = kind === 'link'
      ? stageModel('model.create_link_type', {
          key,
          label: name,
          from_types: [fromType],
          to_types: [toType],
          from_key: side,
          from_label: name,
          to_key: `${side}_to`,
          to_label: name,
          from_cardinality: 'many',
          to_cardinality: 'many',
          attributes: extra.split(',').map((item) => attributeKey(item)).filter(Boolean),
        })
      : kind === 'type'
      ? stageModel('model.create_type', { key, label: name })
      : kind === 'list'
        ? stageModel('model.create_value_list', {
            key,
            label: name,
            options: extra.split(',').map((item) => item.trim()).filter(Boolean).map((item) => ({
              code: attributeKey(item),
              label: item,
            })),
          })
        : kind === 'family'
          ? stageModel('model.create_unit_family', {
              key,
              dimension: name,
              base_unit: extra.trim() || key,
              units: [{ code: extra.trim() || key, factor: '1' }],
            })
          : stageModel('model.create_structure', { key, label: name })
    void work.then(() => onClose()).catch((reason: unknown) => {
      setError(reason instanceof ApiError ? reason.message : copy.validation)
    })
  }
  return (
    <div className="overlay" data-testid="model-dialog">
      <form
        className="dialog"
        role="dialog"
        aria-labelledby="model-dialog-title"
        onMouseDown={(event) => event.stopPropagation()}
        onSubmit={(event) => {
          event.preventDefault()
          submit()
        }}
      >
        <strong id="model-dialog-title">{title}</strong>
        <label className="model-field">
          {copy.attributeName}
          <input data-testid="model-dialog-name" autoFocus aria-label={copy.attributeName} value={label} onChange={(event) => setLabel(event.target.value)} />
        </label>
        {kind === 'link' ? (
          <>
            <label className="model-field">
              {copy.fromType}
              <select data-testid="link-type-from" aria-label={copy.fromType} value={fromType} onChange={(event) => setFromType(event.target.value)}>
                {entities.map((key) => <option key={key} value={key}>{key}</option>)}
              </select>
            </label>
            <label className="model-field">
              {copy.toType}
              <select data-testid="link-type-to" aria-label={copy.toType} value={toType} onChange={(event) => setToType(event.target.value)}>
                {entities.map((key) => <option key={key} value={key}>{key}</option>)}
              </select>
            </label>
            <label className="model-field">
              {copy.linkAttribute}
              <input data-testid="link-type-attribute" aria-label={copy.linkAttribute} value={extra} onChange={(event) => setExtra(event.target.value)} />
            </label>
          </>
        ) : null}
        {kind === 'list' ? (
          <label className="model-field">
            {copy.options}
            <input data-testid="model-dialog-options" aria-label={copy.options} value={extra} onChange={(event) => setExtra(event.target.value)} />
          </label>
        ) : null}
        {kind === 'family' ? (
          <label className="model-field">
            {copy.baseUnit}
            <input data-testid="model-dialog-unit" aria-label={copy.baseUnit} value={extra} onChange={(event) => setExtra(event.target.value)} />
          </label>
        ) : null}
        {error ? <p className="error">{error}</p> : null}
        <div className="toolbar">
          <button type="button" onClick={onClose}>{copy.cancel}</button>
          <button type="submit" className="primary" data-testid="model-dialog-save">{copy.save}</button>
        </div>
      </form>
    </div>
  )
}
