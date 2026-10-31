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

test('cell editor keeps Home/End/selection keys; active cell stays put', async ({ page }) => {
  test.setTimeout(120_000)
  await login(page)
  await openProducts(page)

  const original = 'Mature cheddar'
  const row = rowByName(page, original)
  await expect(row).toBeVisible({ timeout: 60_000 })
  const rowId = await row.getAttribute('id')
  expect(rowId).toMatch(/^object-row-/)
  const stableRow = page.locator(`[id="${rowId}"]`)

  const nameCell = stableRow.locator('[data-column="name"]').first()
  await nameCell.click()
  await expect(stableRow.locator('.active-cell')).toHaveAttribute('data-column', 'name')
  const activeBefore = await stableRow.locator('.active-cell').getAttribute('data-column')
  const activeRowBefore = rowId

  await nameCell.dblclick()
  const editor = page.getByTestId('cell-editor-name')
  await expect(editor).toBeVisible()

  const sample = 'Braian key guard sample'
  await editor.fill(sample)
  await expect(editor).toHaveValue(sample)

  // Caret at end → Home moves to start (not first column/row).
  await editor.evaluate((el: HTMLInputElement) => el.setSelectionRange(el.value.length, el.value.length))
  await editor.press('Home')
  expect(await editor.evaluate((el: HTMLInputElement) => el.selectionStart)).toBe(0)
  expect(await editor.evaluate((el: HTMLInputElement) => el.selectionEnd)).toBe(0)

  await editor.press('End')
  expect(await editor.evaluate((el: HTMLInputElement) => el.selectionStart)).toBe(sample.length)

  // Shift+Home selects back to start.
  await editor.press('Shift+Home')
  expect(await editor.evaluate((el: HTMLInputElement) => el.selectionStart)).toBe(0)
  expect(await editor.evaluate((el: HTMLInputElement) => el.selectionEnd)).toBe(sample.length)

  // Ctrl+A selects all.
  await editor.press('Control+a')
  expect(await editor.evaluate((el: HTMLInputElement) => el.selectionStart)).toBe(0)
  expect(await editor.evaluate((el: HTMLInputElement) => el.selectionEnd)).toBe(sample.length)

  // Collapse to mid, then Ctrl+ArrowLeft / Ctrl+ArrowRight move by word.
  await editor.evaluate((el: HTMLInputElement) => {
    const mid = el.value.indexOf(' ') + 1
    el.setSelectionRange(mid, mid)
  })
  const midBefore = await editor.evaluate((el: HTMLInputElement) => el.selectionStart)
  await editor.press('Control+ArrowLeft')
  const afterWordLeft = await editor.evaluate((el: HTMLInputElement) => el.selectionStart)
  expect(afterWordLeft).toBeLessThan(midBefore ?? 0)
  await editor.press('Control+ArrowRight')
  const afterWordRight = await editor.evaluate((el: HTMLInputElement) => el.selectionStart)
  expect(afterWordRight).toBeGreaterThan(afterWordLeft ?? 0)

  // PageUp/PageDown must not move the active row while editing.
  await editor.press('PageDown')
  await editor.press('PageUp')
  await expect(editor).toBeVisible()
  await expect(editor).toHaveValue(sample)

  // Backspace / Delete edit the value, not the grid.
  await editor.evaluate((el: HTMLInputElement) => el.setSelectionRange(el.value.length, el.value.length))
  await editor.press('Backspace')
  const afterBackspace = await editor.inputValue()
  expect(afterBackspace).toBe(sample.slice(0, -1))
  await editor.evaluate((el: HTMLInputElement) => el.setSelectionRange(0, 0))
  await editor.press('Delete')
  const afterDelete = await editor.inputValue()
  expect(afterDelete).toBe(afterBackspace.slice(1))

  // Active cell / column must not have moved; same row still editing.
  const activeAfter = await stableRow.locator('.active-cell').getAttribute('data-column')
  expect(activeAfter, 'active column must stay on name while editing').toBe(activeBefore ?? 'name')
  await expect(page.locator(`[id="${activeRowBefore}"]`).getByTestId('cell-editor-name')).toBeVisible()

  // Esc discard — leave seed Name untouched for later specs.
  await editor.press('Escape')
  await expect(page.getByTestId('cell-editor-name')).toHaveCount(0)
  await expect(stableRow.locator('[data-column="name"]')).toContainText(original)
})
