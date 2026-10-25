import { useEffect, useMemo, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { barY, defineChart } from '@tanstack/charts'
import { scaleBand } from '@tanstack/charts/scales/band'
import { scaleLinear } from '@tanstack/charts/scales/linear'
import { Chart } from '@tanstack/react-charts'
import { api, type Json } from './api'
import { Button } from './components/ui/button'
import { Input } from './components/ui/input'
import { copy } from './i18n'

type UsageRow = {
  task_type: string
  tasks: number
  human_time_ms: number
  changes_human: number
  changes_agent: number
  changes_integration: number
}

type TrendRow = { day: string; task_type: string; tasks: number; human_time_ms: number }

const TYPE_LABEL: Record<string, string> = {
  import: copy.taskImport,
  mass_edit: copy.taskMassEdit,
  enrich: copy.taskEnrich,
  search: copy.taskSearch,
  navigate: copy.taskNavigate,
  review: copy.taskReview,
  model: copy.taskModel,
  agent: copy.taskAgent,
  other: copy.taskOther,
}

function label(type: string) {
  return TYPE_LABEL[type] ?? type
}

function formatTime(ms: number) {
  const minutes = Math.max(0, Math.round(ms / 60_000))
  if (minutes < 60) return `${minutes} min`
  const hours = Math.floor(minutes / 60)
  const rest = minutes % 60
  return rest === 0 ? `${hours} h` : `${hours} h ${rest} min`
}

export function UsageView() {
  const queryClient = useQueryClient()
  const usage = useQuery({
    queryKey: ['usage'],
    queryFn: () => api<Json>('/v1/usage?days=30'),
  })
  const rows = ((usage.data?.by_type as Json[] | undefined) ?? []).map((row) => ({
    task_type: String(row.task_type),
    tasks: Number(row.tasks ?? 0),
    human_time_ms: Number(row.human_time_ms ?? 0),
    changes_human: Number(row.changes_human ?? 0),
    changes_agent: Number(row.changes_agent ?? 0),
    changes_integration: Number(row.changes_integration ?? 0),
  })) satisfies UsageRow[]
  const trend = ((usage.data?.trend as Json[] | undefined) ?? []).map((row) => ({
    day: String(row.day),
    task_type: String(row.task_type),
    tasks: Number(row.tasks ?? 0),
    human_time_ms: Number(row.human_time_ms ?? 0),
  })) satisfies TrendRow[]
  const share = (usage.data?.share as Json | undefined) ?? {}
  const human = Number(share.human ?? 0)
  const agent = Number(share.agent ?? 0)
  const integration = Number(share.integration ?? 0)
  const total = human + agent + integration
  const chartRows = rows.map((row) => ({ type: label(row.task_type), minutes: Math.round(row.human_time_ms / 60_000) }))
  const definition = useMemo(
    () =>
      defineChart({
        marks: [barY(chartRows, { x: 'type', y: 'minutes' })],
        scales: {
          x: { scale: () => scaleBand().padding(0.2) },
          y: { scale: scaleLinear, nice: true },
        },
      }),
    [chartRows],
  )
  return (
    <div className="usage" data-testid="usage">
      <p className="usage-note" data-testid="usage-note">{copy.usageMeasured}</p>
      <p className="hint">{copy.usageWindow}</p>
      {rows.length > 0 ? <Chart definition={definition} height={180} ariaLabel={copy.usageTime} /> : <p>{copy.usageEmpty}</p>}
      <table>
        <thead>
          <tr>
            <th>{copy.usageTaskType}</th>
            <th className="num">{copy.usageTasks}</th>
            <th className="num">{copy.usageTime}</th>
            <th className="num">{copy.usageHuman}</th>
            <th className="num">{copy.usageAgent}</th>
            <th className="num">{copy.usageIntegration}</th>
          </tr>
        </thead>
        <tbody>
          {rows.map((row) => (
            <tr key={row.task_type}>
              <td>{label(row.task_type)}</td>
              <td className="num">{row.tasks}</td>
              <td className="num">{formatTime(row.human_time_ms)}</td>
              <td className="num">{row.changes_human}</td>
              <td className="num">{row.changes_agent}</td>
              <td className="num">{row.changes_integration}</td>
            </tr>
          ))}
        </tbody>
      </table>
      <div>
        <p className="hint">{copy.usageShare}</p>
        <div className="usage-share" aria-hidden="true">
          <span className="share-human" style={{ width: total ? `${(human / total) * 100}%` : '0%' }} />
          <span className="share-agent" style={{ width: total ? `${(agent / total) * 100}%` : '0%' }} />
          <span className="share-integration" style={{ width: total ? `${(integration / total) * 100}%` : '0%' }} />
        </div>
        <div className="usage-legend">
          <span>{copy.usageHuman} {human}</span>
          <span>{copy.usageAgent} {agent}</span>
          <span>{copy.usageIntegration} {integration}</span>
        </div>
      </div>
      <div>
        <p className="hint">{copy.usageTrend}</p>
        {trend.length === 0 ? <p>{copy.usageEmpty}</p> : (
          <table>
            <thead>
              <tr>
                <th>{copy.usageDay}</th>
                <th>{copy.usageTaskType}</th>
                <th className="num">{copy.usageTasks}</th>
                <th className="num">{copy.usageTime}</th>
              </tr>
            </thead>
            <tbody>
              {trend.map((row) => (
                <tr key={`${row.day}-${row.task_type}`}>
                  <td>{row.day}</td>
                  <td>{label(row.task_type)}</td>
                  <td className="num">{row.tasks}</td>
                  <td className="num">{formatTime(row.human_time_ms)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>
      <Settings
        retention={Number(usage.data?.retention_days ?? 30)}
        gap={Number(usage.data?.inactivity_gap_seconds ?? 300)}
        onSaved={() => void queryClient.invalidateQueries({ queryKey: ['usage'] })}
      />
    </div>
  )
}

function Settings({ retention, gap, onSaved }: { retention: number; gap: number; onSaved: () => void }) {
  const [days, setDays] = useState(String(retention))
  const [seconds, setSeconds] = useState(String(gap))
  const [error, setError] = useState('')
  useEffect(() => {
    setDays(String(retention))
    setSeconds(String(gap))
  }, [retention, gap])
  const save = useMutation({
    mutationFn: () =>
      api('/v1/usage/settings', {
        method: 'PUT',
        body: JSON.stringify({
          raw_retention_days: Number(days),
          inactivity_gap_seconds: Number(seconds),
        }),
      }),
    onSuccess: () => {
      setError('')
      onSaved()
    },
    onError: (reason: unknown) => setError(reason instanceof Error ? reason.message : copy.validation),
  })
  return (
    <form
      className="usage-settings"
      onSubmit={(event) => {
        event.preventDefault()
        save.mutate()
      }}
    >
      <label>
        {copy.usageRetention}
        <Input value={days} inputMode="numeric" onChange={(event) => setDays(event.target.value)} />
      </label>
      <label>
        {copy.usageGap}
        <Input value={seconds} inputMode="numeric" onChange={(event) => setSeconds(event.target.value)} />
      </label>
      <Button type="submit">{copy.usageSave}</Button>
      {error ? <p className="error">{error}</p> : null}
    </form>
  )
}
