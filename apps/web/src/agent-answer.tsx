import type { ReactNode } from 'react'

/** Split agent answer text into plain runs and bold (`**…**`) spans. No extra dependency. */
export function renderAgentAnswer(text: string): ReactNode {
  const parts: ReactNode[] = []
  const pattern = /\*\*(.+?)\*\*/g
  let last = 0
  let match: RegExpExecArray | null
  let key = 0
  while ((match = pattern.exec(text)) !== null) {
    if (match.index > last) {
      parts.push(text.slice(last, match.index))
    }
    parts.push(<strong key={`b${key++}`}>{match[1]}</strong>)
    last = match.index + match[0].length
  }
  if (last < text.length) {
    parts.push(text.slice(last))
  }
  return parts.length > 0 ? parts : text
}

/** Plain-text form used in tests when React is not needed. */
export function stripAgentMarkdown(text: string): string {
  return text.replace(/\*\*(.+?)\*\*/g, '$1')
}
