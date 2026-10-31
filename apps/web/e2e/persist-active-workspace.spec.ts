import { expect, test, type Page } from '@playwright/test'
import { demoCredentials } from './demo'

async function login(page: Page) {
  const demo = demoCredentials()
  await page.goto('/')
  await page.getByTestId('email').fill(demo.email)
  await page.getByTestId('password').fill(demo.password)
  await page.getByRole('button', { name: 'Sign in' }).click()
  await expect(page.getByTestId('dock')).toBeVisible()
}

function activeTab(page: Page) {
  return page.locator('nav.tabs button.tab.active')
}

test('reload restores the active workspace; unknown id falls back', async ({ page }) => {
  test.setTimeout(120_000)
  await login(page)

  const tabs = page.locator('nav.tabs')
  await expect(tabs.getByRole('button', { name: /^Articles/ })).toBeVisible()

  // Non-default workspace.
  await tabs.getByRole('button', { name: /^Articles/ }).click()
  await expect(activeTab(page)).toHaveText(/^Articles/)

  await page.reload()
  await expect(page.getByTestId('dock')).toBeVisible()
  await expect(activeTab(page)).toHaveText(/^Articles/, { timeout: 30_000 })

  // Unknown / removed workspace id → default (first type workspace, Products).
  const me = await page.request.get('/v1/me')
  expect(me.ok()).toBeTruthy()
  const userId = ((await me.json()) as { user_id?: string }).user_id
  expect(userId).toBeTruthy()
  await page.evaluate((key) => localStorage.setItem(key, JSON.stringify('00000000-0000-4000-8000-000000000000')), `braian.activeWorkspace.${userId}`)

  await page.reload()
  await expect(page.getByTestId('dock')).toBeVisible()
  await expect(activeTab(page)).toHaveText(/^Products/, { timeout: 30_000 })
})
