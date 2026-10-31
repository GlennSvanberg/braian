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
  const palette = page.getByTestId('command-palette')
  await expect(palette).toBeVisible()
  await page.getByTestId('palette-input').fill('Select fields')
  await page.keyboard.press('Enter')
  await expect(page.getByTestId('fields-mode')).toBeVisible()
}

async function enterOn(page: Page, rowTestId: string) {
  await expect(page.getByTestId(rowTestId)).toBeVisible()
  for (let i = 0; i < 40; i += 1) {
    const active = page.locator('[data-testid="fields-mode"] [data-active="true"]')
    const activeId = await active.getAttribute('data-testid')
    if (activeId === rowTestId) {
      await page.keyboard.press('Enter')
      return
    }
    const activeBox = await active.boundingBox()
    const targetBox = await page.getByTestId(rowTestId).boundingBox()
    if (activeBox && targetBox && targetBox.y + 1 < activeBox.y) {
      await page.keyboard.press('ArrowUp')
    } else {
      await page.keyboard.press('ArrowDown')
    }
  }
  throw new Error(`Could not move active highlight to ${rowTestId}`)
}

async function visibleHeaderIds(page: Page) {
  return page.locator('.head [data-testid^="col-"]').evaluateAll((nodes) =>
    nodes.map((node) => node.getAttribute('data-testid') || ''),
  )
}

test.describe('move column, pathValues flash, live row sweep', () => {
  test('Move column left uses last clicked header and shows it in the command', async ({ page }) => {
    await login(page)
    await openProducts(page)
    await expect(page.getByText('Mature cheddar')).toBeVisible({ timeout: 60_000 })

    const before = await visibleHeaderIds(page)
    expect(before.length).toBeGreaterThan(2)
    // Activate via header only — not a body cell.
    const target = before[1]!
    const header = page.getByTestId(target)
    await header.click()
    await expect(header).toHaveAttribute('data-active-column', 'true')
    const headerLabel = ((await header.locator('button.column-header-btn').textContent()) || '').replace(/\s*[↑↓]\s*$/, '').trim()
    expect(headerLabel.length).toBeGreaterThan(0)

    const putLeft = page.waitForResponse(
      (res) =>
        res.url().includes('/column-prefs') &&
        res.request().method() === 'PUT' &&
        res.ok(),
      { timeout: 30_000 },
    )
    await page.keyboard.press('Control+k')
    await expect(page.getByTestId('command-palette')).toBeVisible()
    await page.getByTestId('palette-input').fill('Move column left')
    const move = page.getByTestId('command-palette').getByRole('option').filter({ hasText: 'Move column left' }).first()
    await expect(move).toBeVisible()
    await expect(move).toContainText(headerLabel)
    await page.keyboard.press('Enter')
    await putLeft
    const afterLeft = await visibleHeaderIds(page)
    expect(afterLeft.indexOf(target)).toBe(before.indexOf(target) - 1)
  })

  test('Move column right still works after clicking a body cell', async ({ page }) => {
    await login(page)
    await openProducts(page)
    await expect(page.getByText('Mature cheddar')).toBeVisible({ timeout: 60_000 })

    const before = await visibleHeaderIds(page)
    const target = before[1]!
    const colId = target.replace(/^col-/, '')
    const row = page.getByTestId('object-row').filter({ hasText: 'Mature cheddar' }).first()
    await row.locator(`[data-column="${colId}"]`).click()
    await expect(page.getByTestId(target)).toHaveAttribute('data-active-column', 'true')

    const putRight = page.waitForResponse(
      (res) =>
        res.url().includes('/column-prefs') &&
        res.request().method() === 'PUT' &&
        res.ok(),
      { timeout: 30_000 },
    )
    await page.keyboard.press('Control+k')
    await expect(page.getByTestId('command-palette')).toBeVisible()
    await page.getByTestId('palette-input').fill('Move column right')
    await page.keyboard.press('Enter')
    await putRight
    const afterRight = await visibleHeaderIds(page)
    expect(afterRight.indexOf(target)).toBe(before.indexOf(target) + 1)
  })

  test('pathValues cells show a loading placeholder instead of emdash while the batch is pending', async ({
    page,
  }) => {
    await login(page)
    await openProducts(page)
    await expect(page.getByText('Mature cheddar')).toBeVisible({ timeout: 60_000 })

    // Articles › GTIN (first) — hop + cardinality always uses pathValues.
    await openFieldsPalette(page)
    await page.getByTestId('fields-mode-input').fill('articles')
    await enterOn(page, 'fields-row-articles')
    await enterOn(page, 'fields-row-first')
    await page.getByTestId('fields-mode-input').fill('gtin')
    await enterOn(page, 'fields-row-gtin')
    await page.keyboard.press('Escape')
    await expect(page.locator('.head [data-testid*="gtin"]').first()).toBeVisible()

    let release: (() => void) | undefined
    const gate = new Promise<void>((resolve) => {
      release = resolve
    })
    await page.route('**/v1/objects:pathValues**', async (route: Route) => {
      await gate
      await route.continue()
    })

    const pathValuesRequest = page.waitForRequest(
      (req) => req.url().includes('pathValues') && req.method() === 'POST',
      { timeout: 60_000 },
    )
    await page.reload()
    await expect(page.getByTestId('object-table')).toBeVisible({ timeout: 60_000 })
    await pathValuesRequest

    const gtinCell = page.getByTestId(/hop-cell-.*gtin/)
    await expect(gtinCell.first()).toHaveAttribute('data-loading', 'true', { timeout: 30_000 })
    await expect(gtinCell.first()).not.toHaveText('—')

    release?.()
    await expect(gtinCell.first()).not.toHaveAttribute('data-loading', 'true', { timeout: 30_000 })
  })

  test('live Description and Version from another tab update in one sweep', async ({ browser }) => {
    const contextA = await browser.newContext()
    const contextB = await browser.newContext()
    const pageA = await contextA.newPage()
    const pageB = await contextB.newPage()
    await login(pageA)
    await login(pageB)
    await openProducts(pageA)
    await openProducts(pageB)
    await expect(pageA.getByText('Mature cheddar')).toBeVisible({ timeout: 60_000 })
    await expect(pageB.getByText('Mature cheddar')).toBeVisible({ timeout: 60_000 })

    // Description column on both tabs
    for (const page of [pageA, pageB]) {
      await openFieldsPalette(page)
      await page.getByTestId('fields-mode-input').fill('description')
      const row = page.getByTestId('fields-row-description')
      await expect(row).toBeVisible()
      await row.hover()
      await page.keyboard.press('Enter')
      const lang = page.getByTestId('fields-row-en')
      if (await lang.isVisible().catch(() => false)) {
        await lang.hover()
        await page.keyboard.press('Enter')
      }
      await page.keyboard.press('Escape')
    }

    const rowA = pageA.getByTestId('object-row').filter({ hasText: 'Mature cheddar' }).first()
    const rowB = pageB.getByTestId('object-row').filter({ hasText: 'Mature cheddar' }).first()
    await expect(rowA.locator('[data-testid^="path-cell-description"]').first()).toBeVisible({
      timeout: 30_000,
    })
    const versionBefore = (await rowA.locator('[data-column="version"]').textContent())?.trim() || ''
    const descBefore =
      (await rowA.locator('[data-testid^="path-cell-description"]').first().textContent())?.trim() || ''

    // Edit Description on tab B
    const descCellB = rowB.locator('[data-column^="description"]').first()
    await descCellB.dblclick()
    const editor = pageB.locator('[data-testid^="cell-editor-description"]').first()
    await expect(editor).toBeVisible()
    const nextText = `Live sweep ${Date.now()}`
    await editor.fill(nextText)
    await editor.press('Enter')
    await expect(pageB.locator('[data-testid^="cell-editor-description"]')).toHaveCount(0)

    // On tab A both cells must land together: once Description matches, Version must already differ.
    await expect(async () => {
      const desc =
        (await rowA.locator('[data-testid^="path-cell-description"]').first().textContent())?.trim() || ''
      const version = (await rowA.locator('[data-column="version"]').textContent())?.trim() || ''
      expect(desc).toContain('Live sweep')
      expect(desc).not.toBe(descBefore)
      expect(version).not.toBe(versionBefore)
      expect(version.length).toBeGreaterThan(0)
    }).toPass({ timeout: 60_000 })

    await contextA.close()
    await contextB.close()
  })
})
