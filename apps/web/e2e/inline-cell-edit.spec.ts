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

/** Commit a Name cell to `text` (used to restore seed data for later specs). */
async function setNameCell(page: Page, row: ReturnType<Page['locator']>, text: string) {
  await row.locator('[data-column="name"]').first().dblclick()
  const editor = page.getByTestId('cell-editor-name')
  await expect(editor).toBeVisible()
  await editor.fill(text)
  await editor.press('Enter')
  await expect(page.getByTestId('cell-editor-name')).toHaveCount(0)
  await expect
    .poll(async () => (await rowByName(page, text).locator('[data-column="name"]').textContent())?.trim() ?? '', {
      timeout: 30_000,
    })
    .toBe(text)
}

test('inline Name cell edit keeps caret and persists after Enter', async ({ page }) => {
  test.setTimeout(120_000)
  await login(page)
  await openProducts(page)

  const original = 'Mature cheddar'
  const edited = 'Aged cheddar e2e'
  const row = rowByName(page, original)
  await expect(row).toBeVisible({ timeout: 60_000 })
  // Pin by DOM id — after fill() the name cell no longer contains the original text.
  const rowId = await row.getAttribute('id')
  expect(rowId).toMatch(/^object-row-/)
  const stableRow = page.locator(`[id="${rowId}"]`)

  // Always restore seed Name so later specs (move-col, palette, …) still find Mature cheddar
  // even if Playwright runs this file mid-suite.
  try {
    const nameCell = stableRow.locator('[data-column="name"]').first()
    await nameCell.click()
    await expect(stableRow.locator('.active-cell')).toHaveAttribute('data-column', 'name')
    const activeBefore = await stableRow.locator('.active-cell').getAttribute('data-column')

    await nameCell.dblclick()
    const editor = page.getByTestId('cell-editor-name')
    await expect(editor).toBeVisible()

    await editor.fill(edited)
    await expect(editor).toHaveValue(edited)

    await editor.evaluate((el: HTMLInputElement) => {
      const end = el.value.length
      el.setSelectionRange(end, end)
    })
    const caretBefore = await editor.evaluate((el: HTMLInputElement) => el.selectionStart)
    expect(caretBefore).toBe(edited.length)

    await editor.press('ArrowLeft')
    const caretAfter = await editor.evaluate((el: HTMLInputElement) => el.selectionStart)
    expect(caretAfter, 'ArrowLeft should move caret left by one').toBe((caretBefore ?? 0) - 1)
    await expect(editor).toBeVisible()
    const activeAfter = await stableRow.locator('.active-cell').getAttribute('data-column')
    expect(activeAfter, 'active column must stay on name while editing').toBe(activeBefore ?? 'name')

    await editor.press('Enter')
    await expect(page.getByTestId('cell-editor-name')).toHaveCount(0)
    await expect
      .poll(async () => (await rowByName(page, edited).locator('[data-column="name"]').textContent())?.trim() ?? '', {
        timeout: 30_000,
      })
      .toBe(edited)

    await page.reload()
    await expect(page.getByTestId('dock')).toBeVisible()
    await openProducts(page)
    await expect(rowByName(page, edited).locator('[data-column="name"]')).toContainText(edited, {
      timeout: 60_000,
    })

    const row2 = rowByName(page, edited)
    await row2.locator('[data-column="name"]').first().dblclick()
    const editor2 = page.getByTestId('cell-editor-name')
    await expect(editor2).toBeVisible()
    await editor2.fill('Should not stick')
    await editor2.press('Escape')
    await expect(page.getByTestId('cell-editor-name')).toHaveCount(0)
    await expect(rowByName(page, edited).locator('[data-column="name"]')).toContainText(edited)
  } finally {
    await openProducts(page)
    const current = rowByName(page, new RegExp(`${original}|${edited}|Should not stick`))
    if (await current.count()) {
      const name = (await current.locator('[data-column="name"]').textContent())?.trim() ?? ''
      if (name !== original) {
        await setNameCell(page, current, original)
      }
    }
  }
})
