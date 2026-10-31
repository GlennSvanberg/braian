import { setTheme } from './ui'
import { expect, test, type Page } from '@playwright/test'
import { mkdirSync } from 'node:fs'
import { demoCredentials } from './demo'

const shots = '/opt/cursor/artifacts/final'

async function resetProducts(page: Page) {
  const listed = await page.request.get('/v1/workspaces')
  const body = (await listed.json()) as { workspaces: Array<{ id: string; name: string }> }
  const enrich = body.workspaces.find((item) => item.name === 'Products')
  if (!enrich) throw new Error('missing Products')
  await page.evaluate(() => sessionStorage.setItem('braian.layout.suppressFlush', '1'))
  const me = await page.request.get('/v1/me')
  const userId = ((await me.json()) as { user_id?: string }).user_id
  if (userId) {
    await page.evaluate((key) => localStorage.removeItem(key), `braian.layout.${userId}.default.${enrich.id}`)
  }
  const deleted = await page.request.delete(`/v1/workspaces/${enrich.id}/state?environment=default`)
  expect(deleted.ok() || deleted.status() === 204).toBeTruthy()
  await page.reload()
  await expect(page.getByTestId('object-row').first()).toBeVisible()
  await page.evaluate(() => sessionStorage.removeItem('braian.layout.suppressFlush'))
}

async function login(page: Page) {
  const demo = demoCredentials()
  await page.goto('/')
  await page.getByTestId('email').fill(demo.email)
  await page.getByTestId('password').fill(demo.password)
  await page.getByRole('button', { name: 'Sign in' }).click()
  await expect(page.getByTestId('object-table')).toBeVisible()
  await expect(page.getByTestId('object-row').first()).toBeVisible()
  await resetProducts(page)
}

async function hold(page: Page, tabName: string, x: number, y: number) {
  const tab = page.locator('.pane-tab', { hasText: tabName }).first()
  const box = await tab.boundingBox()
  if (!box) throw new Error(`missing ${tabName}`)
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2)
  await page.mouse.down()
  await page.mouse.move(x, y, { steps: 14 })
  await expect(page.getByTestId('drag-ghost')).toBeVisible()
}

test('polish screenshots', async ({ page }) => {
  test.skip(!process.env.BRAIAN_FINAL, 'set BRAIAN_FINAL=1 against the demo server')
  test.setTimeout(180_000)
  mkdirSync(shots, { recursive: true })

  await page.goto('/')
  await expect(page.getByTestId('email')).toBeVisible()
  await page.screenshot({ path: `${shots}/login-dark.png` })
  await page.evaluate(() => localStorage.setItem('braian.theme', JSON.stringify('light')))
  await page.reload()
  await expect(page.getByTestId('email')).toBeVisible()
  await expect.poll(() => page.evaluate(() => document.documentElement.dataset.theme)).toBe('light')
  await page.screenshot({ path: `${shots}/login-light.png` })
  await page.evaluate(() => localStorage.setItem('braian.theme', JSON.stringify('dark')))
  await page.reload()

  await login(page)
  await page.screenshot({ path: `${shots}/enrich-dark.png` })
  await setTheme(page, 'light')
  await page.screenshot({ path: `${shots}/enrich-light.png` })
  await setTheme(page, 'dark')

  await page.getByTestId('object-row').first().click()
  await expect(page.getByTestId('object-detail')).toBeVisible()
  await expect(page.locator('.field-card').first()).toBeVisible()
  await page.screenshot({ path: `${shots}/detail-dark.png` })
  await setTheme(page, 'light')
  await page.screenshot({ path: `${shots}/detail-light.png` })
  await setTheme(page, 'dark')

  await page.getByTestId('object-row').nth(1).click({ button: 'right' })
  await expect(page.getByTestId('context-menu')).toBeVisible()
  await page.screenshot({ path: `${shots}/context-menu.png` })
  await page.getByTestId('context-menu').getByRole('button', { name: /Close/ }).click()

  await page.getByRole('button', { name: 'History' }).click()
  await expect(page.getByTestId('history').locator('article').first()).toBeVisible()
  await page.screenshot({ path: `${shots}/history-dark.png` })

  await page.getByRole('button', { name: 'Graph' }).click()
  await expect(page.locator('canvas[data-testid=graph]')).toBeVisible()
  await page.getByRole('button', { name: 'Open full size' }).click()
  await expect(page.getByTestId('graph-full')).toBeVisible()
  await page.screenshot({ path: `${shots}/graph-full-dark.png` })
  await page.getByRole('button', { name: 'Close full size' }).click()
  await setTheme(page, 'light')
  await page.getByRole('button', { name: 'Open full size' }).click()
  await expect(page.getByTestId('graph-full')).toBeVisible()
  await page.screenshot({ path: `${shots}/graph-full-light.png` })
  await page.getByRole('button', { name: 'Close full size' }).click()
  await setTheme(page, 'dark')

  await page.keyboard.press('Control+k')
  await expect(page.getByTestId('command-palette')).toBeVisible()
  await page.getByTestId('palette-input').fill('cheese')
  await expect(page.locator('.meta-chip').first()).toBeVisible()
  await page.screenshot({ path: `${shots}/palette-dark.png` })
  await page.keyboard.press('Escape')

  await page.keyboard.press('Control+j')
  await expect(page.getByTestId('command-palette')).toBeVisible()
  await expect(page.getByText('AI', { exact: true })).toBeVisible()
  await page.screenshot({ path: `${shots}/palette-ai-dark.png` })
  await page.keyboard.press('Escape')

  await page.getByRole('button', { name: 'Data model' }).click()
  await page.getByTestId('group-dairy').locator('.twist').click()
  await expect(page.getByTestId('group-yoghurt')).toBeVisible()
  await page.screenshot({ path: `${shots}/data-model-dark.png` })
  await setTheme(page, 'light')
  await page.screenshot({ path: `${shots}/data-model-light.png` })
  await setTheme(page, 'dark')
  await page.getByTestId('group-yoghurt').locator('.model-node').click()
  await expect(page.getByTestId('attribute-table')).toContainText('from Dairy')
  await page.screenshot({ path: `${shots}/class-card-dark.png` })

  await page.locator('nav.tabs').getByRole('button', { name: /^Review/ }).click()
  await expect(page.getByTestId('proposal')).toBeVisible()
  await page.screenshot({ path: `${shots}/review-dark.png` })

  await page.locator('nav.tabs').getByRole('button', { name: 'Products' }).click()
  await expect(page.getByTestId('object-row').first()).toBeVisible()
  const objects = page.locator('section.pane').filter({ has: page.locator('.pane-tab', { hasText: 'Products' }) })
  const box = await objects.boundingBox()
  if (!box) throw new Error('missing objects pane')
  await hold(page, 'Graph', box.x + box.width * 0.12, box.y + box.height * 0.55)
  await expect(page.getByTestId('drop-left')).toBeVisible()
  await page.screenshot({ path: `${shots}/dock-left.png` })
  await page.keyboard.press('Escape')
  await page.mouse.up()
  await expect(page.getByTestId('drag-ghost')).toHaveCount(0)

  await hold(page, 'Graph', box.x + box.width * 0.5, box.y + box.height * 0.5)
  await expect(page.getByTestId('drop-center')).toBeVisible()
  await page.screenshot({ path: `${shots}/dock-center.png` })
  await page.keyboard.press('Escape')
  await page.mouse.up()

  const leavesBefore = await page.getByTestId('dock').getAttribute('data-leaves')
  await page.locator('.pane-tab', { hasText: 'Products' }).first().dblclick()
  await expect(page.locator('section.pane')).toHaveCount(1)
  await page.screenshot({ path: `${shots}/maximized.png` })
  await page.locator('.pane-tab', { hasText: 'Products' }).first().dblclick()
  await expect(page.getByTestId('dock')).toHaveAttribute('data-zoom', '')
  await expect(page.getByTestId('dock')).toHaveAttribute('data-leaves', leavesBefore ?? '')
  await page.screenshot({ path: `${shots}/restored.png` })
  await resetProducts(page)
})
