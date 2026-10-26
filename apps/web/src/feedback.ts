import { readUiContext } from './ui-context'
import type { ClientLogEntry } from './feedback-log'
import { formatLogPreview, sanitizeRoute, sanitizeText } from './feedback-log'

export type FeedbackUser = {
  handle: string
  role: string
  email?: string
}

export type FeedbackContext = {
  route: string
  link: string
  build: string
  workspace: { id: string; name: string } | null
  view: { id: string; type: string; title: string } | null
  object: { id: string; type: string; title: string; key?: string } | null
  search: { filter: string; sort: string }
  user: FeedbackUser
}

export type FeedbackCapture = {
  screenshot: string | null
  log: ClientLogEntry[]
}

const SCREENSHOT_MAX_DIM = 1600

export function appBuildId(): string {
  return typeof __BRAIAN_BUILD__ === 'string' ? __BRAIAN_BUILD__ : 'dev'
}

export function buildFeedbackContext(
  user: FeedbackUser,
  object?: { id: string; type?: string; key?: string; title?: string } | null,
): FeedbackContext {
  const snapshot = readUiContext()
  const focused = snapshot.focused
  const pane = focused ? snapshot.panes.find((item) => item.id === focused) ?? null : snapshot.panes.find((item) => item.focused) ?? null
  const primary = snapshot.selection[0]
  const resolved = object?.id
    ? {
        id: object.id,
        type: String(object.type ?? primary?.type ?? ''),
        title: String(object.title ?? primary?.name ?? ''),
        key: object.key ?? primary?.key,
      }
    : primary
      ? { id: primary.id, type: primary.type, title: primary.name, key: primary.key }
      : null
  const route = sanitizeRoute(`${window.location.pathname}${window.location.search}${window.location.hash}`)
  const link = sanitizeLink(window.location.href)
  return {
    route,
    link,
    build: appBuildId(),
    workspace: snapshot.workspace,
    view: pane ? { id: pane.id, type: pane.type, title: pane.title } : null,
    object: resolved,
    search: { filter: snapshot.filter, sort: snapshot.sort },
    user,
  }
}

export async function captureAppScreenshot(): Promise<string | null> {
  try {
    const { domToPng } = await import('modern-screenshot')
    const dataUrl = await domToPng(document.body, {
      scale: 1,
      filter: (node) => {
        if (!(node instanceof HTMLElement)) return true
        return node.dataset.feedbackExclude === undefined
      },
    })
    return await downscalePngBase64(dataUrl, SCREENSHOT_MAX_DIM)
  } catch {
    return null
  }
}

async function downscalePngBase64(dataUrl: string, maxDim: number): Promise<string> {
  const image = new Image()
  await new Promise<void>((resolve, reject) => {
    image.onload = () => resolve()
    image.onerror = () => reject(new Error('image load failed'))
    image.src = dataUrl
  })
  const scale = Math.min(1, maxDim / Math.max(image.width, image.height, 1))
  const width = Math.max(1, Math.round(image.width * scale))
  const height = Math.max(1, Math.round(image.height * scale))
  const canvas = document.createElement('canvas')
  canvas.width = width
  canvas.height = height
  const context = canvas.getContext('2d')
  if (!context) return dataUrl.split(',')[1] ?? ''
  context.drawImage(image, 0, 0, width, height)
  const out = canvas.toDataURL('image/png')
  return out.split(',')[1] ?? ''
}

export function screenshotDataUrl(base64: string | null): string | null {
  if (!base64) return null
  return `data:image/png;base64,${base64}`
}

export function formatFeedbackPreview(context: FeedbackContext, log?: ClientLogEntry[] | null): string {
  const lines = [
    `Route: ${context.route}`,
    `Link: ${context.link}`,
    `Build: ${context.build}`,
    context.workspace ? `Workspace: ${context.workspace.name} (${context.workspace.id})` : 'Workspace: —',
    context.view ? `View: ${context.view.title} (${context.view.type})` : 'View: —',
    context.object
      ? `Object: ${context.object.title || context.object.key || context.object.id} · ${context.object.type} (${context.object.id})`
      : 'Object: —',
    `Filter: ${context.search.filter || '—'}`,
    `Sort: ${context.search.sort || '—'}`,
    `User: ${context.user.handle} (${context.user.role})`,
  ]
  if (log) lines.push('', formatLogPreview(log))
  return lines.join('\n')
}

function sanitizeLink(href: string): string {
  try {
    const url = new URL(href)
    return url.origin + sanitizeRoute(`${url.pathname}${url.search}${url.hash}`)
  } catch {
    return sanitizeText(href)
  }
}

export async function prepareFeedbackCapture(): Promise<FeedbackCapture> {
  const [{ snapshotClientLog }, screenshot] = await Promise.all([
    import('./feedback-log'),
    captureAppScreenshot(),
  ])
  return { screenshot, log: snapshotClientLog() }
}
