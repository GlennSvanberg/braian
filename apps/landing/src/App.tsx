import { Button, ButtonLink } from '@/components/ui/button'
import { Mark } from '@/components/mark'
import {
  agent,
  agentSession,
  architecture,
  demoUrl,
  docsUrl,
  hero,
  licenseUrl,
  nav,
  openSource,
  repoUrl,
  speed,
} from '@/content'
import { performanceFootnote, primaryFigures, secondaryFigures, type Figure } from '@/data/measurements'
import { HeroDemo } from '@/demo/hero-demo'
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
          <Button type="button" variant="ghost" className="theme-toggle" data-theme-toggle aria-label="Switch color theme">
            <span className="when-dark">Light</span>
            <span className="when-light">Dark</span>
          </Button>
          <ButtonLink variant="ghost" className="topbar-github" href={repoUrl}>
            GitHub
          </ButtonLink>
          <ButtonLink variant="primary" href={demoUrl}>
            Try the demo
          </ButtonLink>
        </header>
        <main id="content">
          <Hero />
          <Speed />
          <Agent />
          <Architecture />
          <Performance />
          <OpenSource />
        </main>
        <footer className="footer">
          <div className="footer-brand">
            <div className="brand">
              <Mark />
              Braian
            </div>
            <p>Open-source product data. Self-hosted. MIT.</p>
          </div>
          <nav className="footer-links" aria-label="Footer">
            <a href={demoUrl}>Demo</a>
            <a href={repoUrl}>GitHub</a>
            <a href={docsUrl}>Documentation</a>
            <a href={licenseUrl}>MIT License</a>
          </nav>
        </footer>
      </div>
    </>
  )
}

function Hero() {
  return (
    <section className="hero" aria-labelledby="hero-title">
      <div className="hero-copy">
        <p className="eyebrow">{hero.eyebrow}</p>
        <h1 id="hero-title">
          {hero.title} <span className="accent">{hero.titleAccent}</span>
        </h1>
        <p className="lede hero-lede">{hero.lede}</p>
        <div className="hero-actions">
          <ButtonLink variant="primary" size="lg" href={demoUrl}>
            Try the demo
          </ButtonLink>
          <ButtonLink variant="raised" size="lg" href={repoUrl}>
            View on GitHub
          </ButtonLink>
        </div>
        <ul className="hero-proof" aria-label="Median figures">
          {hero.proof.map((item) => (
            <li key={item.label}>
              <strong>{item.value}</strong> {item.label}
            </li>
          ))}
          <li>
            <a className="text-link" href="#performance">
              All figures
            </a>
          </li>
        </ul>
      </div>
      {/* Video slot: replace <HeroDemo /> with the navigation recording when it exists. */}
      <figure className="hero-visual" data-video-slot>
        <div className="hero-window" aria-hidden="true">
          <HeroDemo />
        </div>
        <figcaption className="caption">{hero.caption}</figcaption>
      </figure>
    </section>
  )
}

function SectionHead({ id, eyebrow, title, lede }: { id: string; eyebrow?: string; title: string; lede: string }) {
  return (
    <div className="section-head">
      {eyebrow ? <p className="eyebrow eyebrow-accent">{eyebrow}</p> : null}
      <h2 id={id}>{title}</h2>
      <p className="lede">{lede}</p>
    </div>
  )
}

function Speed() {
  return (
    <section className="section" id="speed" aria-labelledby="speed-title">
      <SectionHead id="speed-title" eyebrow={speed.eyebrow} title={speed.title} lede={speed.lede} />
      <ul className="points points-4">
        {speed.points.map((point, i) => (
          <li key={point.title} className="sheet point">
            <span className="point-index mono">0{i + 1}</span>
            <h3>{point.title}</h3>
            <p>{point.body}</p>
          </li>
        ))}
      </ul>
    </section>
  )
}

function Agent() {
  return (
    <section className="section" id="agent" aria-labelledby="agent-title">
      <SectionHead id="agent-title" eyebrow={agent.eyebrow} title={agent.title} lede={agent.lede} />
      <div className="agent-board">
        <figure className="agent-stage">
          <AgentPanel />
          <figcaption className="caption">Illustrative session in the demo catalog.</figcaption>
        </figure>
        <ul className="agent-points">
          {agent.points.map((point) => (
            <li key={point.title}>
              <h3>{point.title}</h3>
              <p>{point.body}</p>
            </li>
          ))}
        </ul>
      </div>
    </section>
  )
}

function AgentPanel() {
  return (
    <div className="agent-panel" aria-label="Ctrl+K panel in AI mode">
      <div className="ai-input">
        <span className="ai-badge">AI</span>
        <span className="ai-prompt">{agentSession.prompt}</span>
        <span className="kbd mono">Esc</span>
      </div>
      <ol className="ai-steps">
        {agentSession.steps.map((step) => (
          <li key={step}>
            <Check />
            {step}
          </li>
        ))}
      </ol>
      <p className="ai-answer">{agentSession.answer}</p>
      <div className="sugg">
        <div className="sugg-head">
          <span>Suggested changes</span>
          <span className="sugg-count">4 objects · 4 fields</span>
        </div>
        {agentSession.changes.map((change) => (
          <div key={change.id} className="sugg-row">
            <span className="sugg-object">
              {change.name}
              <span className="d-id">{change.id}</span>
            </span>
            <span className="sugg-field">{agentSession.field}</span>
            <span className="sugg-values">
              <span className="sugg-before" aria-label="empty">—</span>
              <span className="sugg-arrow" aria-hidden="true">→</span>
              <ins>{change.after}</ins>
            </span>
          </div>
        ))}
        <p className="sugg-more">{agentSession.more}</p>
      </div>
      <div className="ai-foot">
        <span className="ai-owner">{agentSession.owner}</span>
        <span className="btn btn-raised">Discard</span>
        <span className="btn btn-primary">Save</span>
      </div>
    </div>
  )
}

function Check() {
  return (
    <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true" className="step-check">
      <path d="M3.5 8.5 6.5 11.5 12.5 4.5" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  )
}

function Architecture() {
  return (
    <section className="section" id="architecture" aria-labelledby="architecture-title">
      <SectionHead
        id="architecture-title"
        eyebrow={architecture.eyebrow}
        title={architecture.title}
        lede={architecture.lede}
      />
      <figure className="sheet flow" aria-labelledby="architecture-title">
        <p className="sr-only">
          The UI, the agent in Ctrl+K, the API and MCP clients write through one shared operation
          registry into Postgres, which holds the object model and the change log. Logical
          replication reaches the relay, then Redpanda, which splits into a fast interactive lane and
          a heavier bulk lane. Both lanes reach the indexer and Typesense. Search loops back to the
          UI. The change stream also sends live updates over server-sent events to open UIs, and
          stream and webhook events to external consumers.
        </p>
        <DataFlow />
        <p className="stack">{architecture.stack}</p>
      </figure>
      <ul className="principles">
        {architecture.principles.map((item) => (
          <li key={item.title} className="sheet principle">
            <h3>{item.title}</h3>
            <p>{item.body}</p>
            <code className="mono">{item.code}</code>
          </li>
        ))}
      </ul>
      <div className="sheet job">
        <div className="job-copy">
          <h3>{architecture.job.title}</h3>
          <p>{architecture.job.body}</p>
        </div>
        <Terminal lines={architecture.job.code} label="Shell" />
      </div>
    </section>
  )
}

function Terminal({ lines, label }: { lines: readonly string[]; label: string }) {
  return (
    <div className="terminal">
      <div className="terminal-bar" aria-hidden="true">
        <span>{label}</span>
      </div>
      <pre className="mono">
        {lines.map((line, i) => (
          <span key={i} className={line.startsWith('#') ? 'comment' : undefined}>
            {line || ' '}
            {'\n'}
          </span>
        ))}
      </pre>
    </div>
  )
}

function FigureValue({ figure }: { figure: Figure }) {
  return (
    <p className="stat-value">
      {figure.value}
      {figure.unit ? <span className="stat-unit">{figure.unit}</span> : null}
    </p>
  )
}

function Performance() {
  return (
    <section className="section" id="performance" aria-labelledby="performance-title">
      <SectionHead
        id="performance-title"
        eyebrow="Braian’s own numbers"
        title="Performance"
        lede="From save to search, and from your screen to everyone else’s."
      />
      <ul className="stats stats-primary">
        {primaryFigures.map((figure) => (
          <li key={figure.label} className="sheet stat">
            <FigureValue figure={figure} />
            <p className="stat-label">{figure.label}</p>
            <p className="stat-note">{figure.note}</p>
          </li>
        ))}
      </ul>
      <ul className="stats stats-secondary">
        {secondaryFigures.map((figure) => (
          <li key={figure.label} className="sheet stat">
            <FigureValue figure={figure} />
            <p className="stat-label">{figure.label}</p>
            <p className="stat-note">{figure.note}</p>
          </li>
        ))}
      </ul>
      <p className="footnote">{performanceFootnote}</p>
    </section>
  )
}

function OpenSource() {
  return (
    <section className="section" id="open-source" aria-labelledby="open-source-title">
      <div className="sheet sheet-xl oss">
        <div className="oss-copy">
          <p className="eyebrow eyebrow-accent">Open source</p>
          <h2 id="open-source-title">{openSource.title}</h2>
          <p className="lede">{openSource.lede}</p>
          <ul className="oss-facts">
            {openSource.facts.map((fact) => (
              <li key={fact}>{fact}</li>
            ))}
          </ul>
          <div className="hero-actions">
            <ButtonLink variant="primary" size="lg" href={demoUrl}>
              Try the demo
            </ButtonLink>
            <ButtonLink variant="raised" size="lg" href={repoUrl}>
              View on GitHub
            </ButtonLink>
          </div>
        </div>
        <Terminal lines={openSource.code} label="Run it locally" />
      </div>
    </section>
  )
}
