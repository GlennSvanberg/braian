import { useEffect, useState } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { ApiError, api, type Json } from './api'
import { formatCount, formatTimestamp } from './format'
import { copy } from './i18n'
import { attributeKey } from './model-view'
import { onDraftChange, requestDiscardDraft, stageModel } from './model-stage'

type DialogKind = 'type' | 'list' | 'family' | 'tree'

export function ModelDraftHost() {
  const queryClient = useQueryClient()
  const draft = useQuery({ queryKey: ['model-draft'], queryFn: () => api<Json>('/v1/model/draft') })
  const [review, setReview] = useState(false)
  const [dialog, setDialog] = useState<DialogKind | null>(null)
  const [error, setError] = useState('')
  useEffect(() => onDraftChange(() => {
    void queryClient.invalidateQueries({ queryKey: ['model-draft'] })
  }), [queryClient])
  useEffect(() => {
    const open = () => {
      setError('')
      if (Number(draft.data?.count ?? 0) > 0 && draft.data?.conflict !== true) setReview(true)
    }
    const discard = () => {
      void api('/v1/model/draft', { method: 'DELETE' }).then(() => {
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
                void api('/v1/model/draft/rebase', { method: 'POST' }).then(refresh).catch((reason: unknown) => {
                  setError(reason instanceof ApiError ? reason.message : copy.validation)
                })
              }}
            >
              {copy.rebaseDraft}
            </button>
          ) : (
            <button type="button" data-testid="review-save" onClick={() => setReview(true)}>{copy.reviewAndSave}</button>
          )}
          <button type="button" onClick={() => requestDiscardDraft()}>{copy.discardDraft}</button>
          {error ? <span className="error">{error}</span> : null}
        </div>
      ) : null}
      {review && draft.data ? (
        <ReviewDialog
          body={draft.data}
          onClose={() => setReview(false)}
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
      if (event.key === 'Escape') {
        event.preventDefault()
        onClose()
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [onClose])
  const save = () => {
    setBusy(true)
    void api('/v1/model/draft/save', {
      method: 'POST',
      body: JSON.stringify(destructive ? { confirm_name: typed } : {}),
    })
      .then(() => onSaved())
      .catch((reason: unknown) => setError(reason instanceof ApiError ? reason.message : copy.validation))
      .finally(() => setBusy(false))
  }
  return (
    <div className="overlay" data-testid="review-dialog">
      <div className="dialog consequence" role="dialog" aria-labelledby="review-title" onMouseDown={(event) => event.stopPropagation()}>
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
  const [error, setError] = useState('')
  const title = kind === 'type' ? copy.newType : kind === 'list' ? copy.newValueList : kind === 'family' ? copy.newUnitFamily : copy.newTree
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
    const work = kind === 'type'
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
