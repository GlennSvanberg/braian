import { Mark } from '@/components/mark'

/** Decorative picture of the product. Real UI chrome, demo catalog words, no metrics. */
export function Stage() {
  return (
    <div className="stage" inert aria-hidden="true">
        <header className="top">
          <div className="brand">
            <Mark />
            Braian
          </div>
          <div className="tabs">
            <button type="button" className="tab active" tabIndex={-1}>
              Items
            </button>
            <button type="button" className="tab" tabIndex={-1}>
              Data model
            </button>
          </div>
          <div className="spacer" />
          <span className="icon-btn">
            <kbd>Ctrl</kbd>
            <kbd>K</kbd>
          </span>
        </header>
        <div className="stage-panes">
            <section className="pane">
              <div className="pane-tabs">
                <button type="button" className="pane-tab active" tabIndex={-1}>
                  Objects
                </button>
              </div>
              <div className="pane-body">
                <div className="grid">
                  <div className="head">
                    <div className="cell name">Name</div>
                    <div className="cell cls">Class</div>
                  </div>
                  <div className="row selected">
                    <div className="cell name">Dome tent</div>
                    <div className="cell cls">Tent</div>
                  </div>
                  <div className="row">
                    <div className="cell name">Tunnel tent</div>
                    <div className="cell cls">Tent</div>
                  </div>
                  <div className="row">
                    <div className="cell name">Down bag</div>
                    <div className="cell cls">Sleeping bag</div>
                  </div>
                  <div className="row">
                    <div className="cell name">Stud</div>
                    <div className="cell cls">Timber</div>
                  </div>
                </div>
              </div>
            </section>
            <section className="pane">
              <div className="pane-tabs">
                <button type="button" className="pane-tab active" tabIndex={-1}>
                  Detail
                </button>
                <button type="button" className="pane-tab" tabIndex={-1}>
                  Graph
                </button>
                <button type="button" className="pane-tab" tabIndex={-1}>
                  History
                </button>
              </div>
              <div className="pane-body">
                <div className="form">
                  <strong>Dome tent</strong>
                  <div className="field">
                    <label>Name</label>
                    <div className="langs">
                      <div className="lang">
                        <b>sv</b>
                        <span>Kupoltält</span>
                      </div>
                      <div className="lang">
                        <b>en</b>
                        <span>Dome tent</span>
                      </div>
                      <div className="lang">
                        <b>de</b>
                        <span>Kuppelzelt</span>
                      </div>
                    </div>
                  </div>
                </div>
              </div>
            </section>
        </div>
      <div className="command">
        <div className="fake-input">Search objects, open views, or ask the agent</div>
        <div className="palette-list">
          <div className="hit">
            <span>
              <span className="hit-title">Items</span>
              <small>Open the workspace</small>
            </span>
            <kbd>Enter</kbd>
          </div>
          <div className="hit ask">
            <span className="hit-title">Ask AI: “translate the empty names”</span>
          </div>
        </div>
        <div className="palette-foot">
          <span>Run the highlighted row</span>
          <span>↵ run · Tab ask AI · Esc close</span>
        </div>
      </div>
      <div className="status">
        <span className="live">connected</span>
      </div>
    </div>
  )
}
