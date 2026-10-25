import { Button, ButtonLink } from '@/components/ui/button'
import { Mark } from '@/components/mark'
import { architecture, demoUrl, features, licenseUrl, repoUrl, surfaces } from '@/content'
import { Stage } from '@/stage'

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
            <a className="nav-link" href="#how">
              How it works
            </a>
            <a className="nav-link" href="#features">
              Features
            </a>
            <a className="nav-link" href="#use">
              Use cases
            </a>
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
          <How />
          <Features />
          <UseCases />
          <Architecture />
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
          </nav>
          <nav className="footer-links" aria-label="On this page">
            <a href="#how">How it works</a>
            <a href="#features">Features</a>
            <a href="#use">Use cases</a>
            <a href="#architecture">Architecture</a>
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
        <p className="note">One operation, from the UI, the API, or an agent.</p>
      </div>
      <Stage />
    </section>
  )
}

function How() {
  return (
    <section className="section" id="how" aria-labelledby="how-title">
      <div className="section-head">
        <h2 id="how-title">One operation, every surface</h2>
        <p className="lede">
          Every action is one shared operation, whether it comes from the UI, Ctrl+K, the built-in
          agent, the API, or an external MCP client.
        </p>
      </div>
      <div className="sheet how">
        <ul className="surfaces">
          {surfaces.map((item) => (
            <li key={item.name}>
              <b>{item.name}</b>
              <span>{item.detail}</span>
            </li>
          ))}
        </ul>
        <p className="same">Same operation.</p>
        <div className="split-notes">
          <article>
            <h3>On behalf of a person</h3>
            <p>An agent always acts for a user. That person stays the actor on the record.</p>
          </article>
          <article>
            <h3>Proposals a person accepts</h3>
            <p>
              Edits become proposals. A person accepts them — a single change, a bulk edit, or an
              undo.
            </p>
          </article>
        </div>
      </div>
    </section>
  )
}

function Features() {
  return (
    <section className="section" id="features" aria-labelledby="features-title">
      <div className="section-head">
        <h2 id="features-title">What you work in</h2>
        <p className="lede">The workspace is the product. The API speaks the same language.</p>
      </div>
      <ul className="feature-grid">
        {features.map((feature) => (
          <li key={feature.title} className="sheet feature">
            <h3>{feature.title}</h3>
            <p>{feature.body}</p>
          </li>
        ))}
      </ul>
    </section>
  )
}

function UseCases() {
  return (
    <section className="section" id="use" aria-labelledby="use-title">
      <div className="section-head">
        <h2 id="use-title">Who it is for</h2>
      </div>
      <div className="use-grid">
        <article className="sheet feature">
          <h3>Product data teams</h3>
          <p>
            Wholesalers, retailers, and manufacturers. Items, attributes, languages, and structure —
            the work a PIM is for — on a system you host.
          </p>
        </article>
        <article className="sheet feature">
          <h3>Developers</h3>
          <p>
            An open PIM that is API-first and MCP-first. The operations your tools call are the
            operations the UI calls.
          </p>
        </article>
      </div>
    </section>
  )
}

function Architecture() {
  return (
    <section className="section" id="architecture" aria-labelledby="arch-title">
      <div className="section-head">
        <h2 id="arch-title">How a change moves</h2>
      </div>
      <div className="sheet arch-sheet">
        <ol className="arch">
          {architecture.map((step, index) => (
            <li key={step.name}>
              <span className="step">{index + 1}</span>
              <h3>{step.name}</h3>
              <p>{step.body}</p>
            </li>
          ))}
        </ol>
        <p className="stack">Rust on the server. TanStack Start in the app.</p>
      </div>
    </section>
  )
}
