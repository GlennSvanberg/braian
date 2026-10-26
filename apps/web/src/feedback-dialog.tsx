import { useEffect, useMemo, useState } from 'react'
import { toast } from 'sonner'
import { ApiError, api } from './api'
import { Checkbox } from './components/ui/checkbox'
import { Dialog, DialogContent, DialogDescription, DialogTitle } from './components/ui/dialog'
import {
  buildFeedbackContext,
  formatFeedbackPreview,
  screenshotDataUrl,
  type FeedbackCapture,
  type FeedbackUser,
} from './feedback'

type ObjectHint = { id: string; type?: string; key?: string; title?: string } | null

export function FeedbackDialog({
  open,
  onClose,
  user,
  object,
  capture,
}: {
  open: boolean
  onClose: () => void
  user: FeedbackUser
  object: ObjectHint
  capture: FeedbackCapture | null
}) {
  const [message, setMessage] = useState('')
  const [includeScreenshot, setIncludeScreenshot] = useState(false)
  const [includeLog, setIncludeLog] = useState(false)
  const [error, setError] = useState('')
  const [busy, setBusy] = useState(false)
  const context = useMemo(() => buildFeedbackContext(user, object), [user, object, open])
  const logSnapshot = capture?.log ?? []
  const preview = useMemo(
    () => formatFeedbackPreview(context, includeLog ? logSnapshot : null),
    [context, includeLog, logSnapshot],
  )
  const thumb = screenshotDataUrl(capture?.screenshot ?? null)

  useEffect(() => {
    if (!open) return
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.preventDefault()
        onClose()
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [open, onClose])

  useEffect(() => {
    if (!open) {
      setMessage('')
      setIncludeScreenshot(false)
      setIncludeLog(false)
      setError('')
      setBusy(false)
    }
  }, [open])

  const submit = () => {
    const trimmed = message.trim()
    if (!trimmed || busy) return
    setBusy(true)
    setError('')
    void (async () => {
      try {
        await api('/v1/feedback', {
          method: 'POST',
          body: JSON.stringify({
            message: trimmed,
            context,
            log: includeLog ? logSnapshot : null,
            screenshot: includeScreenshot ? capture?.screenshot ?? null : null,
          }),
        })
        toast.success('Feedback sent')
        onClose()
      } catch (reason: unknown) {
        const detail = reason instanceof ApiError ? reason.message : 'Could not send feedback'
        setError(detail)
      } finally {
        setBusy(false)
      }
    })()
  }

  return (
    <Dialog open={open} onOpenChange={(next) => { if (!next) onClose() }}>
      <DialogContent data-testid="feedback-dialog" data-feedback-exclude>
        <DialogTitle>Send feedback</DialogTitle>
        <DialogDescription>Share a bug report or idea with the team. Context is attached automatically.</DialogDescription>
        <label className="field">
          <span>Message</span>
          <textarea
            data-testid="feedback-message"
            rows={5}
            value={message}
            onChange={(event) => setMessage(event.target.value)}
            placeholder="What happened? What did you expect?"
            autoFocus
          />
        </label>
        {thumb ? (
          <div className="feedback-shot" data-testid="feedback-screenshot">
            <span className="muted">Screenshot preview</span>
            <img src={thumb} alt="App screenshot preview" />
          </div>
        ) : null}
        <label className="field">
          <span>Context preview</span>
          <pre className="feedback-preview" data-testid="feedback-preview">{preview}</pre>
        </label>
        <label className="check-row">
          <Checkbox
            checked={includeScreenshot}
            onCheckedChange={(value) => setIncludeScreenshot(value === true)}
            aria-label="Include screenshot"
          />
          <span>Include screenshot</span>
        </label>
        <label className="check-row">
          <Checkbox
            checked={includeLog}
            onCheckedChange={(value) => setIncludeLog(value === true)}
            aria-label="Include recent log"
          />
          <span>Include recent log</span>
        </label>
        {error ? <p className="error" role="alert">{error}</p> : null}
        <div className="dialog-actions">
          <button type="button" className="ghost" onClick={onClose} disabled={busy}>Cancel</button>
          <button type="button" onClick={submit} disabled={busy || !message.trim()}>
            {busy ? 'Sending…' : 'Send'}
          </button>
        </div>
      </DialogContent>
    </Dialog>
  )
}
