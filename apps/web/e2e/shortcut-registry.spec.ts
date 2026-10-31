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

async function openProducts(page: Page) {
  await page.locator('nav.tabs').getByRole('button', { name: /^Products/ }).click()
  await expect(page.getByTestId('object-table')).toBeVisible({ timeout: 60_000 })
  await expect(page.getByText('Mature cheddar')).toBeVisible({ timeout: 60_000 })
}

async function visibleHeaderIds(page: Page) {
  return page.locator('.head [data-testid^="col-"]').evaluateAll((nodes) =>
    nodes.map((node) => node.getAttribute('data-testid')).filter((id): id is string => Boolean(id)),
  )
}

test.describe('shortcut registry', () => {
  test('Ctrl+K palette shows chords from the shortcut registry', async ({ page }) => {
    test.setTimeout(120_000)
    await login(page)
    await openProducts(page)

    await page.keyboard.press('Control+k')
    const palette = page.getByTestId('palette-input')
    await expect(palette).toBeVisible()
    await palette.fill('Focus pane left')
    const row = page.locator('[cmdk-item], .hit').filter({ hasText: 'Focus pane left' }).first()
    await expect(row).toBeVisible()
    await expect(row.locator('kbd')).toHaveText('Alt+←')
    await page.keyboard.press('Escape')
  })

  test('Alt+Shift+Arrow does not steal pane focus (Alt alone still bound)', async ({ page }) => {
    test.setTimeout(120_000)
    await login(page)
    await openProducts(page)

    await page.keyboard.press('Control+k')
    await page.getByTestId('palette-input').fill('Articles view')
    const articles = page.locator('[cmdk-item], .hit').filter({ hasText: /Articles/ }).first()
    await expect(articles).toBeVisible()
    await articles.click()
    await expect(page.locator('.pane-tab', { hasText: 'Articles' })).toBeVisible({ timeout: 30_000 })

    await page.keyboard.press('Alt+Shift+ArrowLeft')
    await page.waitForTimeout(200)
    await expect(page.getByTestId('dock')).toBeVisible()
    await expect(page.getByTestId('palette-input')).toHaveCount(0)

    await page.keyboard.press('Alt+ArrowRight')
    await expect(page.getByTestId('dock')).toBeVisible()
  })

  test('Ctrl+Shift+ArrowLeft moves the active column; blocked while editing (#132)', async ({ page }) => {
    test.setTimeout(120_000)
    await login(page)
    await openProducts(page)

    const before = await visibleHeaderIds(page)
    expect(before.length).toBeGreaterThan(2)
    const target = before[1]!
    const header = page.getByTestId(target)
    await header.click()
    await expect(header).toHaveAttribute('data-active-column', 'true')

    const putLeft = page.waitForResponse(
      (res) =>
        res.url().includes('/column-prefs') &&
        res.request().method() === 'PUT' &&
        res.ok(),
      { timeout: 30_000 },
    )
    await page.keyboard.press('Control+Shift+ArrowLeft')
    await putLeft
    const afterLeft = await visibleHeaderIds(page)
    expect(afterLeft.indexOf(target)).toBe(before.indexOf(target) - 1)

    // #132 guard: while a cell editor is open, Ctrl+Shift must not move columns.
    const nameHeader = page.locator('.head [data-testid^="col-"]').filter({ hasText: /^Name/ }).first()
    const nameId = await nameHeader.getAttribute('data-testid')
    expect(nameId).toBeTruthy()
    const colId = nameId!.replace(/^col-/, '')
    const row = page.getByTestId('object-row').filter({ hasText: 'Mature cheddar' }).first()
    await row.locator(`[data-column="${colId}"]`).dblclick()
    await expect(page.locator('.cell-editor, [data-testid^="cell-editor"]').first()).toBeVisible({ timeout: 10_000 })

    const mid = await visibleHeaderIds(page)
    await page.keyboard.press('Control+Shift+ArrowRight')
    await page.waitForTimeout(400)
    const still = await visibleHeaderIds(page)
    expect(still).toEqual(mid)
    await page.keyboard.press('Escape')
  })

  test('F1 help lists registry chords including Ctrl+Shift column move', async ({ page }) => {
    test.setTimeout(120_000)
    await login(page)
    await openProducts(page)
    await page.keyboard.press('F1')
    const help = page.getByTestId('help-shortcuts')
    await expect(help).toBeVisible()
    await expect(help.locator('[data-shortcut-id="table.column.move-left"]')).toContainText('Ctrl+Shift+←')
    await expect(help.locator('[data-shortcut-id="help.open"]')).toContainText('F1')
    await page.keyboard.press('Escape')
    await expect(help).toHaveCount(0)
  })
})
