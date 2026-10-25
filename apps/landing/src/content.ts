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

export const agentExample = {
  prompt: 'Translate all descriptions in Spring 2026 to German',
  progress: 'Translating descriptions',
  objects: '1,248 objects',
  changes: '1,248 attribute changes',
} as const

export const apiTiles = [
  { title: 'Headless server', icon: 'server' },
  { title: 'API and MCP', icon: 'nodes' },
  { title: '/subscribe events', icon: 'pulse' },
  { title: 'Ship a feature', icon: 'spark' },
] as const

export const productAttributes = [
  { name: 'Name', type: 'text', detail: '20 languages' },
  { name: 'Weight', type: 'number', detail: 'kg' },
  { name: 'Supplier', type: 'reference', detail: 'Supplier' },
  { name: 'Variants', type: 'list', detail: '' },
] as const

export const moreEntities = ['Supplier', 'Certificate'] as const

export const liveOn = ['API', 'Search', 'UI'] as const

export const audiences = ['Product teams', 'Developers'] as const
