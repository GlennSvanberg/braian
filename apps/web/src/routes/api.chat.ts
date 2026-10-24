import { createFileRoute } from '@tanstack/react-router'
import { chat, toServerSentEventsResponse, toolDefinition } from '@tanstack/ai'
import { openaiText } from '@tanstack/ai-openai'
import { sv } from '../i18n'

const inputSchema = {
  type: 'object',
  properties: {
    name: { type: 'string', description: 'Operationens namn' },
    arguments: { type: 'object', additionalProperties: true },
  },
  required: ['name'],
}

function modelName(): 'gpt-6-luna' | 'gpt-6-luna-pro' | 'gpt-6-sol' {
  const name = process.env.BRAIAN_AGENT_MODEL
  if (name === 'gpt-6-luna-pro' || name === 'gpt-6-sol') return name
  return 'gpt-6-luna'
}

function available() {
  return Boolean(process.env.OPENAI_API_KEY?.trim()) && process.env.BRAIAN_AGENT_MOCK !== '1'
}

function fallbackResponse() {
  const messageId = 'braian-fallback'
  const runId = 'braian-fallback-run'
  const now = Date.now()
  const events = [
    { type: 'RUN_STARTED', threadId: 'braian', runId, timestamp: now },
    { type: 'TEXT_MESSAGE_START', messageId, role: 'assistant', timestamp: now },
    { type: 'TEXT_MESSAGE_CONTENT', messageId, delta: sv.agentUnavailable, timestamp: now },
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
        if (!available()) return fallbackResponse()
        const body = (await request.json()) as { messages?: unknown[] }
        const cookie = request.headers.get('cookie') ?? ''
        const apiBase = process.env.BRAIAN_API_PROXY ?? 'http://127.0.0.1:8090'
        const callOperation = toolDefinition({
          name: 'call_operation',
          description: 'Kör en registrerad Braian-operation för den inloggade personen. Skrivningar som kräver godkännande blir förslag.',
          inputSchema,
        }).server(async (args: unknown) => {
          const record = (args ?? {}) as { name?: string; arguments?: Record<string, unknown> }
          const response = await fetch(`${apiBase}/v1/operations/${record.name ?? ''}`, {
            method: 'POST',
            headers: { 'content-type': 'application/json', cookie },
            body: JSON.stringify(record.arguments ?? {}),
          })
          return response.json()
        })
        const stream = chat({
          adapter: openaiText(modelName()),
          threadId: 'braian',
          messages: body.messages as never,
          systemPrompts: [
            'Du är Braian-agenten och agerar för den inloggade personen. Skrivande ändringar ska bli förslag. Svara på svenska. Använd call_operation.',
          ],
          tools: [callOperation],
        })
        return toServerSentEventsResponse(stream)
      },
    },
  },
})
