import { expect, test, type Page } from '@playwright/test'
import { writeFileSync } from 'node:fs'

async function login(page: Page) {
  const started = Date.now()
  await page.goto('/')
  await page.getByTestId('email').fill(process.env.BRAIAN_DEMO_EMAIL ?? 'admin@braian.local')
  await page.getByTestId('password').fill(process.env.BRAIAN_DEMO_PASSWORD ?? 'braian-demo')
  await page.getByRole('button', { name: 'Logga in' }).click()
  await expect(page.getByTestId('object-row').first()).toBeVisible()
  return Date.now() - started
}

test('table load, filter and scroll', async ({ page }) => {
  test.skip(!process.env.BRAIAN_SCALE, 'set BRAIAN_SCALE=1 against a 100k catalog')
  const loadMs = await login(page)
  const found = await page.getByTestId('row-count').innerText()
  const filterStarted = Date.now()
  await page.getByTestId('quick-filter').fill('Kupol')
  await expect(page.getByTestId('object-row').first()).toContainText('Kupol', { timeout: 10_000 })
  const filterMs = Date.now() - filterStarted
  await page.getByTestId('quick-filter').fill('')
  await expect(page.getByTestId('row-count')).toHaveText(/100013/)
  const scroll = await page.evaluate(async () => {
    const grid = document.querySelector('.grid') as HTMLElement | null
    if (!grid) return { ms: -1, frames: 0, slowest: -1, scrollHeight: 0, scrollTop: 0, clientHeight: 0 }
    const height = grid.scrollHeight
    const steps = 80
    const step = Math.max(1, Math.floor((height - grid.clientHeight) / steps))
    const started = performance.now()
    let frames = 0
    let slowest = 0
    for (let top = 0; top < height; top += step) {
      const frameStart = performance.now()
      grid.scrollTop = top
      await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()))
      slowest = Math.max(slowest, performance.now() - frameStart)
      frames += 1
    }
    grid.scrollTop = height
    await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()))
    return {
      ms: Math.round(performance.now() - started),
      frames: frames + 1,
      slowest: Math.round(slowest),
      scrollHeight: height,
      scrollTop: grid.scrollTop,
      clientHeight: grid.clientHeight,
    }
  })
  const report = { found, loadMs, filterMs, scrollMs: scroll.ms, frames: scroll.frames, slowestFrameMs: scroll.slowest, scrollHeight: scroll.scrollHeight, scrollTop: scroll.scrollTop }
  console.log(JSON.stringify(report))
  const out = process.env.BRAIAN_MEASURE_OUT
  if (out) writeFileSync(out, `${JSON.stringify(report, null, 2)}\n`)
  expect(scroll.scrollTop + scroll.clientHeight).toBeGreaterThanOrEqual(scroll.scrollHeight - 2)
  expect(scroll.slowest).toBeLessThan(80)
  expect(loadMs).toBeLessThan(8_000)
  expect(filterMs).toBeLessThan(3_000)
})
