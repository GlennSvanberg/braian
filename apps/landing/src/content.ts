export const demoUrl = 'https://demo.braian.io'
export const repoUrl = 'https://github.com/GlennSvanberg/openpim'
export const licenseUrl = 'https://github.com/GlennSvanberg/openpim/blob/main/LICENSE'
export const docsUrl = 'https://github.com/GlennSvanberg/openpim#readme'

export const nav = [
  { href: '#speed', label: 'Speed' },
  { href: '#agent', label: 'Agent' },
  { href: '#architecture', label: 'Architecture' },
  { href: '#performance', label: 'Performance' },
  { href: '#open-source', label: 'Open source' },
] as const

export const hero = {
  eyebrow: 'Open-source product data · MIT · Self-hosted',
  title: 'Every click is instant.',
  titleAccent: 'And it understands you.',
  lede: 'Braian is a product data system that keeps up with you. Views, searches and filters open at once, across the whole catalog. Press Ctrl+K, say what you want, and it does the work in your session.',
  proof: [
    { value: '64 ms', label: 'save to searchable, p50' },
    { value: '20 ms', label: 'save to every open session, p50' },
  ],
  caption: 'Animated mock-up of the demo workspace. A screen recording will replace it.',
} as const

export const speed = {
  eyebrow: 'For the people in the catalog',
  title: 'No waiting between clicks',
  lede: 'A legacy PIM opens fast, then makes you wait on every click, view and search. In Braian the tree, the table, the detail and the history stay instant across the whole catalog.',
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
  title: 'The system understands you',
  lede: 'Say what you want in plain words. The agent works in the Ctrl+K panel, in the workspace you already have open, and leaves its work for you to review.',
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
  title: 'An architecture you’ll be proud of',
  lede: 'One write path. One change log. Everything else is a consumer you can replace, rebuild, or write yourself.',
  stack: 'Rust on the server. TanStack Start in the app. Postgres, Redpanda and Typesense underneath.',
  principles: [
    {
      title: 'One operation registry',
      body: 'The UI, Ctrl+K, the agent, the REST API and MCP call the same named operations with the same permission checks. The agent can do exactly what you can, nothing more.',
      code: 'patch_object · mass_edit · ui.filter',
    },
    {
      title: 'A change queue you subscribe to',
      body: 'Every committed change lands in Redpanda on an interactive lane and a bulk lane, so a single edit never waits behind a bulk load. Read it as server-sent events or signed webhooks.',
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
    body: 'Listen for changes, write the result back through the operation the UI uses. Validation, history and the responsible person come with it.',
    code: [
      '# listen to every committed change',
      'curl -N "$BRAIAN/v1/stream" \\',
      '  -H "authorization: Bearer $TOKEN"',
      '',
      '# write back through the same operation as the UI',
      'curl "$BRAIAN/v1/operations/patch_object" \\',
      '  -H "authorization: Bearer $TOKEN" \\',
      '  -H "content-type: application/json" \\',
      '  -d \'{"id":"…","values":{"namn":{"de":"Kuppelzelt 13"}}}\'',
    ],
  },
} as const

export const openSource = {
  title: 'Open source. Yours to run.',
  lede: 'MIT licensed and self-hosted. Read the code, fork it, change it, and run it on your own servers. Do what you want with it.',
  facts: ['MIT license', 'Rust and TypeScript', 'Postgres · Redpanda · Typesense', 'Docker Compose for development'],
  code: [
    'git clone https://github.com/GlennSvanberg/openpim',
    'cd openpim && cp .env.example .env',
    'just up && pnpm install && just dev',
  ],
} as const
