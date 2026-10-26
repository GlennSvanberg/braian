import { expect, test, type Page } from '@playwright/test'
import { demoCredentials } from './demo'

async function login(page: Page) {
  const demo = demoCredentials()
  await page.setViewportSize({ width: 1280, height: 720 })
  await page.goto('/')
  await page.getByTestId('email').fill(demo.email)
  await page.getByTestId('password').fill(demo.password)
  await page.getByRole('button', { name: 'Sign in' }).click()
  await expect(page.getByTestId('object-row').first()).toBeVisible()
}

test('details pane keeps a single scroll container that fills the pane', async ({ page }) => {
  test.setTimeout(60_000)
  await login(page)
  await page.getByTestId('object-row').first().click()
  await page.locator('[data-view-type="object_detail"]').first().click()
  const detail = page.getByTestId('object-detail')
  await expect(detail.locator('.field').first()).toBeVisible()

  const detailReport = await detail.evaluate((form) => {
    const field = form.querySelector('[data-field]') as HTMLElement | null
    const scroller = form.querySelector('.detail-scroll') as HTMLElement | null
    if (!field || !scroller) return { count: -1, names: [] as string[], fills: false, scrollerFills: false, gap: -1, moved: 0, distance: 0 }
    const pane = form.closest('section.pane')
    const body = form.closest('.pane-body') as HTMLElement | null
    if (!pane || !body) return { count: -1, names: [] as string[], fills: false, scrollerFills: false, gap: -1, moved: 0, distance: 0 }
    const names: string[] = []
    let node = field.parentElement
    while (node && pane.contains(node)) {
      const overflow = getComputedStyle(node).overflowY
      if ((overflow === 'auto' || overflow === 'scroll') && node.scrollHeight > node.clientHeight + 1) {
        names.push(String(node.className || node.tagName))
      }
      node = node.parentElement
    }
    const padTop = Number.parseFloat(getComputedStyle(body).paddingTop) || 0
    const padBottom = Number.parseFloat(getComputedStyle(body).paddingBottom) || 0
    const bodyBox = body.getBoundingClientRect()
    const box = form.getBoundingClientRect()
    const fills = Math.abs(box.top - (bodyBox.top + padTop)) <= 2 && Math.abs(box.bottom - (bodyBox.bottom - padBottom)) <= 2
    const scrollBox = scroller.getBoundingClientRect()
    const scrollerFills = Math.abs(scrollBox.top - box.top) <= 2 && Math.abs(scrollBox.bottom - box.bottom) <= 2
    const before = field.getBoundingClientRect().top
    const distance = Math.min(160, scroller.scrollHeight - scroller.clientHeight)
    scroller.scrollTop = distance
    const moved = before - field.getBoundingClientRect().top
    scroller.scrollTop = scroller.scrollHeight
    const fields = scroller.querySelectorAll('.field')
    const last = fields[fields.length - 1] as HTMLElement | undefined
    const gap = last ? scroller.getBoundingClientRect().bottom - last.getBoundingClientRect().bottom : -1
    return { count: names.length, names, fills, scrollerFills, gap, moved, distance }
  })

  expect(detailReport.count, JSON.stringify(detailReport)).toBe(1)
  expect(detailReport.names[0], JSON.stringify(detailReport)).toContain('detail-scroll')
  expect(detailReport.fills, JSON.stringify(detailReport)).toBe(true)
  expect(detailReport.scrollerFills, JSON.stringify(detailReport)).toBe(true)
  expect(detailReport.distance, JSON.stringify(detailReport)).toBeGreaterThan(80)
  expect(detailReport.moved, JSON.stringify(detailReport)).toBeGreaterThan(80)
  expect(detailReport.gap, JSON.stringify(detailReport)).toBeGreaterThanOrEqual(-2)
  expect(detailReport.gap, JSON.stringify(detailReport)).toBeLessThan(48)

  const gridReport = await page.getByTestId('object-table').evaluate((table) => {
    const row = table.querySelector('[data-testid="object-row"]') as HTMLElement | null
    const grid = table.querySelector('.grid') as HTMLElement | null
    const body = table.closest('.pane-body') as HTMLElement | null
    if (!row || !grid || !body) return { count: -1, names: [] as string[], bottom: -1 }
    const names: string[] = []
    let node = row.parentElement
    const pane = table.closest('section.pane')
    while (node && pane?.contains(node)) {
      const overflow = getComputedStyle(node).overflowY
      if ((overflow === 'auto' || overflow === 'scroll') && node.scrollHeight > node.clientHeight + 1) names.push(String(node.className || node.tagName))
      node = node.parentElement
    }
    const padBottom = Number.parseFloat(getComputedStyle(body).paddingBottom) || 0
    const bottom = Math.abs(grid.getBoundingClientRect().bottom - (body.getBoundingClientRect().bottom - padBottom))
    return { count: names.length, names, bottom }
  })
  expect(gridReport.count, JSON.stringify(gridReport)).toBe(1)
  expect(gridReport.names[0], JSON.stringify(gridReport)).toContain('grid')
  expect(gridReport.bottom, JSON.stringify(gridReport)).toBeLessThanOrEqual(2)

  await page.locator('.pane-tab', { hasText: 'Graph' }).first().click()
  await expect(page.locator('canvas[data-testid=graph]')).toBeVisible()
  const graphReport = await page.locator('canvas[data-testid=graph]').evaluate((canvas) => {
    const pane = canvas.closest('section.pane')
    const body = canvas.closest('.pane-body') as HTMLElement | null
    const stage = canvas.closest('.graph-stage') as HTMLElement | null
    if (!pane || !body || !stage) return { count: -1, names: [] as string[], stageFills: false, stageHeight: 0 }
    const names: string[] = []
    let node = canvas.parentElement
    while (node && pane.contains(node)) {
      const overflow = getComputedStyle(node).overflowY
      if ((overflow === 'auto' || overflow === 'scroll') && node.scrollHeight > node.clientHeight + 1) names.push(String(node.className || node.tagName))
      node = node.parentElement
    }
    const padBottom = Number.parseFloat(getComputedStyle(body).paddingBottom) || 0
    const stageBox = stage.getBoundingClientRect()
    const bodyBox = body.getBoundingClientRect()
    return {
      count: names.length,
      names,
      stageFills: Math.abs(stageBox.bottom - (bodyBox.bottom - padBottom)) <= 2,
      stageHeight: stageBox.height,
    }
  })
  expect(graphReport.count, JSON.stringify(graphReport)).toBe(0)
  expect(graphReport.stageFills, JSON.stringify(graphReport)).toBe(true)
  expect(graphReport.stageHeight, JSON.stringify(graphReport)).toBeGreaterThan(200)

  await page.locator('.pane-tab', { hasText: 'History' }).first().click()
  const history = page.getByTestId('history')
  await expect(history.locator('article').first()).toBeVisible()
  const historyReport = await history.evaluate((root) => {
    const article = root.querySelector('article') as HTMLElement | null
    const body = root.closest('.pane-body') as HTMLElement | null
    const pane = root.closest('section.pane')
    if (!article || !body || !pane) return { count: -1, names: [] as string[], fills: false }
    const names: string[] = []
    let node = article.parentElement
    while (node && pane.contains(node)) {
      const overflow = getComputedStyle(node).overflowY
      if ((overflow === 'auto' || overflow === 'scroll') && node.scrollHeight > node.clientHeight + 1) names.push(String(node.className || node.tagName))
      node = node.parentElement
    }
    const padTop = Number.parseFloat(getComputedStyle(body).paddingTop) || 0
    const padBottom = Number.parseFloat(getComputedStyle(body).paddingBottom) || 0
    const bodyBox = body.getBoundingClientRect()
    const box = root.getBoundingClientRect()
    return {
      count: names.length,
      names,
      fills: Math.abs(box.top - (bodyBox.top + padTop)) <= 2 && Math.abs(box.bottom - (bodyBox.bottom - padBottom)) <= 2,
    }
  })
  expect(historyReport.count, JSON.stringify(historyReport)).toBeLessThanOrEqual(1)
  expect(historyReport.fills, JSON.stringify(historyReport)).toBe(true)
})
