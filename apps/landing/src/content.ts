export const demoUrl = 'https://demo.braian.io'
export const repoUrl = 'https://github.com/GlennSvanberg/braian'
export const licenseUrl = 'https://github.com/GlennSvanberg/braian/blob/main/README.md#license'
export const docsUrl = 'https://github.com/GlennSvanberg/braian#readme'

export const nav = [
  { href: '#speed', label: 'Speed' },
  { href: '#agent', label: 'Agent' },
  { href: '#architecture', label: 'Architecture' },
  { href: '#performance', label: 'Performance' },
  { href: '#open-source', label: 'Open source' },
] as const

export const hero = {
  eyebrow: 'Open-source product data · Apache-2.0 · Self-hosted',
  title: 'Open-source PIM where',
  titleAccent: 'every click is instant.',
  lede: 'A system that understands you, and keeps making your work easier.',
  proof: [
    { value: '64 ms', label: 'save to searchable, p50' },
    { value: '20 ms', label: 'save to every open session, p50' },
  ],
  caption: 'Demo workspace',
} as const

export const speed = {
  eyebrow: 'For the people in the catalog',
  title: 'No waiting between clicks',
  lede: 'Tree, table, detail, and search stay instant.',
  points: [
    {
      title: 'Jump anywhere',
      body: 'Tree, table, detail, history and graph sit side by side in one workspace. Moving between them is instant.',
    },
    {
      title: 'Search the whole catalog',
      body: 'Filters, sorting and grouping run on the server across every object, not only the rows you happened to load.',
    },
    {
      title: 'Keyboard first',
      body: 'Arrows move, Enter opens, Esc goes back. Every command shows its shortcut, and Ctrl+K reaches all of them.',
    },
    {
      title: 'Live by default',
      body: 'When a colleague saves, the row updates in your open view. No refresh, no stale copy.',
    },
  ],
} as const

export const agent = {
  eyebrow: 'Ctrl+K',
  title: 'An agent that does the work, you approve',
  lede: 'Nothing is saved until you press Save.',
  points: [
    {
      title: 'Works where you are',
      body: 'It opens views, filters, selects rows and places panes in your workspace. You see each step as it happens. There is no separate chat window.',
    },
    {
      title: 'Acts as you',
      body: 'It runs with your permissions and calls the same operations you can. It can do exactly what you can, nothing more.',
    },
    {
      title: 'Suggests, never saves',
      body: 'Agents and mass edits leave suggested changes. You review them as a diff and press Save. Nothing reaches the catalog until a person does.',
    },
    {
      title: 'A person behind every change',
      body: 'History records who saved each change and which agent suggested it. A rollback is a new suggestion, never a rewrite of the past.',
    },
  ],
} as const

export const agentSession = {
  prompt: 'Fill in the missing German names for tents',
  steps: [
    'Opening a list of articles',
    'Filtering on “Tält” where Name (de) is empty',
    'Writing German names for 4 objects',
  ],
  answer: '4 suggested changes are ready for review.',
  changes: [
    { id: 'SYN-000012', name: 'Kupoltält 13', after: 'Kuppelzelt 13' },
    { id: 'SYN-000013', name: 'Tunneltält 14', after: 'Tunnelzelt 14' },
    { id: 'SYN-000014', name: 'Familjetält 15', after: 'Familienzelt 15' },
  ],
  more: '1 more',
  field: 'Name (de)',
  owner: 'Suggested by the agent for Maria Lind',
} as const

export const architecture = {
  eyebrow: 'For developers',
  title: 'One write path',
  lede: 'Everything else is a consumer.',
  stack: ['Rust', 'TanStack Start', 'Postgres', 'Typesense'],
  principles: [
    {
      title: 'One operation registry',
      body: 'The UI, Ctrl+K, the agent, the REST API and MCP call the same named operations with the same permission checks. The agent can do exactly what you can, nothing more.',
      code: 'patch_object · mass_edit · ui.filter',
    },
    {
      title: 'A change queue you subscribe to',
      body: 'Every committed change appends to the `changes` table in Postgres on an interactive lane and a bulk lane, so a single edit never waits behind a bulk load. The indexer and live stream read by seq. Subscribe via server-sent events or signed webhooks.',
      code: 'GET /v1/stream',
    },
    {
      title: 'Jobs and integrations are containers',
      body: 'Run them anywhere, in any language. They listen on the queue and write back through the API with a token tied to a person. Custom views embed by URL.',
      code: 'POST /v1/operations/{name}',
    },
    {
      title: 'Transformation is code',
      body: 'Imports and exports are programs you own and version, not mappings locked inside an import tool. The API validates every write either way.',
      code: 'type · length · value list · unit',
    },
    {
      title: 'Postgres is the truth',
      body: 'Objects are JSONB rows with an append-only change log. The search index and the stream derive from it, and both can be rebuilt.',
      code: 'braian reindex',
    },
    {
      title: 'Fast by default',
      body: 'A PATCH answers in 17.5 ms at p50. The same validation rules compile to WebAssembly, so the browser checks a field before it is sent.',
      code: 'crates/rules → wasm32',
    },
  ],
  job: {
    title: 'A job is two requests',
    body: 'Listen for changes, then write back through the same operation.',
    code: [
      '# listen',
      'curl -N "$BRAIAN/v1/stream" \\',
      '  -H "authorization: Bearer $TOKEN"',
      '',
      '# write back',
      'curl "$BRAIAN/v1/operations/patch_object" \\',
      '  -H "authorization: Bearer $TOKEN" \\',
      '  -H "content-type: application/json" \\',
      '  -d \'{"id":"…","values":{"namn":{"de":"Kuppelzelt 13"}}}\'',
    ],
  },
} as const

export const openSource = {
  title: 'Open source. Yours to run.',
  lede: 'Apache-2.0 licensed and self-hosted.',
  facts: ['Apache-2.0', 'Rust and TypeScript', 'Postgres · Typesense', 'Docker Compose for development'],
  code: [
    'git clone https://github.com/GlennSvanberg/braian',
    'cd braian && cp .env.example .env',
    'just up && pnpm install && just dev',
  ],
} as const
