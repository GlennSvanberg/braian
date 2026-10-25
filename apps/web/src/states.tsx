export function StateNote({
  tone,
  title,
  detail,
  testId,
}: {
  tone: 'empty' | 'loading' | 'error'
  title: string
  detail?: string
  testId?: string
}) {
  const mark = tone === 'error' ? '!' : tone === 'loading' ? '…' : '·'
  return (
    <div className={`state state-${tone}`} data-testid={testId ?? `state-${tone}`}>
      <span className="state-mark" aria-hidden="true">
        {mark}
      </span>
      <strong>{title}</strong>
      {detail ? <p>{detail}</p> : null}
    </div>
  )
}
