import { expect, test, type Page } from '@playwright/test'
import { demoCredentials } from './demo'

async function login(page: Page) {
  const demo = demoCredentials()
  await page.goto('/')
  await page.getByTestId('email').fill(demo.email)
  await page.getByTestId('password').fill(demo.password)
  await page.getByRole('button', { name: 'Sign in' }).click()
  await expect(page.getByTestId('dock')).toBeVisible({ timeout: 60_000 })
}

async function createPersonalWorkspace(page: Page, name: string) {
  const res = await page.request.post('/v1/workspaces', {
    data: {
      name,
      locked: false,
      share_token: `user-e2e-${crypto.randomUUID().slice(0, 8)}`,
      layout: { type: 'tabs', views: [], active: 0 },
    },
  })
  expect(res.ok()).toBeTruthy()
  const body = (await res.json()) as { id: string }
  return body.id
}

function uniqueName(prefix: string) {
  return `${prefix} ${crypto.randomUUID().slice(0, 8)}`
}

test('delete workspace from context menu and survives reload', async ({ page }) => {
  test.setTimeout(120_000)
  await login(page)
  const name = uniqueName('Delete me')
  await createPersonalWorkspace(page, name)
  await page.reload()
  await expect(page.getByTestId('dock')).toBeVisible({ timeout: 60_000 })
  const tab = page.locator('nav.tabs button.tab', { hasText: name })
  await expect(tab).toBeVisible({ timeout: 30_000 })
  await tab.click({ button: 'right' })
  const menu = page.getByTestId('workspace-menu')
  await expect(menu).toBeVisible()
  await menu.getByTestId('workspace-delete').click()
  await expect(page.getByTestId('workspace-delete-dialog')).toBeVisible()
  await page.getByTestId('workspace-delete-confirm').click()
  await expect(tab).toHaveCount(0, { timeout: 15_000 })
  await page.reload()
  await expect(page.getByTestId('dock')).toBeVisible({ timeout: 60_000 })
  await expect(page.locator('nav.tabs button.tab', { hasText: name })).toHaveCount(0)
})

test('delete workspace from Ctrl+K', async ({ page }) => {
  test.setTimeout(120_000)
  await login(page)
  const name = uniqueName('Palette delete')
  await createPersonalWorkspace(page, name)
  await page.reload()
  await expect(page.getByTestId('dock')).toBeVisible({ timeout: 60_000 })
  const tab = page.locator('nav.tabs button.tab', { hasText: name })
  await expect(tab).toBeVisible({ timeout: 30_000 })
  await tab.click()
  await expect(tab).toHaveClass(/active/)
  await page.keyboard.press('Control+K')
  const palette = page.getByTestId('command-palette')
  await expect(palette).toBeVisible()
  await palette.getByTestId('palette-input').fill('Delete workspace')
  await expect(palette.locator('.hit').filter({ hasText: 'Remove the active workspace' })).toBeVisible()
  await page.keyboard.press('Enter')
  await expect(page.getByTestId('workspace-delete-dialog')).toBeVisible({ timeout: 15_000 })
  await page.getByTestId('workspace-delete-confirm').click()
  await expect(page.locator('nav.tabs button.tab', { hasText: name })).toHaveCount(0, { timeout: 15_000 })
})

test('locked workspace cannot be deleted from the menu', async ({ page }) => {
  test.setTimeout(120_000)
  await login(page)
  const name = uniqueName('Lock delete')
  await createPersonalWorkspace(page, name)
  await page.reload()
  await expect(page.getByTestId('dock')).toBeVisible({ timeout: 60_000 })
  const tab = page.locator('nav.tabs button.tab', { hasText: name })
  await expect(tab).toBeVisible({ timeout: 30_000 })
  await tab.click({ button: 'right' })
  const menu = page.getByTestId('workspace-menu')
  await expect(menu).toBeVisible()
  await menu.getByRole('button', { name: /Lock workspace/i }).click()
  await tab.click({ button: 'right' })
  await expect(page.getByTestId('workspace-menu')).toBeVisible()
  await expect(page.getByTestId('workspace-delete')).toBeDisabled()
  // Unlock so leftover state does not confuse later runs
  await page.getByRole('button', { name: /Unlock workspace/i }).click()
})
