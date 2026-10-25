import type { DiffLine } from './field-diff'

/** One changed field per line. Old value is struck through; the new value follows it. */
export function FieldDiff({ lines }: { lines: DiffLine[] }) {
  if (lines.length === 0) return null
  return (
    <ul className="field-diff" data-testid="field-diff">
      {lines.map((line) => (
        <li key={line.id}>
          <span className="diff-label">{line.label}: </span>
          {line.before ? <span className="diff-old">{line.before}</span> : null}
          {line.before && line.after ? <span className="diff-arrow">{' -> '}</span> : null}
          {line.after ? <span className="diff-new">{line.after}</span> : null}
        </li>
      ))}
    </ul>
  )
}
