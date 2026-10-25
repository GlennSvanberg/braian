import { createFileRoute } from '@tanstack/react-router'
import { sv } from '../i18n'

function modelName() {
  return process.env.BRAIAN_AGENT_MODEL?.trim() || 'gpt-6-luna'
}

function available() {
  return Boolean(process.env.OPENAI_API_KEY?.trim()) && process.env.BRAIAN_AGENT_MOCK !== '1'
}

function eventStream(text: string) {
  const messageId = 'braian-agent'
  const runId = 'braian-agent-run'
  const now = Date.now()
  const events = [
    { type: 'RUN_STARTED', threadId: 'braian', runId, timestamp: now },
    { type: 'TEXT_MESSAGE_START', messageId, role: 'assistant', timestamp: now },
    { type: 'TEXT_MESSAGE_CONTENT', messageId, delta: text, timestamp: now },
    { type: 'TEXT_MESSAGE_END', messageId, timestamp: now },
    { type: 'RUN_FINISHED', threadId: 'braian', runId, timestamp: now },
  ]
  const body = events.map((event) => `data: ${JSON.stringify(event)}\n\n`).join('')
  return new Response(body, { headers: { 'content-type': 'text/event-stream; charset=utf-8' } })
}

export const Route = createFileRoute('/api/chat')({
  server: {
    handlers: {
      GET: async () =>
        Response.json({
          available: available(),
          model: modelName(),
          reply: available() ? '' : sv.agentUnavailable,
        }),
      POST: async ({ request }) => {
        if (!available()) return eventStream(sv.agentUnavailable)
        const body = await request.text()
        const cookie = request.headers.get('cookie') ?? ''
        const apiBase = process.env.BRAIAN_API_PROXY ?? 'http://127.0.0.1:8090'
        const response = await fetch(`${apiBase}/v1/agent`, {
          method: 'POST',
          headers: { 'content-type': 'application/json', cookie },
          body,
        })
        const payload = (await response.json().catch(() => ({}))) as { reply?: string; detail?: string }
        const text = response.ok
          ? payload.reply || sv.agentEmpty
          : payload.detail || 'Modellen svarade inte.'
        return eventStream(text)
      },
    },
  },
})
