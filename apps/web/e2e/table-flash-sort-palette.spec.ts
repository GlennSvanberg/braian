import { expect, test, type Page, type Route } from '@playwright/test'
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

async function openFieldsPalette(page: Page) {
  await page.getByTestId('object-table').focus()
  await page.keyboard.press('Control+k')
  await expect(page.getByTestId('command-palette')).toBeVisible()
  await page.getByTestId('palette-input').fill('Select fields')
  await page.keyboard.press('Enter')
  await expect(page.getByTestId('fields-mode')).toBeVisible()
}

function headerIds(page: Page) {
  return page.locator('.head [data-testid^="col-"]')
}

test.describe('table flash, sort cycle, fields palette filter', () => {
  test('does not flash default columns while column prefs are pending', async ({ page }) => {
    // Saved prefs: Number + Name + Brand only (Type/Status/Version hidden).
    // Without the fix, builtins flash Type/Status/Version before the GET returns.
    let release!: () => void
    const gate = new Promise<void>((resolve) => {
      release = resolve
    })
    const saved = {
      document: {
        version: 2,
        columns: [
          { path: 'number', visible: true },
          { path: 'name', visible: true },
          { path: 'brand', visible: true },
          { path: 'type', visible: false },
          { path: 'status', visible: false },
          { path: 'version', visible: false },
        ],
      },
    }
    // Match only the API — never /assets/column-prefs-*.js (that hung login).
    await page.route('**/v1/views/**/column-prefs**', async (route: Route) => {
      if (route.request().method() !== 'GET') {
        await route.continue()
        return
      }
      await gate
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify(saved),
      })
    })

    await login(page)
    await page.locator('nav.tabs').getByRole('button', { name: /^Products/ }).click()

    // object-table mounts only after prefs; while held we show the skeleton shell.
    await expect(page.getByTestId('column-prefs-skeleton')).toBeVisible()
    await expect(page.getByTestId('object-table')).toHaveCount(0)
    await expect(page.locator('.head [data-testid="col-type"]')).toHaveCount(0)
    await expect(page.locator('.head [data-testid="col-status"]')).toHaveCount(0)
    await expect(page.locator('.head [data-testid="col-version"]')).toHaveCount(0)
    await expect(page.locator('.head [data-testid="col-key"]')).toHaveCount(0)

    release()

    await expect(page.getByTestId('column-prefs-skeleton')).toHaveCount(0)
    await expect(page.getByTestId('object-table')).toBeVisible()
    await expect(page.locator('.head [data-testid="col-number"]')).toBeVisible()
    await expect(page.locator('.head [data-testid="col-name"]')).toBeVisible()
    await expect(page.locator('.head [data-testid="col-brand"]')).toBeVisible()
    await expect(page.locator('.head [data-testid="col-type"]')).toHaveCount(0)
    await expect(page.locator('.head [data-testid="col-status"]')).toHaveCount(0)
  })

  test('Name cycles asc → desc → none; Brand and Number sort or show not-sortable', async ({ page }) => {
    await login(page)
    await openProducts(page)
    await expect(headerIds(page).first()).toBeVisible()

    const name = page.locator('.head [data-testid="col-name"]')
    await expect(name).toBeVisible()
    const nameBtn = name.locator('button.column-header-btn')

    // First click → ascending
    await nameBtn.click()
    await expect(name).toHaveAttribute('aria-sort', 'ascending')
    // Second → descending
    await nameBtn.click()
    await expect(name).toHaveAttribute('aria-sort', 'descending')
    // Third → none (sorting removed)
    await nameBtn.click()
    await expect(name).toHaveAttribute('aria-sort', 'none')

    for (const id of ['col-brand', 'col-number'] as const) {
      const col = page.locator(`.head [data-testid="${id}"]`)
      if ((await col.count()) === 0) {
        // Ensure Brand/Number are visible for the assertion.
        await openFieldsPalette(page)
        const key = id.replace('col-', '')
        await page.getByTestId('fields-mode-input').fill(key === 'number' ? 'Number' : 'Brand')
        const row = page.getByTestId(`fields-row-${key}`)
        await expect(row).toBeVisible()
        if ((await page.getByTestId(`fields-check-${key}`).count()) === 0) {
          await row.click()
        }
        await page.keyboard.press('Escape')
        await expect(col).toBeVisible()
      }
      const sortable = await col.getAttribute('data-sortable')
      if (sortable === 'true') {
        const btn = col.locator('button.column-header-btn')
        await btn.click()
        await expect(col).toHaveAttribute('aria-sort', 'ascending')
        await btn.click()
        await expect(col).toHaveAttribute('aria-sort', 'descending')
        await btn.click()
        await expect(col).toHaveAttribute('aria-sort', 'none')
      } else {
        await expect(col).toHaveAttribute('data-sortable', 'false')
        const btn = col.locator('button.column-header-btn')
        await expect(btn).toHaveAttribute('title', /Not sortable/i)
        await btn.click()
        await expect(col).toHaveAttribute('aria-sort', 'none')
      }
    }
  })

  test('Fields › keeps filter text and row highlight across Enter, Enter', async ({ page }) => {
    await login(page)
    await openProducts(page)
    await openFieldsPalette(page)

    const input = page.getByTestId('fields-mode-input')
    await input.fill('Brand')
    const row = page.getByTestId('fields-row-brand')
    await expect(row).toBeVisible()
    // Move highlight onto Brand if needed
    await row.hover()
    await expect(row).toHaveAttribute('data-active', 'true')

    const wasChecked = (await page.getByTestId('fields-check-brand').count()) > 0
    await page.keyboard.press('Enter')
    await expect(input).toHaveValue('Brand')
    await expect(row).toHaveAttribute('data-active', 'true')
    if (wasChecked) {
      await expect(page.getByTestId('fields-check-brand')).toHaveCount(0)
    } else {
      await expect(page.getByTestId('fields-check-brand')).toBeVisible()
    }

    await page.keyboard.press('Enter')
    await expect(input).toHaveValue('Brand')
    await expect(row).toHaveAttribute('data-active', 'true')
    if (wasChecked) {
      await expect(page.getByTestId('fields-check-brand')).toBeVisible()
    } else {
      await expect(page.getByTestId('fields-check-brand')).toHaveCount(0)
    }
  })

  test('prefs GET 500 seeds defaults; Fields › toggle keeps other columns', async ({ page }) => {
    await page.route('**/v1/views/**/column-prefs**', async (route: Route) => {
      if (route.request().method() === 'GET') {
        await route.fulfill({ status: 500, contentType: 'text/plain', body: 'prefs unavailable' })
        return
      }
      await route.continue()
    })

    await login(page)
    await openProducts(page)
    await expect(page.getByTestId('column-prefs-load-error')).toBeVisible()
    await expect(page.getByTestId('column-prefs-load-error')).toContainText(/Couldn't load saved view/i)

    const before = await headerIds(page).count()
    expect(before).toBeGreaterThan(1)
    await expect(page.locator('.head [data-testid="col-name"]')).toBeVisible()

    await openFieldsPalette(page)
    const input = page.getByTestId('fields-mode-input')
    await input.fill('Brand')
    const row = page.getByTestId('fields-row-brand')
    await expect(row).toBeVisible()
    await row.hover()
    await expect(row).toHaveAttribute('data-active', 'true')
    await page.keyboard.press('Enter')
    await page.keyboard.press('Escape')

    // Without the fix, order was [] and the toggle collapsed the table to Brand alone.
    await expect(page.locator('.head [data-testid="col-name"]')).toBeVisible()
    const after = await headerIds(page).count()
    expect(after).toBeGreaterThan(1)
    await expect(page.getByTestId('column-prefs-load-error')).toBeVisible()
  })

})
