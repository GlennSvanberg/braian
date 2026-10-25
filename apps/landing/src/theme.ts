import './styles.css'

const storageKey = 'braian-theme'

function current(): 'light' | 'dark' {
  return document.documentElement.getAttribute('data-theme') === 'light' ? 'light' : 'dark'
}

function apply(theme: 'light' | 'dark') {
  const root = document.documentElement
  if (theme === 'light') root.setAttribute('data-theme', 'light')
  else root.removeAttribute('data-theme')
  const meta = document.querySelector('meta[name="theme-color"]')
  if (meta instanceof HTMLMetaElement) {
    const bg = getComputedStyle(root).getPropertyValue('--bg').trim()
    if (bg) meta.content = bg
  }
}

function storedTheme(): 'light' | 'dark' | null {
  try {
    const value = localStorage.getItem(storageKey)
    return value === 'light' || value === 'dark' ? value : null
  } catch {
    return null
  }
}

const saved = storedTheme()
if (saved) apply(saved)

if (!document.documentElement.dataset.themeBound) {
  document.documentElement.dataset.themeBound = '1'
  document.addEventListener('click', (event) => {
    const target = event.target
    if (!(target instanceof Element) || !target.closest('[data-theme-toggle]')) return
    const next = current() === 'light' ? 'dark' : 'light'
    try {
      localStorage.setItem(storageKey, next)
    } catch {
      /* private mode */
    }
    apply(next)
  })
}
