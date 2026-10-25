/**
 * Published landing figures. Edit this file when a run is remeasured.
 * Every figure stays tied to the hardware and data size of its set.
 * Leave unreached goals out of this file.
 */

export const measurementsDoc = {
  href: 'https://github.com/GlennSvanberg/openpim/blob/main/docs/measurements/2026-10-25-perf-v3-10k.md',
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
    id: 'v3-10k',
    name: 'Perf v3',
    hardware: '8 vCPU AMD EPYC-Rome, 15.6 GB RAM',
    dataSize: '10,000 objects, 301 attributes × 10 languages',
    recordedOn: '2026-10-25',
    conditions: 'Postgres, Redpanda, Typesense and Braian on the same machine',
    figures: [
      {
        value: '3,022 / 3,104 / 3,031',
        label: 'Bulk write, objects/s',
        note: 'Three runs of 10,000 objects. A run with values already built wrote 3,116 objects/s.',
      },
      {
        value: '17.2 ms',
        label: 'PATCH, p50',
        note: 'p95 21.4 ms. PATCH then GET p50/p95 24.9 / 29.8 ms.',
      },
      {
        value: '7.1 ms',
        label: 'GET by id, p50',
        note: 'p95 8.8 ms. 32 concurrent clients: p50/p95 18.3 / 25.6 ms, 1,698 req/s.',
      },
      {
        value: '92.8 ms',
        label: 'Read 100 ids, p50',
        note: 'p95 119.5 ms.',
      },
      {
        value: '1.20 s',
        label: 'Searchable after a change, p50',
        note: 'During the 10,000-object load, n=13, p95 2.07 s. All 60 samples in that run: p50 0.62 s, p95 1.65 s.',
      },
      {
        value: '16.2 ms',
        label: 'Search, p50',
        note: 'p95 18.9 ms. 32 concurrent clients: p50/p95 41.8 / 79.6 ms, 714 req/s.',
      },
      {
        value: '516 docs/s',
        label: 'Indexing',
        note: '2 s peak 603 docs/s. Reindex of 10,000 documents: 17.0 s (587 docs/s).',
      },
      {
        value: '204',
        label: 'Relay health',
        note: 'Idle, after the load, and 30 s after a 50,000-object load.',
      },
    ],
  },
  {
    id: 'v3-50k',
    name: 'Perf v3, sustained',
    hardware: '8 vCPU AMD EPYC-Rome, 15.6 GB RAM',
    dataSize: '50,000 objects, 301 attributes × 10 languages',
    recordedOn: '2026-10-25',
    conditions: 'Same machine as the 10,000-object run',
    figures: [
      {
        value: '2.07 s',
        label: 'Searchable after a change, p50',
        note: 'During the load, n=87, p95 4.09 s, p99 4.86 s.',
      },
      {
        value: '475 docs/s',
        label: 'Indexing',
      },
    ],
  },
]
