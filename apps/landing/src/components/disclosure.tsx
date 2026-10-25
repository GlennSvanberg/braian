export type DisclosureItem = {
  title: string
  body: string
  code?: string
}

export function DisclosureList({
  items,
  idPrefix,
  columns,
  numbered = false,
}: {
  items: readonly DisclosureItem[]
  idPrefix: string
  columns: '4' | '3' | 'side'
  numbered?: boolean
}) {
  return (
    <ul className={`disclose-grid disclose-grid-${columns}`}>
      {items.map((item, index) => (
        <Disclosure
          key={item.title}
          id={`${idPrefix}-${index + 1}`}
          icon={numbered ? String(index + 1).padStart(2, '0') : undefined}
          title={item.title}
          body={item.body}
          chip={item.code}
        />
      ))}
    </ul>
  )
}

function Disclosure({
  id,
  title,
  body,
  chip,
  icon,
}: {
  id: string
  title: string
  body: string
  chip?: string
  icon?: string
}) {
  return (
    <li className="sheet disclose-card" data-disclose-card>
      <h3>
        <button type="button" className="disclose" data-disclose aria-expanded={false} aria-controls={id}>
          <span className="disclose-main">
            <span className="disclose-title">
              {icon ? (
                <span className="point-index mono" aria-hidden="true">
                  {icon}
                </span>
              ) : null}
              {title}
            </span>
            {chip ? <code className="mono disclose-chip">{chip}</code> : null}
          </span>
          <Chevron />
        </button>
      </h3>
      <div id={id} className="disclose-panel" inert>
        <div className="disclose-body">
          <p>{body}</p>
        </div>
      </div>
    </li>
  )
}

function Chevron() {
  return (
    <svg className="disclose-chevron" viewBox="0 0 16 16" width="16" height="16" aria-hidden="true">
      <path
        d="M4 6.5 8 10.5 12 6.5"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.6"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  )
}
