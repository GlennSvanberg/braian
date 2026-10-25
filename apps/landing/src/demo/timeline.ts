export type Scene = { id: string; ms: number }

export const timeline: readonly Scene[] = [
  { id: 's0', ms: 1900 },
  { id: 's1', ms: 650 },
  { id: 's2', ms: 650 },
  { id: 's3', ms: 1500 },
  { id: 's4', ms: 900 },
  { id: 's5', ms: 1500 },
  { id: 's6', ms: 1500 },
  { id: 's7', ms: 2600 },
]

/** Scenes hide every `[data-show]` element whose list does not name them, and mark `[data-sel]` and `[data-flash]` the same way. */
export function sceneCss(scope: string) {
  return timeline
    .map(({ id }) =>
      [
        `${scope}[data-scene="${id}"] [data-show]:not([data-show~="${id}"]){display:none}`,
        `${scope}[data-scene="${id}"] [data-sel~="${id}"]{background:var(--accent-dim);color:var(--text)}`,
        `${scope}[data-scene="${id}"] [data-flash~="${id}"]{animation:demo-flash 900ms var(--ease)}`,
      ].join(''),
    )
    .join('')
}
