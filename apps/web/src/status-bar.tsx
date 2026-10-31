import { useEffect, useState } from 'react'
import { copy } from './i18n'
import { statusMessage, type LiveState } from './status-message'
import { useWriteStatus } from './write-status'

/** True while the browser reports a network connection. */
export function useOnline(): boolean {
  const [online, setOnline] = useState(() => (typeof navigator === 'undefined' ? true : navigator.onLine))
  useEffect(() => {
    const up = () => setOnline(true)
    const down = () => setOnline(false)
    window.addEventListener('online', up)
    window.addEventListener('offline', down)
    return () => {
      window.removeEventListener('online', up)
      window.removeEventListener('offline', down)
    }
  }, [])
  return online
}

/** Shows something only when it is needed. Empty when saved and connected. */
export function StatusBar({ online, live, locked }: { online: boolean; live: LiveState; locked: boolean }) {
  const writes = useWriteStatus()
  const [showSaving, setShowSaving] = useState(false)
  useEffect(() => {
    if (writes.pending === 0) {
      setShowSaving(false)
      return
    }
    // Skip the flash for writes that finish at once.
    const timer = window.setTimeout(() => setShowSaving(true), 300)
    return () => window.clearTimeout(timer)
  }, [writes.pending])
  const message = statusMessage(
    { saving: showSaving, online, live, saveError: writes.error, locked },
    { reconnecting: copy.reconnecting, saving: copy.saving, saveFailed: copy.saveFailed, locked: copy.locked },
  )
  return (
    <span
      className="status-bar"
      data-testid="status-bar"
      data-kind={message?.kind ?? ''}
      title={message?.title}
      role="status"
      aria-live="polite"
    >
      {message?.text ?? ''}
    </span>
  )
}
