/**
 * Published landing figures. Edit this file when a run is remeasured.
 * Only Braian's own numbers, all from the same 8 vCPU VPS. Leave unreached goals out.
 */

export const performanceFootnote = 'Measured on an 8 vCPU VPS.'

export type Figure = {
  value: string
  unit?: string
  label: string
  note: string
}

export const primaryFigures: readonly Figure[] = [
  { value: '64', unit: 'ms', label: 'Save to searchable', note: 'p50, idle' },
  { value: '20', unit: 'ms', label: 'Save to live update in other sessions', note: 'p50' },
  {
    value: '4,237',
    unit: 'objects/s',
    label: 'Bulk write',
    note: 'About 4 minutes per million articles',
  },
]

export const secondaryFigures: readonly Figure[] = [
  {
    value: '192 / 405',
    unit: 'ms',
    label: 'Save to searchable during a 50,000-object bulk load',
    note: 'p50 / p95',
  },
  { value: '17.5', unit: 'ms', label: 'PATCH an object', note: 'p50' },
  { value: '11.5', unit: 'ms', label: 'Open the history view', note: 'p50' },
]
