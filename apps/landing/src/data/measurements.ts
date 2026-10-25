/**
 * Published landing figures. Edit this file when a run is remeasured.
 * Every figure stays tied to the hardware and data size of its set.
 * Leave unreached goals out of this file.
 */

export const measurementsDoc = {
  href: 'https://github.com/GlennSvanberg/openpim/blob/main/docs/measurements/2026-10-25-index-pipeline.md',
  label: 'Measurement notes',
} as const

export const measuredLabel = 'Measured'

export type MeasuredFigure = {
  value: string
  label: string
  note?: string
}

export type MeasuredSet = {
  id: string
  name: string
  hardware: string
  dataSize: string
  recordedOn: string
  conditions?: string
  figures: readonly MeasuredFigure[]
}

export const measuredSets: readonly MeasuredSet[] = [
  {
    id: 'a',
    name: 'Set A',
    hardware: '4 vCPU / 15 GiB VM',
    dataSize: '8,000 objects',
    recordedOn: '2026-10-25',
    conditions: 'Relay and indexer running',
    figures: [
      {
        value: '0.24 s',
        label: 'Edit searchable after commit, during bulk load',
        note: 'Previously 34.5 s. Stated target: under 5 s p95.',
      },
      {
        value: '9,323 docs/s',
        label: 'Full reindex',
      },
      {
        value: '797 events/s',
        label: 'Relay',
        note: 'All 8,000 documents searchable in 10.2 s.',
      },
      {
        value: '77 objects/s',
        label: 'Heavy-object import',
        note: '1.2 GiB max RSS.',
      },
    ],
  },
  {
    id: 'b',
    name: 'Set B',
    hardware: '8 vCPU VPS',
    dataSize: '1M objects',
    recordedOn: '2026-10-25',
    figures: [
      {
        value: '8.6 ms',
        label: 'Get by id, p50',
        note: '1 client.',
      },
      {
        value: '22.7 ms',
        label: 'Get by id, p50',
        note: '32 concurrent clients, ~1,390 req/s.',
      },
      {
        value: '~1,000 objects/s',
        label: 'Bulk write of medium objects',
      },
    ],
  },
]
