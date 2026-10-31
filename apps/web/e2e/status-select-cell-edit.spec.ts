import { expect, test, type Page } from '@playwright/test'
import { demoCredentials } from './demo'

async function login(page: Page) {
  const demo = demoCredentials()
  await page.goto('/')
  await page.getByTestId('email').fill(demo.email)
  await page.getByTestId('password').fill(demo.password)
  await page.getByRole('button', { name: 'Sign in' }).click()
  await expect(page.getByTestId('workspace')).toBeVisible()
}

async function openProducts(page: Page) {
  await page.locator('nav.tabs').getByRole('button', { name: /^Products/ }).click()
  await expect(page.getByTestId('object-table')).toBeVisible()
}

function rowByName(page: Page, name: RegExp | string) {
  return page.locator('[data-testid="object-row"]').filter({ hasText: name }).first()
}

test('Status cell uses Select and persists; Type/Version stay read-only', async ({ page }) => {
  test.setTimeout(120_000)
  await login(page)
  await openProducts(page)

  const row = rowByName(page, 'Mature cheddar')
  await expect(row).toBeVisible({ timeout: 60_000 })
  const rowId = await row.getAttribute('id')
  expect(rowId).toMatch(/^object-row-/)
  const stableRow = page.locator(`[id="${rowId}"]`)

  const statusCell = stableRow.locator('[data-column="status"]').first()
  await expect(statusCell).toBeVisible()
  const labelBefore = (await statusCell.innerText()).trim()

  // Type / Version: double-click must not open an editor.
  await stableRow.locator('[data-column="type"]').first().dblclick()
  await expect(page.getByTestId('cell-editor-type')).toHaveCount(0)
  await stableRow.locator('[data-column="version"]').first().dblclick()
  await expect(page.getByTestId('cell-editor-version')).toHaveCount(0)

  // Status: opens a Select (not a free-text input).
  await statusCell.dblclick()
  const editor = page.getByTestId('cell-editor-status')
  await expect(editor).toBeVisible()
  await expect(editor.locator('input[type="text"]')).toHaveCount(0)
  const trigger = page.getByTestId('cell-editor-status-trigger')
  await expect(trigger).toBeVisible()

  // Pick a different option than the current label.
  const targetLabel = labelBefore === 'OK' ? 'Draft' : 'OK'
  // List may already be open via defaultOpen; if not, open it.
  const option = page.getByRole('option', { name: targetLabel })
  if (!(await option.isVisible().catch(() => false))) {
    await trigger.click()
  }
  await expect(option).toBeVisible()
  await option.click()

  await expect(page.getByTestId('cell-editor-status')).toHaveCount(0)
  await expect
    .poll(async () => (await stableRow.locator('[data-column="status"]').innerText()).trim(), {
      timeout: 30_000,
    })
    .toBe(targetLabel)

  await page.reload()
  await expect(page.getByTestId('dock')).toBeVisible()
  await openProducts(page)
  const after = rowByName(page, 'Mature cheddar')
  await expect(after.locator('[data-column="status"]')).toContainText(targetLabel, { timeout: 60_000 })

  // Restore previous status so later specs stay stable.
  try {
    await after.locator('[data-column="status"]').first().dblclick()
    await expect(page.getByTestId('cell-editor-status-trigger')).toBeVisible()
    const restore = page.getByRole('option', { name: labelBefore })
    if (!(await restore.isVisible().catch(() => false))) {
      await page.getByTestId('cell-editor-status-trigger').click()
    }
    await restore.click()
    await expect
      .poll(async () => (await rowByName(page, 'Mature cheddar').locator('[data-column="status"]').innerText()).trim(), {
        timeout: 30_000,
      })
      .toBe(labelBefore)
  } catch {
    // best-effort restore
  }
})
