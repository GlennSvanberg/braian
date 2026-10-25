import type { ReactNode } from 'react'
import { Button, ButtonLink } from '@/components/ui/button'
import { Mark } from '@/components/mark'
import {
  agentExample,
  apiTiles,
  audiences,
  demoUrl,
  licenseUrl,
  liveOn,
  moreEntities,
  nav,
  productAttributes,
  repoUrl,
} from '@/content'
import { measuredLabel, measuredSets, measurementsDoc } from '@/data/measurements'
import { DataFlow } from '@/flow/diagram'

export function App() {
  return (
    <>
      <a className="skip" href="#content">
        Skip to content
      </a>
      <div className="wrap">
        <header className="topbar" id="top">
          <a className="brand" href="#top">
            <Mark />
            Braian
          </a>
          <nav className="section-nav" aria-label="Sections">
            {nav.map((item) => (
              <a key={item.href} className="nav-link" href={item.href}>
                {item.label}
              </a>
            ))}
          </nav>
          <div className="spacer" />
          <ButtonLink variant="ghost" href={repoUrl}>
            GitHub
          </ButtonLink>
          <ButtonLink variant="primary" href={demoUrl}>
            Try the demo
          </ButtonLink>
          <Button type="button" variant="ghost" data-theme-toggle>
            <span className="when-dark">Light</span>
            <span className="when-light">Dark</span>
          </Button>
        </header>
        <main id="content">
          <Hero />
          <Agent />
          <Model />
          <Api />
          <Flow />
          <UseCases />
          <Measured />
        </main>
        <footer className="sheet footer">
          <div className="footer-brand">
            <div className="brand">
              <Mark />
              Braian
            </div>
            <p>Open-source product data. Self-hosted.</p>
          </div>
          <nav className="footer-links" aria-label="Footer">
            <a href={demoUrl}>Try the demo</a>
            <a href={repoUrl}>View on GitHub</a>
            <a href={licenseUrl}>MIT License</a>
            <a href={measurementsDoc.href}>{measurementsDoc.label}</a>
          </nav>
          <nav className="footer-links" aria-label="On this page">
            {nav.map((item) => (
              <a key={item.href} href={item.href}>
                {item.label}
              </a>
            ))}
            <a href="#use">Who it is for</a>
          </nav>
        </footer>
      </div>
    </>
  )
}

function Hero() {
  return (
    <section className="hero" aria-labelledby="hero-title">
      <div className="sheet sheet-xl hero-copy">
        <p className="eyebrow">Open source · MIT · Self-hosted</p>
        <h1 id="hero-title">
          Product data, <span className="accent">built for agents.</span>
        </h1>
        <p className="lede">A self-hosted product data system, with agents in your session.</p>
        <div className="hero-actions">
          <ButtonLink variant="primary" size="lg" href={demoUrl}>
            Try the demo
          </ButtonLink>
          <ButtonLink variant="raised" size="lg" href={repoUrl}>
            View on GitHub
          </ButtonLink>
        </div>
      </div>
      <figure className="sheet sheet-xl video-slot">
        <div className="video-frame">
          <figcaption>
            <p className="eyebrow">Demo video</p>
            <p>A recording of the product will sit here.</p>
          </figcaption>
        </div>
      </figure>
    </section>
  )
}

function Agent() {
  return (
    <section className="section" id="agent" aria-labelledby="agent-title">
      <div className="section-head">
        <h2 id="agent-title">An agent that works in your session, not a chatbot</h2>
        <p className="lede">
          It acts as you, with your permissions. Every change is a proposal you can undo in one click.
        </p>
      </div>
      <figure className="session-figure">
        <ol className="session">
          <li className="sheet session-ask">
            <span className="kbd mono">Ctrl+K</span>
            <p>{agentExample.prompt}</p>
          </li>
          <Beat />
          <li className="sheet session-progress">
            <span className="eyebrow">In place</span>
            <p>{agentExample.progress}</p>
            <span className="bar" aria-hidden="true" />
          </li>
          <Beat />
          <li className="sheet session-proposal">
            <span className="eyebrow">Example</span>
            <p className="proposal-count">{agentExample.objects}</p>
            <p className="proposal-detail">{agentExample.changes}</p>
            <div className="proposal-actions">
              <span className="btn btn-primary">Accept</span>
              <span className="btn btn-raised">Review diff</span>
            </div>
          </li>
          <Beat />
          <li className="sheet session-undo">
            <strong>Undo</strong>
            <span>One click</span>
          </li>
        </ol>
        <figcaption className="note session-caption">Illustrative session.</figcaption>
      </figure>
    </section>
  )
}

function Beat() {
  return <li className="beat-to" aria-hidden="true" />
}

function attributeDetail(attr: { type: string; detail: string }) {
  if (!attr.detail) return ''
  return attr.type === 'reference' ? `→ ${attr.detail}` : attr.detail
}

function Model() {
  return (
    <section className="section" id="model" aria-labelledby="model-title">
      <div className="section-head">
        <h2 id="model-title">Model anything. No migrations, no developers.</h2>
        <p className="lede">API, search, and the UI pick up a new entity immediately.</p>
      </div>
      <div className="schema-board">
        <article className="sheet schema">
          <header>
            <h3>Product</h3>
            <span className="eyebrow">Entity</span>
          </header>
          <ul>
            {productAttributes.map((attr) => (
              <li key={attr.name} className="attr">
                <span className="attr-name">{attr.name}</span>
                <span className="attr-type">{attr.type}</span>
                <span className="attr-detail">{attributeDetail(attr)}</span>
              </li>
            ))}
          </ul>
        </article>
        <div className="schema-side">
          <p className="note">New entities, the same way</p>
          <ul className="entity-picks">
            {moreEntities.map((name) => (
              <li key={name} className="sheet entity-pick">
                {name}
              </li>
            ))}
          </ul>
          <p className="note">Live on</p>
          <ul className="pickup">
            {liveOn.map((name) => (
              <li key={name} className="sheet">
                {name}
              </li>
            ))}
          </ul>
        </div>
      </div>
    </section>
  )
}

function Api() {
  return (
    <section className="section" id="api" aria-labelledby="api-title">
      <div className="section-head">
        <h2 id="api-title">An API you can build on</h2>
        <p className="lede">Headless, so a feature ships on the API and the event stream.</p>
      </div>
      <ul className="tiles">
        {apiTiles.map((tile) => (
          <li key={tile.title} className="sheet tile">
            <TileIcon name={tile.icon} />
            <span>{tile.title}</span>
          </li>
        ))}
      </ul>
    </section>
  )
}

function Flow() {
  return (
    <section className="section" id="flow" aria-labelledby="flow-title">
      <div className="section-head">
        <h2 id="flow-title">How data moves</h2>
        <p className="lede">One operation in. Search, live updates, and /subscribe branch back out.</p>
      </div>
      <figure className="sheet flow" aria-labelledby="flow-title">
        <p className="sr-only">
          UI, Agent with Ctrl+K and proposals, API, and MCP clients write through one shared
          operation layer into Postgres, which holds the object model and the change log. Logical
          replication reaches the relay, then Redpanda, which splits into a fast interactive lane
          and a heavier bulk lane. Both lanes reach the indexer and Typesense. Search loops back to
          the UI. The change stream also sends SSE live updates back to open UIs, and /subscribe
          events to external consumers for headless integrations.
        </p>
        <DataFlow />
        <p className="stack">Rust on the server. TanStack Start in the app.</p>
      </figure>
    </section>
  )
}

function UseCases() {
  return (
    <section className="section" id="use" aria-labelledby="use-title">
      <div className="section-head">
        <h2 id="use-title">Who it is for</h2>
        <p className="lede">People who run the catalog, and people who extend it.</p>
      </div>
      <ul className="who">
        {audiences.map((name) => (
          <li key={name} className="sheet who-card">
            {name}
          </li>
        ))}
      </ul>
    </section>
  )
}

function Measured() {
  return (
    <section className="section" id="measured" aria-labelledby="measured-title">
      <div className="section-head">
        <h2 id="measured-title">{measuredLabel}</h2>
        <p className="lede">
          Recorded runs, with the hardware and the data size.{' '}
          <a className="text-link" href={measurementsDoc.href}>
            {measurementsDoc.label}
          </a>
        </p>
      </div>
      <div className="measured-sets">
        {measuredSets.map((set) => (
          <article key={set.id} className="measured-set" aria-labelledby={`set-${set.id}`}>
            <p className="set-note" id={`set-${set.id}`}>
              <strong>{set.name}</strong>
              {` · ${set.hardware} · ${set.dataSize}`}
              {set.conditions ? ` · ${set.conditions}` : ''}
              {` · ${set.recordedOn}`}
            </p>
            <ul className="stat-grid">
              {set.figures.map((figure) => (
                <li key={`${set.id}-${figure.value}-${figure.label}`} className="sheet stat">
                  <p className="stat-value">{figure.value}</p>
                  <p className="stat-label">{figure.label}</p>
                  {figure.note ? <p className="stat-note">{figure.note}</p> : null}
                </li>
              ))}
            </ul>
          </article>
        ))}
      </div>
    </section>
  )
}

function TileIcon({ name }: { name: string }) {
  const common = {
    viewBox: '0 0 24 24',
    width: 22,
    height: 22,
    fill: 'none',
    stroke: 'currentColor',
    strokeWidth: 1.6,
    strokeLinecap: 'round' as const,
    strokeLinejoin: 'round' as const,
    'aria-hidden': true,
  }
  const paths: Record<string, ReactNode> = {
    server: (
      <>
        <rect x="4" y="4" width="16" height="6" rx="1.5" />
        <rect x="4" y="14" width="16" height="6" rx="1.5" />
        <path d="M8 7h.01M8 17h.01" />
      </>
    ),
    nodes: (
      <>
        <circle cx="6" cy="12" r="2.2" />
        <circle cx="18" cy="7" r="2.2" />
        <circle cx="18" cy="17" r="2.2" />
        <path d="M8 12h6M14.2 10.2 16 8.6M14.2 13.8 16 15.4" />
      </>
    ),
    pulse: <path d="M3 12h4l2-5 4 10 2-5h6" />,
    spark: (
      <>
        <path d="M12 3v4M12 17v4M3 12h4M17 12h4" />
        <circle cx="12" cy="12" r="2.4" />
      </>
    ),
  }
  return <svg {...common}>{paths[name]}</svg>
}
