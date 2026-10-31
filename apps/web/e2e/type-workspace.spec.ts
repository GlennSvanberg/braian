import { setTheme } from './ui'
import { expect, test, type Page } from '@playwright/test'
import { mkdirSync } from 'node:fs'
import { demoCredentials } from './demo'

const shots = '/opt/cursor/artifacts/type-workspaces'
mkdirSync(shots, { recursive: true })

async function login(page: Page) {
  await page.addInitScript(() => {
    localStorage.setItem('braian.theme', JSON.stringify('dark'))
  })
  const demo = demoCredentials()
  await page.goto('/')
  await page.getByTestId('email').fill(demo.email)
  await page.getByTestId('password').fill(demo.password)
  await page.getByRole('button', { name: 'Sign in' }).click()
  await expect(page.getByTestId('dock')).toBeVisible()
}

async function resetWorkspace(page: Page, name: string) {
  await page.evaluate(() => sessionStorage.setItem('braian.layout.suppressFlush', '1'))
  const listed = await page.request.get('/v1/workspaces')
  const body = (await listed.json()) as { workspaces: Array<{ id: string; name: string }> }
  const row = body.workspaces.find((item) => item.name === name)
  if (!row) throw new Error(`missing workspace ${name}`)
  const me = await page.request.get('/v1/me')
  const userId = ((await me.json()) as { user_id?: string }).user_id
  if (userId) {
    await page.evaluate((key) => localStorage.removeItem(key), `braian.layout.${userId}.default.${row.id}`)
  }
  const deleted = await page.request.delete(`/v1/workspaces/${row.id}/state?environment=default`)
  expect(deleted.ok() || deleted.status() === 204).toBeTruthy()
  await page.reload()
  await expect(page.getByTestId('dock')).toBeVisible()
  await page.evaluate(() => sessionStorage.removeItem('braian.layout.suppressFlush'))
  return row.id
}

test('each object type has its own workspace and class attributes', async ({ page }) => {
  test.setTimeout(120_000)
  await login(page)
  const tabs = page.locator('nav.tabs')
  await expect(tabs.getByRole('button', { name: 'Products' })).toBeVisible()
  await expect(tabs.getByRole('button', { name: 'Articles' })).toBeVisible()
  await expect(tabs.getByRole('button', { name: 'Suppliers' })).toBeVisible()
  await expect(tabs.getByRole('button', { name: 'Variants' })).toHaveCount(0)
  await expect(tabs.getByRole('button', { name: 'Data model' })).toBeVisible()
  await expect(tabs.getByRole('button', { name: /^Enrich/ })).toHaveCount(0)

  await resetWorkspace(page, 'Articles')
  await tabs.getByRole('button', { name: 'Articles' }).click()
  const table = page.getByTestId('object-table')
  await expect(table).toBeVisible()
  await expect(page.locator('.pane-tab', { hasText: 'Articles' })).toBeVisible()
  await expect(page.locator('.pane-tab', { hasText: 'Selections' })).toBeVisible()
  await page.getByTestId('object-row').first().click()
  await page.locator('[data-view-type="class_attributes"]').click()
  const classes = page.getByTestId('class-attributes')
  await expect(classes).toBeVisible()
  await expect(classes.getByTestId('class-section').first()).toBeVisible()
  await expect(classes).toContainText('Article')
  await page.screenshot({ path: `${shots}/articles-dark.png` })

  await setTheme(page, 'light')
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'light')
  await page.screenshot({ path: `${shots}/articles-light.png` })
  await setTheme(page, 'dark')
})
