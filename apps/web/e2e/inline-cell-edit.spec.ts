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

function cheddarRow(page: Page) {
  return page.locator('[data-testid="object-row"]').filter({ hasText: /Mature cheddar|Aged cheddar e2e/ }).first()
}

test('inline Name cell edit keeps caret and persists after Enter', async ({ page }) => {
  test.setTimeout(120_000)
  await login(page)
  await openProducts(page)

  const row = page.locator('[data-testid="object-row"]').filter({ hasText: 'Mature cheddar' }).first()
  await expect(row).toBeVisible({ timeout: 60_000 })
  // Pin by DOM id — after fill() the name cell no longer contains "Mature cheddar".
  const rowId = await row.getAttribute('id')
  expect(rowId).toBeTruthy()
  const stableRow = page.locator(`#${CSS.escape(rowId!)}`)

  const nameCell = stableRow.locator('[data-column="name"]').first()
  // Activate the Name column so .active-cell is present before we edit.
  await nameCell.click()
  await expect(stableRow.locator('.active-cell')).toHaveAttribute('data-column', 'name')
  const activeBefore = await stableRow.locator('.active-cell').getAttribute('data-column')

  await nameCell.dblclick()
  const editor = page.getByTestId('cell-editor-name')
  await expect(editor).toBeVisible()

  // Replace the value with a unique suffix so we can assert persistence.
  await editor.fill('Aged cheddar e2e')
  await expect(editor).toHaveValue('Aged cheddar e2e')

  // Move caret to end, then ArrowLeft must move caret — not the active column.
  await editor.evaluate((el: HTMLInputElement) => {
    const end = el.value.length
    el.setSelectionRange(end, end)
  })
  const caretBefore = await editor.evaluate((el: HTMLInputElement) => el.selectionStart)
  expect(caretBefore).toBe('Aged cheddar e2e'.length)

  await editor.press('ArrowLeft')
  const caretAfter = await editor.evaluate((el: HTMLInputElement) => el.selectionStart)
  expect(caretAfter, 'ArrowLeft should move caret left by one').toBe((caretBefore ?? 0) - 1)
  await expect(editor).toBeVisible()
  const activeAfter = await stableRow.locator('.active-cell').getAttribute('data-column')
  expect(activeAfter, 'active column must stay on name while editing').toBe(activeBefore ?? 'name')

  // Enter commits; value must stick after settle (and not snap back to Mature cheddar).
  await editor.press('Enter')
  await expect(page.getByTestId('cell-editor-name')).toHaveCount(0)
  await expect
    .poll(async () => (await cheddarRow(page).locator('[data-column="name"]').textContent())?.trim() ?? '', {
      timeout: 30_000,
    })
    .toBe('Aged cheddar e2e')

  // Reload — persisted name must remain (language_dependent map shape).
  await page.reload()
  await expect(page.getByTestId('dock')).toBeVisible()
  await openProducts(page)
  await expect(cheddarRow(page).locator('[data-column="name"]')).toContainText('Aged cheddar e2e', {
    timeout: 60_000,
  })

  // Esc cancels without saving.
  const row2 = cheddarRow(page)
  await row2.locator('[data-column="name"]').first().dblclick()
  const editor2 = page.getByTestId('cell-editor-name')
  await expect(editor2).toBeVisible()
  await editor2.fill('Should not stick')
  await editor2.press('Escape')
  await expect(page.getByTestId('cell-editor-name')).toHaveCount(0)
  await expect(cheddarRow(page).locator('[data-column="name"]')).toContainText('Aged cheddar e2e')
})
