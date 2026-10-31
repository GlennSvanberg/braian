import { textValue } from './api'
import type { DiffLine } from './field-diff'

/** One changed field per line. Old value is struck through; the new value follows it. */
export function FieldDiff({ lines }: { lines: DiffLine[] }) {
  if (lines.length === 0) return null
  return (
    <ul className="field-diff" data-testid="field-diff">
      {lines.map((line) => (
        <li key={line.id}>
          {line.language ? <span className="lang-chip" data-testid="lang-chip">{line.language}</span> : null}
          <span className="diff-label">{line.label}: </span>
          {line.change === 'add' ? <span className="diff-new">+ {textValue(line.after)}</span> : null}
          {line.change === 'remove' ? <span className="diff-old">{textValue(line.before)}</span> : null}
          {line.change ? null : (
            <>
              {line.before ? <span className="diff-old">{textValue(line.before)}</span> : null}
              {line.before && line.after ? <span className="diff-arrow">{' -> '}</span> : null}
              {line.after ? <span className="diff-new">{textValue(line.after)}</span> : null}
            </>
          )}
        </li>
      ))}
    </ul>
  )
}
