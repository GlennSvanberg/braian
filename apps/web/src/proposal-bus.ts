export type OpenProposal = { id: string }

type Listener = (proposal: OpenProposal) => void

const listeners = new Set<Listener>()

export function publishProposal(id: string) {
  if (!id) return
  for (const listen of listeners) listen({ id })
}

export function onProposal(listen: Listener) {
  listeners.add(listen)
  return () => {
    listeners.delete(listen)
  }
}
