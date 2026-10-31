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

async function deleteShareToken(page: Page, token: string) {
  const listed = await page.request.get('/v1/workspaces')
  expect(listed.ok()).toBeTruthy()
  const body = (await listed.json()) as {
    workspaces: Array<{ id: string; share_token?: string | null; name: string }>
  }
  const row = body.workspaces.find((item) => item.share_token === token)
  if (!row) return
  const deleted = await page.request.delete(`/v1/workspaces/${row.id}`)
  expect(deleted.ok() || deleted.status() === 204).toBeTruthy()
}

async function listWorkspaces(page: Page) {
  const listed = await page.request.get('/v1/workspaces')
  expect(listed.ok()).toBeTruthy()
  return (await listed.json()) as {
    workspaces: Array<{ id: string; share_token?: string | null; name: string }>
  }
}

async function ensureReviewExists(page: Page) {
  const body = await listWorkspaces(page)
  if (body.workspaces.some((item) => item.share_token === 'braian-review')) return
  await page.keyboard.press('Control+K')
  const palette = page.getByTestId('command-palette')
  await expect(palette).toBeVisible()
  await palette.getByTestId('palette-input').fill('New from template')
  await expect(palette.getByRole('option', { name: /^New from template/ })).toBeVisible()
  await page.keyboard.press('Enter')
  await expect(palette.locator('.crumb')).toContainText('New from template')
  await palette.getByTestId('palette-input').fill('Review')
  await expect(palette.getByRole('option', { name: /^Review/ })).toBeVisible()
  await page.keyboard.press('Enter')
  await expect(page.locator('nav.tabs button.tab', { hasText: 'Review' })).toBeVisible({ timeout: 30_000 })
  await page.reload()
  await expect(page.getByTestId('dock')).toBeVisible({ timeout: 60_000 })
}

test('tab menu New from template opens the template submenu and creates a tab', async ({ page }) => {
  test.setTimeout(120_000)
  await login(page)

  // Remove Review so Enter creates a fresh tab via canonicalShareToken braian-review.
  await deleteShareToken(page, 'braian-review')
  await page.reload()
  await expect(page.getByTestId('dock')).toBeVisible({ timeout: 60_000 })
  await expect(page.locator('nav.tabs button.tab', { hasText: 'Review' })).toHaveCount(0)

  const tab = page.locator('nav.tabs button.tab').first()
  await tab.click({ button: 'right' })
  const menu = page.getByTestId('workspace-menu')
  await expect(menu).toBeVisible()
  await menu.getByTestId('workspace-new-from-template').click()

  const palette = page.getByTestId('command-palette')
  await expect(palette).toBeVisible()
  await expect(palette.locator('.crumb')).toContainText('New from template')
  // Submenu lists templates only — not the generic Ctrl+K root.
  await expect(palette.getByRole('option', { name: /^Review/ })).toBeVisible()
  await expect(palette.getByRole('option', { name: /^Data model/ })).toBeVisible()
  await expect(palette.getByRole('option', { name: /On selected object/i })).toHaveCount(0)
  await expect(palette.getByRole('option', { name: /^Recent/i })).toHaveCount(0)
  await expect(palette.getByRole('option', { name: /^Review/ })).toContainText(/Create ·/)

  await palette.getByTestId('palette-input').fill('Review')
  const review = palette.getByRole('option', { name: /^Review/ })
  await expect(review).toBeVisible()
  // Query filters the submenu; Enter runs the first (only) match.
  await page.keyboard.press('Enter')
  await expect(palette).toHaveCount(0)
  await expect(page.locator('nav.tabs button.tab', { hasText: 'Review' })).toBeVisible({ timeout: 30_000 })
})

test('Ctrl+K New from template submenu matches the tab menu', async ({ page }) => {
  test.setTimeout(90_000)
  await login(page)
  await page.keyboard.press('Control+K')
  const palette = page.getByTestId('command-palette')
  await expect(palette).toBeVisible()
  await palette.getByTestId('palette-input').fill('New from template')
  await expect(palette.getByRole('option', { name: /^New from template/ })).toBeVisible()
  await page.keyboard.press('Enter')
  await expect(palette.locator('.crumb')).toContainText('New from template')
  await expect(palette.getByRole('option', { name: /^Products/ })).toBeVisible()
  await expect(palette.getByRole('option', { name: /On selected object/i })).toHaveCount(0)
  await page.keyboard.press('Escape')
  // From Ctrl+K path Esc pops to root; another Esc closes.
  await page.keyboard.press('Escape')
  await expect(palette).toHaveCount(0)
})

test('opens an existing Review tab without creating another', async ({ page }) => {
  test.setTimeout(120_000)
  await login(page)
  await ensureReviewExists(page)

  const before = await listWorkspaces(page)
  const beforeCount = before.workspaces.length
  expect(before.workspaces.filter((item) => item.share_token === 'braian-review')).toHaveLength(1)

  // Focus another tab so Enter must re-focus Review.
  const products = page.locator('nav.tabs button.tab', { hasText: 'Products' }).first()
  await products.click()
  await expect(products).toHaveClass(/active/)

  await page.keyboard.press('Control+K')
  const palette = page.getByTestId('command-palette')
  await expect(palette).toBeVisible()
  await palette.getByTestId('palette-input').fill('New from template')
  await expect(palette.getByRole('option', { name: /^New from template/ })).toBeVisible()
  await page.keyboard.press('Enter')
  await expect(palette.locator('.crumb')).toContainText('New from template')
  await expect(palette.getByRole('option', { name: /^Review/ })).toContainText(/Open existing/)
  await palette.getByTestId('palette-input').fill('Review')
  await expect(palette.getByRole('option', { name: /^Review/ })).toBeVisible()
  await page.keyboard.press('Enter')
  await expect(palette).toHaveCount(0)

  const reviewTab = page.locator('nav.tabs button.tab', { hasText: 'Review' })
  await expect(reviewTab).toBeVisible()
  await expect(reviewTab).toHaveClass(/active/)

  const after = await listWorkspaces(page)
  expect(after.workspaces.length).toBe(beforeCount)
  expect(after.workspaces.filter((item) => item.share_token === 'braian-review')).toHaveLength(1)
})
