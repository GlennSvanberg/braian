import type { ReactNode } from 'react'
import { Mark } from '@/components/mark'
import { sceneCss, timeline } from '@/demo/timeline'

type Row = { id: string; name: string; status: 'OK' | 'Draft' | 'Sold out'; weight: string; sel?: string; flash?: string }

const tents: Row[] = [
  { id: 'SYN-000012', name: 'Kupoltält 13', status: 'OK', weight: '2.4 kg', sel: 's0' },
  { id: 'SYN-000013', name: 'Tunneltält 14', status: 'Draft', weight: '3.1 kg', sel: 's1' },
  { id: 'SYN-000014', name: 'Familjetält 15', status: 'OK', weight: '7.8 kg', sel: 's2' },
  { id: 'SYN-000015', name: 'Vindskydd 16', status: 'OK', weight: '0.9 kg' },
  { id: 'SYN-000022', name: 'Kupoltält 23', status: 'Sold out', weight: '2.2 kg' },
  { id: 'SYN-000023', name: 'Tunneltält 24', status: 'OK', weight: '2.9 kg' },
  { id: 'SYN-000024', name: 'Familjetält 25', status: 'Draft', weight: '8.1 kg' },
]

const lumber: Row[] = [
  { id: 'ART-REGEL-45', name: 'Regel 45×95', status: 'OK', weight: '8.6 kg', sel: 's3 s4 s5' },
  { id: 'SYN-000101', name: 'Råspont 22×95', status: 'OK', weight: '4.1 kg' },
  { id: 'SYN-000102', name: 'Läkt 25×48', status: 'Draft', weight: '2.3 kg', flash: 's7' },
  { id: 'SYN-000103', name: 'Fasadpanel 22×145', status: 'OK', weight: '5.9 kg' },
  { id: 'SYN-000104', name: 'Regel 45×120', status: 'OK', weight: '10.8 kg' },
  { id: 'SYN-000105', name: 'Läkt 34×70', status: 'Sold out', weight: '3.4 kg' },
  { id: 'SYN-000106', name: 'Råspont 28×120', status: 'OK', weight: '6.0 kg' },
]

type Detail = {
  show: string
  flash?: string
  id: string
  type: string
  status: Row['status']
  names: [string, string, string]
  weight: string
  group: string
}

const details: Detail[] = [
  {
    show: 's0',
    id: 'SYN-000012',
    type: 'Article',
    status: 'OK',
    names: ['Kupoltält 13', 'Dome tent 13', 'Kuppelzelt 13'],
    weight: '2.4 kg',
    group: 'Friluft › Tält',
  },
  {
    show: 's1',
    id: 'SYN-000013',
    type: 'Article',
    status: 'Draft',
    names: ['Tunneltält 14', 'Tunnel tent 14', 'Tunnelzelt 14'],
    weight: '3.1 kg',
    group: 'Friluft › Tält',
  },
  {
    show: 's2',
    id: 'SYN-000014',
    type: 'Article',
    status: 'OK',
    names: ['Familjetält 15', 'Family tent 15', 'Familienzelt 15'],
    weight: '7.8 kg',
    group: 'Friluft › Tält',
  },
  {
    show: 's3 s4 s5',
    id: 'ART-REGEL-45',
    type: 'Article',
    status: 'OK',
    names: ['Regel 45×95', 'Stud 45×95', 'Kantholz 45×95'],
    weight: '8.6 kg',
    group: 'Bygg › Virke',
  },
  {
    show: 's6 s7',
    flash: 's6',
    id: 'SYN-000012',
    type: 'Article',
    status: 'OK',
    names: ['Kupoltält 13', 'Dome tent 13', 'Kuppelzelt 13'],
    weight: '2.4 kg',
    group: 'Friluft › Tält',
  },
]

const statusClass = { OK: 'ok', Draft: 'draft', 'Sold out': 'out' } as const

function Status({ value }: { value: Row['status'] }) {
  return <span className={`d-status d-${statusClass[value]}`}>{value}</span>
}

function Rows({ rows }: { rows: Row[] }) {
  return rows.map((row) => (
    <div key={row.id} className="d-row" data-sel={row.sel} data-flash={row.flash}>
      <span className="d-id">{row.id}</span>
      <span className="d-name">{row.name}</span>
      {row.flash ? (
        <span className="d-cell">
          <span data-show="s0 s1 s2 s3 s4 s5 s6">
            <Status value={row.status} />
          </span>
          <span data-show="s7">
            <Status value="OK" />
          </span>
        </span>
      ) : (
        <span className="d-cell">
          <Status value={row.status} />
        </span>
      )}
      <span className="d-weight">{row.weight}</span>
    </div>
  ))
}

function Key({ show, children, wide }: { show: string; children: ReactNode; wide?: boolean }) {
  return (
    <span className={wide ? 'd-key d-key-wide' : 'd-key'} data-show={show}>
      {children}
    </span>
  )
}

export function HeroDemo() {
  return (
    <div className="demo" data-demo data-scene="s0" data-timeline={timeline.map((s) => `${s.id}:${s.ms}`).join(',')}>
      <style dangerouslySetInnerHTML={{ __html: sceneCss('.demo') }} />
      <div className="d-top">
        <span className="d-brand">
          <Mark size={14} />
          Braian
        </span>
        <span className="d-tabs">
          <span className="d-tab is-on">Enrich objects</span>
          <span className="d-tab">Data model</span>
          <span className="d-tab">Review</span>
        </span>
        <span className="d-search">
          <span>Search or ask</span>
          <span className="d-kbd">Ctrl K</span>
        </span>
      </div>

      <div className="d-body">
        <div className="d-pane d-tree">
          <p className="d-head">Structure</p>
          <p className="d-node">Friluft</p>
          <p className="d-node d-leaf" data-sel="s0 s1 s2">
            Tält
          </p>
          <p className="d-node d-leaf">Sovsäckar</p>
          <p className="d-node">Bygg</p>
          <p className="d-node d-leaf d-target" data-sel="s3 s4 s5 s6 s7">
            Virke
            <svg className="d-pointer" data-show="s3" viewBox="0 0 16 20" aria-hidden="true">
              <path d="M2 1.5v14l3.6-3.3 2.6 5.8 2.4-1.1-2.6-5.6H13z" />
            </svg>
          </p>
          <p className="d-node d-leaf">Skivor</p>
        </div>

        <div className="d-pane d-table">
          <div className="d-tablehead">
            <span className="d-title" data-show="s0 s1 s2">
              Tält
            </span>
            <span className="d-title" data-show="s3 s4 s5 s6 s7">
              Virke
            </span>
            <span className="d-chip">Type: Article</span>
          </div>
          <div className="d-row d-cols">
            <span>Number</span>
            <span>Name</span>
            <span>Status</span>
            <span className="d-weight">Weight</span>
          </div>
          <div className="d-rows" data-show="s0 s1 s2">
            <Rows rows={tents} />
          </div>
          <div className="d-rows" data-show="s3 s4 s5 s6 s7">
            <Rows rows={lumber} />
          </div>
        </div>

        <div className="d-pane d-detail">
          {details.map((d) => (
            <div key={d.show} className="d-object" data-show={d.show} data-flash={d.flash}>
              <p className="d-objtitle">
                {d.names[0]} <span className="d-id">{d.id}</span>
              </p>
              <p className="d-chips">
                <span className="d-chip">{d.type}</span>
                <Status value={d.status} />
              </p>
              <div className="d-field">
                <span className="d-label">Name</span>
                <span className="d-langs">
                  {(['sv', 'en', 'de'] as const).map((lang, i) => (
                    <span key={lang} className="d-lang">
                      <small>{lang}</small>
                      {d.names[i]}
                    </span>
                  ))}
                </span>
              </div>
              <div className="d-field">
                <span className="d-label">Weight</span>
                <span className="d-value">{d.weight}</span>
              </div>
              <div className="d-field">
                <span className="d-label">Group</span>
                <span className="d-value">{d.group}</span>
              </div>
            </div>
          ))}
        </div>
      </div>

      <div className="d-palette" data-show="s4 s5">
        <div className="d-input">
          <span className="d-placeholder" data-show="s4">
            Search, go to, do, or ask AI
          </span>
          <span data-show="s5">
            <span data-type>kupol</span>
            <span className="d-caret" />
          </span>
        </div>
        <div data-show="s4">
          <p className="d-group">Recent</p>
          <p className="d-item is-on">
            <span>Regel 45×95</span>
            <small>Open object</small>
          </p>
          <p className="d-item">
            <span>Data model</span>
            <small>Open workspace</small>
          </p>
          <p className="d-item d-ask">
            <span>Ask AI</span>
            <small>Tab</small>
          </p>
        </div>
        <div data-show="s5">
          <p className="d-group">Best match</p>
          <p className="d-item is-on">
            <span>
              <mark>Kupol</mark>tält 13 <span className="d-id">SYN-000012</span>
            </span>
            <small>Open object</small>
          </p>
          <p className="d-group">Objects</p>
          <p className="d-item">
            <span>
              <mark>Kupol</mark>tält 23 <span className="d-id">SYN-000022</span>
            </span>
            <small>Open object</small>
          </p>
          <p className="d-item d-ask">
            <span>Ask AI: “kupol”</span>
            <small>Tab</small>
          </p>
        </div>
      </div>

      <div className="d-status-bar">
        <span>Enrich objects</span>
        <span className="d-live" data-show="s0 s1 s2 s3 s4 s5 s6">
          <i /> Live
        </span>
        <span className="d-live d-live-on" data-show="s7">
          <i /> Läkt 25×48 saved by Maria Lind
        </span>
      </div>

      <div className="d-keys" aria-hidden="true">
        <Key show="s1 s2">↓</Key>
        <Key show="s3" wide>
          Click
        </Key>
        <Key show="s4" wide>
          Ctrl K
        </Key>
        <Key show="s5" wide>
          kupol
        </Key>
        <Key show="s6" wide>
          Enter
        </Key>
        <Key show="s7" wide>
          Live update
        </Key>
      </div>
    </div>
  )
}
