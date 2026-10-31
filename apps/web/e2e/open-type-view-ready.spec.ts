import { expect, test, type Page } from '@playwright/test'
import { demoCredentials } from './demo'

async function login(page: Page) {
  const demo = demoCredentials()
  await page.goto('/')
  await page.getByTestId('email').fill(demo.email)
  await page.getByTestId('password').fill(demo.password)
  await page.getByRole('button', { name: 'Sign in' }).click()
  await expect(page.getByTestId('object-table')).toBeVisible({ timeout: 60_000 })
}

test.describe('openTypeView waits for workspace (Notion 112)', () => {
  test('Ctrl+K Articles view opens after delayed workspace load', async ({ page }) => {
    test.setTimeout(120_000)
    await login(page)

    let release!: () => void
    const held = new Promise<void>((resolve) => {
      release = resolve
    })
    await page.route('**/v1/workspaces/*', async (route) => {
      const url = new URL(route.request().url())
      const path = url.pathname
      const detail = /^\/v1\/workspaces\/[^/]+$/.test(path)
      if (route.request().method() === 'GET' && detail) {
        await held
      }
      await route.continue()
    })

    await page.reload()
    await expect(page.getByTestId('layout-heal-loading')).toBeVisible({ timeout: 30_000 })

    await page.keyboard.press('Control+k')
    await expect(page.getByTestId('command-palette')).toBeVisible()
    await page.getByTestId('palette-input').fill('Articles view')
    const entry = page
      .getByTestId('command-palette')
      .locator('.hit')
      .filter({ hasText: 'Articles view' })
      .filter({ hasText: 'Open view' })
    await expect(entry.first()).toBeVisible()
    await entry.first().click()
    await expect(page.getByTestId('command-palette')).toHaveCount(0)
    await expect(page.getByText('Opening when the workspace is ready…')).toBeVisible()
    await expect(page.locator('.pane-tab', { hasText: 'Articles' })).toHaveCount(0)

    release()
    await expect(page.locator('.pane-tab', { hasText: 'Articles' })).toBeVisible({ timeout: 60_000 })
  })
})
