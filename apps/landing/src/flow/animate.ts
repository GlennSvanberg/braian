export function bindFlow() {
  const root = document.querySelector('[data-flow]')
  if (!(root instanceof HTMLElement) || root.dataset.flowBound === '1') return
  root.dataset.flowBound = '1'
  const reduce = window.matchMedia('(prefers-reduced-motion: reduce)').matches
  const desktop = window.matchMedia('(min-width: 801px)')
  if (reduce || !('IntersectionObserver' in window)) {
    root.classList.add('is-drawn')
    return
  }

  const draw = () => {
    const svg = root.querySelector(desktop.matches ? '.flow-wide' : '.flow-narrow')
    if (!(svg instanceof SVGSVGElement) || svg.dataset.drawn === '1') return
    svg.dataset.drawn = '1'
    svg.querySelectorAll('[data-draw]').forEach((node) => {
      if (!(node instanceof SVGGeometryElement)) return
      const len = node.getTotalLength()
      if (len < 1) return
      node.style.strokeDasharray = `${len}`
      node.style.strokeDashoffset = `${len}`
    })
    root.classList.add('can-draw')
  }

  const play = (svg: Element | null) => {
    if (!(svg instanceof SVGSVGElement) || svg.dataset.played === '1') return
    svg.dataset.played = '1'
    svg.querySelectorAll('animateMotion').forEach((node) => {
      if (!(node instanceof SVGAnimateMotionElement)) return
      const delay = Number(node.getAttribute('data-delay') || '0')
      const dot = node.parentElement
      window.setTimeout(() => {
        node.beginElement()
        if (dot instanceof SVGElement) dot.style.opacity = '1'
      }, delay)
    })
  }

  draw()
  desktop.addEventListener('change', () => {
    const svg = root.querySelector(desktop.matches ? '.flow-wide' : '.flow-narrow')
    if (root.dataset.animated === '1') {
      root.classList.add('is-drawn')
      play(svg)
    } else draw()
  })

  const start = () => {
    if (root.dataset.animated === '1') return
    root.dataset.animated = '1'
    root.classList.add('is-drawn')
    play(root.querySelector(desktop.matches ? '.flow-wide' : '.flow-narrow'))
  }

  const observer = new IntersectionObserver(
    (entries) => {
      if (!entries.some((entry) => entry.isIntersecting)) return
      start()
      observer.disconnect()
    },
    { threshold: 0.22 },
  )
  observer.observe(root)
}
