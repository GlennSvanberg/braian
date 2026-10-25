import { useEffect, useState } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { api, type Json } from './api'
import { copy } from './i18n'
import { affectedLabel, typeNames } from './type-labels'
import { onProposal } from './proposal-bus'
import { StateNote } from './states'

type HostSession = {
  setProposalId: (value: string | null) => void
  push: (line: string) => void
}

export function ConsequenceHost({ session }: { session: HostSession }) {
  const queryClient = useQueryClient()
  const [id, setId] = useState<string | null>(null)
  const [error, setError] = useState('')
  const [busy, setBusy] = useState(false)
  useEffect(() => onProposal((proposal) => {
    setError('')
    setId(proposal.id)
  }), [])
  const defs = useQuery({ queryKey: ['definitions'], queryFn: () => api<Json>('/v1/definitions'), staleTime: 60_000 })
  const proposal = useQuery({
    queryKey: ['proposal', id],
    queryFn: () => api<Json>(`/v1/proposals/${id}`),
    enabled: Boolean(id),
  })
  useEffect(() => {
    if (!id) return
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.preventDefault()
        setId(null)
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [id])
  if (!id) return null
  const body = proposal.data
  const consequence = (body?.consequence as Json | undefined) ?? {}
  const lines = (consequence.lines as string[] | undefined) ?? []
  const samples = (consequence.samples as Json[] | undefined) ?? []
  const jobs = (consequence.jobs as Json[] | undefined) ?? []
  const errors = (consequence.definition_errors as string[] | undefined) ?? []
  const close = () => setId(null)
  const discard = () => {
    setBusy(true)
    void api(`/v1/proposals/${id}/discard`, { method: 'POST' })
      .then(() => {
        session.push(copy.cancel)
        void queryClient.invalidateQueries({ queryKey: ['proposals'] })
        close()
      })
      .catch((reason: unknown) => setError(reason instanceof Error ? reason.message : copy.validation))
      .finally(() => setBusy(false))
  }
  const confirm = () => {
    setBusy(true)
    void api<Json>(`/v1/proposals/${id}/accept`, { method: 'POST', body: '{}' })
      .then((result) => {
        const failed = (result.failed as Json[] | undefined) ?? []
        if (failed.length > 0) {
          setError(String(failed[0]?.detail ?? copy.validation))
          return
        }
        session.setProposalId(id)
        session.push(copy.confirm)
        void queryClient.invalidateQueries({ queryKey: ['definitions'] })
        void queryClient.invalidateQueries({ queryKey: ['proposals'] })
        void queryClient.invalidateQueries({ queryKey: ['proposal', id] })
        close()
      })
      .catch((reason: unknown) => setError(reason instanceof Error ? reason.message : copy.validation))
      .finally(() => setBusy(false))
  }
  return (
    <div className="overlay" data-testid="consequence">
      <div className="dialog consequence" role="dialog" aria-labelledby="consequence-title" onMouseDown={(event) => event.stopPropagation()}>
        <div className="toolbar">
          <strong id="consequence-title">{String(body?.title ?? copy.dataModel)}</strong>
          {body?.source ? <span className="chip">{String(body.source)}</span> : null}
        </div>
        <p className="hint">{copy.reviewChange}</p>
        {proposal.isPending ? <StateNote tone="loading" title={copy.loadingModel} /> : null}
        {proposal.isError ? <StateNote tone="error" title={copy.errorModel} /> : null}
        {lines.length > 0 ? (
          <>
            <p className="hint">{copy.modelDiff}</p>
            <ul className="proposal-tree" data-testid="model-diff">
              {lines.map((line) => (
                <li key={line}>{line}</li>
              ))}
            </ul>
          </>
        ) : null}
        <div data-testid="impact-result">
          <p>
            {affectedLabel(((body?.rows as Json[] | undefined) ?? []).map((row) => String(row.type_key ?? '')), typeNames(defs.data))}: {String(consequence.affected_objects ?? 0)}
          </p>
          <p>
            {copy.samples}: {samples.map((item) => String(item.key ?? item.id ?? '')).filter(Boolean).join(', ') || '—'}
          </p>
          {jobs.length === 0 ? <p>{copy.noJob}</p> : null}
          {jobs.map((job) => (
            <p key={String(job.key)}>
              {String(job.title)} · {((job.reads_fields as string[]) ?? []).join(', ')}
            </p>
          ))}
        </div>
        {errors.map((item) => (
          <p key={item} className="error">{item}</p>
        ))}
        {error ? <p className="error">{error}</p> : null}
        <div className="toolbar">
          <button type="button" onClick={discard} disabled={busy}>{copy.cancel}</button>
          <button type="button" className="primary" data-testid="confirm-model" onClick={confirm} disabled={busy || errors.length > 0}>
            {copy.confirm}
          </button>
        </div>
      </div>
    </div>
  )
}
