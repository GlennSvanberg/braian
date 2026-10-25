const hoverQuery = '(hover: hover) and (pointer: fine)'

export function bindDisclosures() {
  const canHover = window.matchMedia(hoverQuery).matches

  document.querySelectorAll<HTMLElement>('[data-disclose-card]').forEach((card) => {
    if (card.dataset.discloseBound) return
    const button = card.querySelector<HTMLButtonElement>('[data-disclose]')
    const panelId = button?.getAttribute('aria-controls')
    const panel = panelId ? document.getElementById(panelId) : null
    if (!button || !(panel instanceof HTMLElement)) return
    card.dataset.discloseBound = '1'

    let pinned = false
    let hovered = false

    const apply = () => {
      const open = pinned || hovered
      button.setAttribute('aria-expanded', open ? 'true' : 'false')
      card.classList.toggle('is-open', open)
      if (open) panel.removeAttribute('inert')
      else panel.setAttribute('inert', '')
    }

    button.addEventListener('click', () => {
      pinned = !pinned
      if (!pinned) hovered = false
      apply()
    })

    if (canHover) {
      card.addEventListener('mouseenter', () => {
        hovered = true
        apply()
      })
      card.addEventListener('mouseleave', () => {
        hovered = false
        apply()
      })
    }

    card.addEventListener('keydown', (event) => {
      if (event.key !== 'Escape' || !pinned) return
      event.preventDefault()
      pinned = false
      hovered = false
      apply()
      button.focus()
    })
  })
}
