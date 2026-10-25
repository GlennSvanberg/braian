import type { Json } from './api'

export type AgentEvent = {
  type: 'step' | 'tool' | 'proposal' | 'answer' | 'error'
  id?: string
  op?: string
  args?: Json
  state?: 'run' | 'done'
  name?: string
  result?: Json
  proposal?: Json
  text?: string
  available?: boolean
}

export async function streamAgent(
  messages: Array<{ role: string; content: string }>,
  onEvent: (event: AgentEvent) => void,
  signal?: AbortSignal,
) {
  const response = await fetch('/v1/agent', {
    method: 'POST',
    credentials: 'include',
    headers: { 'content-type': 'application/json', accept: 'text/event-stream' },
    body: JSON.stringify({ messages, stream: true }),
    signal,
  })
  const type = response.headers.get('content-type') ?? ''
  if (!response.ok) {
    const body = (await response.json().catch(() => ({}))) as Json
    throw new Error(typeof body.detail === 'string' ? body.detail : response.statusText)
  }
  if (!type.includes('text/event-stream') || !response.body) {
    const body = (await response.json()) as Json
    const tools = Array.isArray(body.tools) ? body.tools : []
    for (const tool of tools) {
      const record = tool as Json
      onEvent({ type: 'tool', name: String(record.name ?? ''), result: (record.result as Json) ?? {} })
    }
    onEvent({ type: 'answer', text: String(body.reply ?? body.detail ?? ''), available: body.available !== false })
    return
  }
  const reader = response.body.getReader()
  const decoder = new TextDecoder()
  let buffer = ''
  while (true) {
    const chunk = await reader.read()
    if (chunk.done) break
    buffer += decoder.decode(chunk.value, { stream: true })
    const parts = buffer.split('\n\n')
    buffer = parts.pop() ?? ''
    for (const part of parts) {
      const line = part.split('\n').find((item) => item.startsWith('data:'))
      if (!line) continue
      onEvent(JSON.parse(line.slice(5).trim()) as AgentEvent)
    }
  }
}
