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

export const agentSteps = [
  { title: 'Propose', hint: 'Ctrl+K, in place' },
  { title: 'Accept', hint: 'You decide' },
  { title: 'Undo', hint: 'Even in bulk' },
] as const

export const apiTiles = [
  { title: 'Headless server', icon: 'server' },
  { title: 'API and MCP', icon: 'nodes' },
  { title: '/subscribe events', icon: 'pulse' },
  { title: 'Ship a feature', icon: 'spark' },
] as const

export const modelCards = [
  { name: 'Item', attrs: ['Name', 'Price'], langs: ['sv', 'en', 'de'] },
  { name: 'Supplier', attrs: ['Country'], langs: [] },
  { name: 'Asset', attrs: ['Alt text'], langs: [] },
] as const

export const audiences = ['Product teams', 'Developers'] as const
