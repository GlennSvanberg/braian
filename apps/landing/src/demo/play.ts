import { timeline } from '@/demo/timeline'

export function bindDemo() {
  const root = document.querySelector('[data-demo]')
  if (!(root instanceof HTMLElement) || root.dataset.demoBound === '1') return
  root.dataset.demoBound = '1'
  if (window.matchMedia('(prefers-reduced-motion: reduce)').matches) return

  const typed = root.querySelector('[data-type]')
  const word = typed?.textContent ?? ''
  let index = 0
  let timer = 0
  let visible = false

  const typeOut = () => {
    if (!typed) return
    typed.textContent = ''
    const each = Math.min(110, Math.floor(timeline[index].ms / (word.length + 4)))
    for (let i = 1; i <= word.length; i++) {
      window.setTimeout(() => {
        typed.textContent = word.slice(0, i)
      }, i * each)
    }
  }

  const show = (next: number) => {
    index = next
    const scene = timeline[index]
    root.dataset.scene = scene.id
    if (scene.id === 's5') typeOut()
  }

  const tick = () => {
    if (!visible || document.hidden) return
    show((index + 1) % timeline.length)
    timer = window.setTimeout(tick, timeline[index].ms)
  }

  const resume = () => {
    window.clearTimeout(timer)
    if (visible && !document.hidden) timer = window.setTimeout(tick, timeline[index].ms)
  }

  new IntersectionObserver(
    (entries) => {
      visible = entries.some((entry) => entry.isIntersecting)
      resume()
    },
    { threshold: 0.25 },
  ).observe(root)
  document.addEventListener('visibilitychange', resume)
}
