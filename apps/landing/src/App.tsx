import { Button, ButtonLink } from '@/components/ui/button'
import { Mark } from '@/components/mark'
import {
  agentPoints,
  apiPoints,
  audiences,
  demoUrl,
  flowReturns,
  flowSources,
  flowSteps,
  licenseUrl,
  modelPoints,
  nav,
  repoUrl,
} from '@/content'
import { measuredLabel, measuredSets, measurementsDoc } from '@/data/measurements'

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
        <p className="lede">
          Braian is an open-source (MIT), self-hosted product data system — what a PIM is usually
          used for. It is built for speed, and made to work with agents from the start.
        </p>
        <div className="hero-actions">
          <ButtonLink variant="primary" size="lg" href={demoUrl}>
            Try the demo
          </ButtonLink>
          <ButtonLink variant="raised" size="lg" href={repoUrl}>
            View on GitHub
          </ButtonLink>
        </div>
        <p className="note">You host it. You extend it.</p>
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
        <h2 id="agent-title">Agent as a first-class citizen</h2>
        <p className="lede">
          The agent works in your session, on the records you have open. You remain the person on
          the record.
        </p>
      </div>
      <ol className="point-grid point-grid-5">
        {agentPoints.map((point) => (
          <li key={point.title} className="sheet point">
            <h3>{point.title}</h3>
            <p>{point.body}</p>
          </li>
        ))}
      </ol>
    </section>
  )
}

function Model() {
  return (
    <section className="section" id="model" aria-labelledby="model-title">
      <div className="section-head">
        <h2 id="model-title">A model you define</h2>
        <p className="lede">
          Create the entities and attributes the catalog needs. A value can carry more than one
          language.
        </p>
      </div>
      <ul className="point-grid point-grid-3">
        {modelPoints.map((point) => (
          <li key={point.title} className="sheet point">
            <h3>{point.title}</h3>
            <p>{point.body}</p>
          </li>
        ))}
      </ul>
    </section>
  )
}

function Api() {
  return (
    <section className="section" id="api" aria-labelledby="api-title">
      <div className="section-head">
        <h2 id="api-title">An API you can build on</h2>
        <p className="lede">
          Braian runs headless. Developers add features against the API and the event stream. It is
          built for the agentic era, where that kind of change should ship quickly.
        </p>
      </div>
      <ul className="point-grid point-grid-2">
        {apiPoints.map((point) => (
          <li key={point.title} className="sheet point">
            <h3 className={point.title.startsWith('/') ? 'mono' : undefined}>{point.title}</h3>
            <p>{point.body}</p>
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
        <p className="lede">
          A write from the UI, the agent, the API, or MCP follows one path into search. Live
          updates and events come back out.
        </p>
      </div>
      <figure className="sheet flow" aria-labelledby="flow-title">
        <p className="sr-only">
          UI, Agent, API, and MCP write into Postgres. A relay reads logical replication and
          publishes to Redpanda on an interactive lane and a bulk lane. The indexer reads both
          lanes and updates Typesense search. SSE sends live updates back to the UI. /subscribe
          sends events to external consumers.
        </p>
        <ol className="pipe">
          <li>
            <div className="node writers">
              <strong>Writers</strong>
              <ul>
                {flowSources.map((name) => (
                  <li key={name}>{name}</li>
                ))}
              </ul>
            </div>
            <span className="chev" aria-hidden="true" />
          </li>
          {flowSteps.map((step, index) => (
            <li key={step.name} className={'lanes' in step ? 'wide' : undefined}>
              <div className="node">
                <strong>{step.name}</strong>
                <span>{step.detail}</span>
                {'lanes' in step ? (
                  <span className="lanes">
                    {step.lanes.map((lane) => (
                      <span key={lane} className="lane">
                        {lane}
                      </span>
                    ))}
                  </span>
                ) : null}
              </div>
              {index < flowSteps.length - 1 ? (
                <span className="chev" aria-hidden="true" />
              ) : (
                <span className="chev chev-end" aria-hidden="true" />
              )}
            </li>
          ))}
        </ol>
        <p className="flow-label">Comes back</p>
        <ul className="returns">
          {flowReturns.map((item) => (
            <li key={item.name}>
              <strong>{item.name}</strong>
              <span>{item.detail}</span>
            </li>
          ))}
        </ul>
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
      </div>
      <ul className="point-grid point-grid-2">
        {audiences.map((audience) => (
          <li key={audience.title} className="sheet point">
            <h3>{audience.title}</h3>
            <p>{audience.body}</p>
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
          Recorded runs. Hardware and data size are part of each figure.{' '}
          <a className="text-link" href={measurementsDoc.href}>
            {measurementsDoc.label}
          </a>
        </p>
      </div>
      <div className="measured-grid">
        {measuredSets.map((set) => (
          <article key={set.id} className="sheet measured-set" aria-labelledby={`set-${set.id}`}>
            <header className="set-head">
              <h3 id={`set-${set.id}`}>{set.name}</h3>
              <p>
                {set.hardware}, {set.dataSize}
              </p>
              <p>
                Recorded {set.recordedOn}
                {set.conditions ? `. ${set.conditions}.` : '.'}
              </p>
            </header>
            <ul className="figures">
              {set.figures.map((figure) => (
                <li key={`${figure.value}-${figure.label}`}>
                  <p className="figure-value">{figure.value}</p>
                  <p className="figure-label">{figure.label}</p>
                  {figure.note ? <p className="figure-note">{figure.note}</p> : null}
                </li>
              ))}
            </ul>
          </article>
        ))}
      </div>
    </section>
  )
}
