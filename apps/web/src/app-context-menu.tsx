import { useEffect, useState, type MouseEvent as ReactMouseEvent } from 'react'
import { copy } from './i18n'

export type AppMenuPoint = { x: number; y: number }

export const APP_CONTEXT_MENU_EVENT = 'braian:app-context-menu'

const textEntrySelector =
  'input, textarea, select, [contenteditable="true"], [contenteditable=""], [contenteditable="plaintext-only"]'

const surfaceSelector = '[role="grid"], [role="tree"], [role="menu"], [role="listbox"], [role="dialog"]'

let hostCount = 0

export function isTextEntryTarget(target: EventTarget | null): boolean {
  return target instanceof Element && Boolean(target.closest(textEntrySelector))
}

function clampPoint(x: number, y: number): AppMenuPoint {
  const width = typeof window === 'undefined' ? 0 : window.innerWidth
  const height = typeof window === 'undefined' ? 0 : window.innerHeight
  return {
    x: Math.max(8, Math.min(x, width > 0 ? width - 220 : x)),
    y: Math.max(8, Math.min(y, height > 0 ? height - 120 : y)),
  }
}

export function requestAppContextMenu(x: number, y: number) {
  if (typeof window === 'undefined') return
  window.dispatchEvent(new CustomEvent(APP_CONTEXT_MENU_EVENT, { detail: { x, y } }))
}

export function handleAppContextMenu(event: ReactMouseEvent) {
  if (event.defaultPrevented || isTextEntryTarget(event.target) || hostCount === 0) return
  event.preventDefault()
  requestAppContextMenu(event.clientX, event.clientY)
}

function keyboardFallbackAllowed(target: EventTarget | null) {
  if (!(target instanceof Element)) return false
  if (isTextEntryTarget(target) || target.closest(surfaceSelector)) return false
  return target === document.body || target === document.documentElement || Boolean(target.closest('.app-bar, .status, .workspace'))
}

export function AppContextMenu({
  point,
  feedbackEnabled,
  onSearch,
  onFeedback,
  onClose,
}: {
  point: AppMenuPoint
  feedbackEnabled?: boolean
  onSearch: () => void
  onFeedback: () => void
  onClose: () => void
}) {
  return (
    <div
      className="menu"
      data-testid="app-context-menu"
      role="menu"
      aria-label={copy.contextMenu}
      style={{ left: point.x, top: point.y }}
      onPointerDown={(event) => event.stopPropagation()}
    >
      <button
        type="button"
        role="menuitem"
        onClick={() => {
          onClose()
          onSearch()
        }}
      >
        {copy.search}…
      </button>
      {feedbackEnabled ? (
        <button
          type="button"
          role="menuitem"
          onClick={() => {
            onClose()
            onFeedback()
          }}
        >
          {copy.sendFeedback}
        </button>
      ) : null}
    </div>
  )
}

export function AppContextMenuHost({
  feedbackEnabled,
  onSearch,
  onFeedback,
}: {
  feedbackEnabled?: boolean
  onSearch: () => void
  onFeedback: () => void
}) {
  const [point, setPoint] = useState<AppMenuPoint | null>(null)

  useEffect(() => {
    hostCount += 1
    const onContextMenu = (event: MouseEvent) => {
      if (event.defaultPrevented || isTextEntryTarget(event.target)) return
      event.preventDefault()
      setPoint(clampPoint(event.clientX, event.clientY))
    }
    const onRequest = (event: Event) => {
      const detail = (event as CustomEvent<AppMenuPoint>).detail
      if (!detail) return
      setPoint(clampPoint(detail.x, detail.y))
    }
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.defaultPrevented) return
      if (!((event.key === 'F10' && event.shiftKey) || event.key === 'ContextMenu')) return
      if (!keyboardFallbackAllowed(event.target)) return
      event.preventDefault()
      const active = document.activeElement
      const rect = active instanceof HTMLElement ? active.getBoundingClientRect() : null
      const x = rect && rect.width > 0 ? rect.left : 8
      const y = rect && rect.height > 0 ? rect.bottom : 8
      setPoint(clampPoint(x, y))
    }
    document.addEventListener('contextmenu', onContextMenu)
    window.addEventListener(APP_CONTEXT_MENU_EVENT, onRequest)
    window.addEventListener('keydown', onKeyDown)
    return () => {
      hostCount = Math.max(0, hostCount - 1)
      document.removeEventListener('contextmenu', onContextMenu)
      window.removeEventListener(APP_CONTEXT_MENU_EVENT, onRequest)
      window.removeEventListener('keydown', onKeyDown)
    }
  }, [])

  useEffect(() => {
    if (!point) return
    document.querySelector<HTMLElement>('[data-testid="app-context-menu"] [role="menuitem"]')?.focus()
    const closeOnPointer = (event: PointerEvent) => {
      const target = event.target
      if (target instanceof Element && target.closest('[data-testid="app-context-menu"]')) return
      setPoint(null)
    }
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === 'Escape') setPoint(null)
    }
    window.addEventListener('pointerdown', closeOnPointer)
    window.addEventListener('keydown', closeOnEscape)
    return () => {
      window.removeEventListener('pointerdown', closeOnPointer)
      window.removeEventListener('keydown', closeOnEscape)
    }
  }, [point])

  if (!point) return null
  return (
    <AppContextMenu
      point={point}
      feedbackEnabled={feedbackEnabled}
      onSearch={onSearch}
      onFeedback={onFeedback}
      onClose={() => setPoint(null)}
    />
  )
}
