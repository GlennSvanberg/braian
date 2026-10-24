import { createFileRoute } from '@tanstack/react-router'

export const Route = createFileRoute('/embed/preview')({
  component: Preview,
})

function Preview() {
  const params = new URLSearchParams(typeof window === 'undefined' ? '' : window.location.search)
  return (
    <main style={{ fontFamily: 'system-ui', padding: 12 }}>
      <p>Inbäddad vy</p>
      <p>objekt {params.get('object') ?? '—'}</p>
    </main>
  )
}
