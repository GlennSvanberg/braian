export const demoUrl = 'https://demo.braian.io'
export const repoUrl = 'https://github.com/GlennSvanberg/openpim'
export const licenseUrl = 'https://github.com/GlennSvanberg/openpim/blob/main/LICENSE'

export const nav = [
  { href: '#agent', label: 'Agent' },
  { href: '#model', label: 'Model' },
  { href: '#api', label: 'API' },
  { href: '#flow', label: 'Flow' },
  { href: '#measured', label: 'Measured' },
] as const

export const agentPoints = [
  {
    title: 'On your behalf',
    body: 'The agent acts for you. You stay the actor on the record.',
  },
  {
    title: 'In place',
    body: 'Ctrl+K works on the record in front of you, inside the workspace.',
  },
  {
    title: 'Progress',
    body: 'Each step is shown while the agent works.',
  },
  {
    title: 'Proposals you accept',
    body: 'An edit is a proposal. You accept a single change or a bulk edit.',
  },
  {
    title: 'Undo',
    body: 'You can undo an accepted change.',
  },
] as const

export const modelPoints = [
  {
    title: 'Entities',
    body: 'Name the kinds of records you need: items, suppliers, assets, or anything else the catalog uses.',
  },
  {
    title: 'Attributes',
    body: 'Add the fields on those records, with the rules you want enforced.',
  },
  {
    title: 'Languages',
    body: 'Write a value in every language you add. The languages sit together on the record.',
  },
] as const

export const apiPoints = [
  {
    title: 'Headless',
    body: 'The server is the product. Screens, agents, and other clients sit on top of it.',
  },
  {
    title: 'API and MCP',
    body: 'External tools call the same operations the workspace calls.',
  },
  {
    title: '/subscribe',
    body: 'External consumers receive events. Open screens receive live updates over SSE.',
  },
  {
    title: 'Fast to extend',
    body: 'Built for the agentic era. A feature ships as a client of the API and the event stream.',
  },
] as const

export const flowSources = ['UI', 'Agent', 'API', 'MCP'] as const

export const flowSteps = [
  { name: 'Postgres', detail: 'Object model' },
  { name: 'Relay', detail: 'Logical replication' },
  { name: 'Redpanda', detail: 'Two lanes', lanes: ['Interactive', 'Bulk'] as const },
  { name: 'Indexer', detail: 'Reads both lanes' },
  { name: 'Typesense', detail: 'Search' },
] as const

export const flowReturns = [
  { name: 'SSE', detail: 'Live updates back to the UI' },
  { name: '/subscribe', detail: 'Events to external consumers' },
] as const

export const audiences = [
  {
    title: 'Product data teams',
    body: 'Wholesalers, retailers, and manufacturers. The catalog work a PIM is for, on a system you host.',
  },
  {
    title: 'Developers',
    body: 'An open product data system that is API-first and MCP-first, ready for the clients you add.',
  },
] as const
