/**
 * Published landing figures. Edit this file when a run is remeasured.
 * Every figure stays tied to the hardware and data size of its set.
 * Leave unreached goals out of this file.
 */

export const measurementsDoc = {
  href: 'https://github.com/GlennSvanberg/openpim/blob/main/docs/measurements/2026-10-25-perf-v4.md',
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
    id: 'v4-10k',
    name: 'Perf v4',
    hardware: '8 vCPU AMD EPYC-Rome, 15.6 GB RAM',
    dataSize: '10,000 medium objects, 301 attributes × 10 languages, 10 searchable fields',
    recordedOn: '2026-10-25',
    conditions: 'Postgres, Redpanda, Typesense and Braian on the same machine',
    figures: [
      {
        value: '64 ms',
        label: 'Searchable after an edit, p50',
        note: 'Idle, API path, n=443, p95 84 ms, p99 144 ms. The probe polls every 25 ms. During the 10,000-object write, n=13: 126 / 195 / 250 ms.',
      },
      {
        value: '20 ms',
        label: 'PATCH to SSE event, p50',
        note: 'Idle at 10,000 objects, p95 28 ms. Idle at 50,000: p50 20 ms, p95 26 ms. All 1,066 probes got an event.',
      },
      {
        value: '17.5 ms',
        label: 'PATCH, p50',
        note: 'Idle bench, median of 3 runs, p95 21.5 ms. API client idle: 21 / 27 ms.',
      },
      {
        value: '9.1 ms',
        label: 'GET by id, p50',
        note: 'Idle bench, p95 11.4 ms. API client idle: 14 / 17 ms.',
      },
      {
        value: '4,237',
        label: 'Bulk write, objects/s',
        note: 'Postgres only, 10,000 objects: 3,848 / 4,237 / 4,300. Short of 5,000 end to end. Full pipeline: 4,208 (3,534 / 4,294 / 4,208). Store work on pregenerated values: 5,086 objects/s.',
      },
      {
        value: 'about 15 ms',
        label: 'Search',
        note: '/v1/search itself, inside the API-path probe.',
      },
    ],
  },
  {
    id: 'v4-50k',
    name: 'Perf v4, sustained',
    hardware: '8 vCPU AMD EPYC-Rome, 15.6 GB RAM',
    dataSize: '50,000 medium objects, 301 attributes × 10 languages, 10 searchable fields',
    recordedOn: '2026-10-25',
    conditions: 'Same machine as the 10,000-object run',
    figures: [
      {
        value: '405 ms',
        label: 'Searchable after an edit, p95',
        note: 'During the 50,000-object bulk write, API path, n=57, p50 192 ms, p99 478 ms. Idle, n=60: 78 / 103 / 116 ms. Index backlog, n=170: 76 / 232 / 621 ms.',
      },
      {
        value: '4,033',
        label: 'Bulk write, objects/s',
        note: 'Postgres only. Full pipeline 3,781 (3,681 with the API client). Short of 5,000 end to end.',
      },
      {
        value: '439 docs/s',
        label: 'Indexing',
        note: '50,000 objects. 10,000 objects, three clean runs: 490 (477 / 531 / 490).',
      },
    ],
  },
]
