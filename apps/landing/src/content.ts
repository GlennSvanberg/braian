export const demoUrl = 'https://demo.braian.io'
export const repoUrl = 'https://github.com/GlennSvanberg/openpim'
export const licenseUrl = 'https://github.com/GlennSvanberg/openpim/blob/main/LICENSE'

export const surfaces = [
  { name: 'UI', detail: 'The workspace' },
  { name: 'Ctrl+K', detail: 'The panel' },
  { name: 'Agent', detail: 'In place, for a person' },
  { name: 'API', detail: 'The same calls' },
  { name: 'MCP', detail: 'External clients' },
] as const

export const features = [
  {
    title: 'Tiled workspaces',
    body: 'Keyboard-first. Views sit side by side, and you move between them from the keyboard.',
  },
  {
    title: 'Ctrl+K',
    body: 'One panel for navigation and for the agent. The agent works in place, on the record in front of you.',
  },
  {
    title: 'Live updates and search',
    body: 'A change shows up without a refresh. Search is how you find a record.',
  },
  {
    title: 'Multilingual attributes',
    body: 'A value holds text per language. The languages sit together on the record.',
  },
  {
    title: 'Data model, with consequences',
    body: 'Classes and attributes are yours to define. A consequence view shows what a change touches before you commit.',
  },
  {
    title: 'History, timeline, rollback',
    body: 'See what changed, follow the timeline, and roll a record back.',
  },
  {
    title: 'Relations',
    body: 'Objects link to other objects. The graph shows those relations.',
  },
  {
    title: 'Job API',
    body: 'Long work runs as a job. You can read its status back.',
  },
] as const

export const architecture = [
  { name: 'Postgres', body: 'The object model.' },
  { name: 'Redpanda', body: 'The change log, from Postgres logical replication.' },
  { name: 'Typesense', body: 'Search.' },
  { name: 'SSE', body: 'Live updates to open screens.' },
] as const
