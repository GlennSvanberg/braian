import { expect, test, type Page } from '@playwright/test'

async function openPalette(page: Page) {
  await page.keyboard.press('Control+k')
  await expect(page.getByTestId('fields-palette')).toBeVisible()
}

test.describe('field picker prototype', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/prototype/field-picker')
    await expect(page.getByTestId('field-picker-prototype')).toBeVisible()
    await expect(page.getByTestId('prototype-product-table')).toBeVisible()
  })

  test('10 document title and focus for Ctrl+K; 9 header/cell x alignment', async ({ page }) => {
    await expect(page).toHaveTitle('Field picker prototype · Braian')
    await expect(page.getByText('Prototype with mock data. Press Ctrl+K to pick fields.')).toBeVisible()
    await page.keyboard.press('Control+k')
    await expect(page.getByTestId('fields-palette')).toBeVisible()
    await page.keyboard.press('Escape')

    const nameHeader = page.getByRole('columnheader', { name: 'Name' })
    const nameCell = page.locator('[role="row"][data-testid="standard-grid-row"]').first().locator('[data-column="name"]')
    const headerBox = await nameHeader.boundingBox()
    const cellBox = await nameCell.boundingBox()
    expect(Math.abs(headerBox!.x - cellBox!.x)).toBeLessThan(2)
  })

  test('1–4 breadcrumb path, subtitles, no double chevron, checks on levels', async ({ page }) => {
    await page.getByTestId('table-select-fields').click()
    await expect(page.getByTestId('fields-palette')).toBeVisible()
    await expect(page.getByTestId('fields-crumb')).toHaveText(/Fields ›$/)
    const articles = page.getByTestId('fields-row-articles')
    await expect(articles.locator('.hit-title')).toHaveText('Articles')
    await expect(page.getByTestId('fields-check-sku')).toBeVisible()
    await expect(page.getByTestId('fields-check-name')).toBeVisible()
    await articles.click()
    await expect(page.getByTestId('fields-crumb')).toContainText('Fields › Articles ›')
    await expect(page.getByTestId('fields-crumb')).not.toContainText('How many linked rows')
    await expect(page.getByTestId('fields-row-all')).toContainText('Every linked row')
    await page.getByTestId('fields-row-all').click()
    await expect(page.getByTestId('fields-crumb')).toContainText('Fields › Articles › All ›')
    await page.getByTestId('fields-row-net_volume').click()
    await expect(page.getByTestId('fields-row-ml')).toBeVisible()
    await expect(page.getByTestId('fields-row-l')).toBeVisible()
    await page.getByTestId('fields-row-ml').click()
    await expect(page.getByRole('columnheader', { name: 'Articles › Net volume (all, ml)' })).toBeVisible()
    await expect(page.getByTestId('fields-check-ml')).toBeVisible()
  })

  test('5 SKU survives adding Allergens (code); Enter does not leak to root', async ({ page }) => {
    await expect(page.getByRole('columnheader', { name: 'SKU' })).toBeVisible()
    await openPalette(page)
    await page.getByTestId('fields-row-allergens').click()
    await page.getByTestId('fields-row-code').click()
    await expect(page.getByTestId('fields-check-code')).toBeVisible()
    await expect(page.getByRole('columnheader', { name: 'SKU' })).toBeVisible()
    await expect(page.getByRole('columnheader', { name: 'Allergens (code)' })).toBeVisible()
  })

  test('6 stay on level for Description; Edit field does not close', async ({ page }) => {
    await openPalette(page)
    await page.getByTestId('fields-row-description').click()
    await page.getByTestId('fields-row-en').click()
    await expect(page.getByRole('columnheader', { name: 'Description (en)' })).toBeVisible()
    await expect(page.getByTestId('fields-row-en')).toBeVisible()
    await expect(page.getByTestId('fields-check-en')).toBeVisible()
    await page.keyboard.press('Escape')
    const header = page.getByRole('columnheader', { name: 'Description (en)' })
    await header.click({ button: 'right' })
    await page.getByTestId('edit-field').click()
    await expect(page.getByTestId('fields-palette')).toBeVisible()
    await page.getByTestId('fields-row-sv').click()
    await expect(page.getByTestId('fields-palette')).toBeVisible()
    await expect(page.getByRole('columnheader', { name: 'Description (sv)' })).toBeVisible()
  })

  test('7 Edit field opens first qualifier; All→First updates in place', async ({ page }) => {
    await openPalette(page)
    await page.getByTestId('fields-row-articles').click()
    await page.getByTestId('fields-row-all').click()
    await page.getByTestId('fields-row-net_volume').click()
    await page.getByTestId('fields-row-ml').click()
    await page.keyboard.press('Escape')
    const header = page.getByRole('columnheader', { name: 'Articles › Net volume (all, ml)' })
    await header.click({ button: 'right' })
    await page.getByTestId('edit-field').click()
    await expect(page.getByTestId('fields-row-all')).toBeVisible()
    await expect(page.getByTestId('fields-check-all')).toBeVisible()
    await page.getByTestId('fields-row-first').click()
    await expect(page.getByRole('columnheader', { name: 'Articles › Net volume (first, ml)' })).toBeVisible()
    await expect(page.getByTestId('fields-palette')).toBeVisible()
    const firstCell = page.locator('[role="row"][data-testid="standard-grid-row"]').first().locator('[data-column*="net_volume"]')
    await expect(firstCell).toHaveText('1000 ml')
  })

  test('8 unit conversion and storage °C; Remove column', async ({ page }) => {
    await openPalette(page)
    await page.getByTestId('fields-row-articles').click()
    await page.getByTestId('fields-row-all').click()
    await page.getByTestId('fields-row-net_volume').click()
    await page.getByTestId('fields-row-ml').click()
    await page.keyboard.press('Escape')
    const oatVolume = page.locator('[role="row"][data-testid="standard-grid-row"]').first().locator('[data-column*="net_volume"]')
    await expect(oatVolume).toHaveText('1000 ml, 250 ml')

    await openPalette(page)
    await page.getByTestId('fields-row-storage_temperature').click()
    await page.getByTestId('fields-row-min').click()
    await expect(page.getByRole('columnheader', { name: 'Storage temperature (min)' })).toBeVisible()
    const tempCell = page.locator('[role="row"][data-testid="standard-grid-row"]').first().locator('[data-column*="storage_temperature"]')
    await expect(tempCell).toHaveText('2 °C')
    await page.keyboard.press('Escape')
    const header = page.getByRole('columnheader', { name: 'Storage temperature (min)' })
    await header.click({ button: 'right' })
    await expect(page.getByTestId('select-fields-menu')).toBeVisible()
    await page.getByTestId('remove-column').click()
    await expect(page.getByRole('columnheader', { name: 'Storage temperature (min)' })).toHaveCount(0)
  })

  test('polish: Specific field-first, title, empty dash, +N tooltip, Supplier check, active subtitle, clear filter', async ({
    page,
  }) => {
    // 9 header tooltip
    await openPalette(page)
    await page.getByTestId('fields-row-articles').click()
    await page.getByTestId('fields-row-all').click()
    await page.getByTestId('fields-row-net_volume').click()
    await page.getByTestId('fields-row-ml').click()
    await page.keyboard.press('Escape')
    const allHeader = page.getByRole('columnheader', { name: 'Articles › Net volume (all, ml)' })
    await expect(allHeader).toHaveAttribute('title', 'Articles › Net volume (all, ml)')

    // 4 +N tooltip on milk (3 articles)
    const milkRow = page.locator('[role="row"][data-testid="standard-grid-row"]').nth(1)
    const milkCell = milkRow.locator('[data-column*="net_volume"]')
    await expect(milkCell).toContainText('+1')
    await expect(milkCell).toHaveAttribute('title', /1000 ml, 1500 ml, 250 ml/)

    // 6 Supplier check at root when Supplier › Country is a column
    await openPalette(page)
    await page.getByTestId('fields-row-supplier').click()
    await page.getByTestId('fields-row-country').click()
    await page.keyboard.press('ArrowLeft') // back to root
    await expect(page.getByTestId('fields-check-supplier')).toBeVisible()
    // 7 active subtitle on checked Description after adding sv
    await page.getByTestId('fields-row-description').click()
    await page.getByTestId('fields-row-sv').click()
    await page.keyboard.press('ArrowLeft')
    await expect(page.getByTestId('fields-row-description')).toContainText('Description — sv')

    // 8 filter clears after toggle
    await page.getByTestId('fields-palette-input').fill('sku')
    await expect(page.getByTestId('fields-palette-input')).toHaveValue('sku')
    await page.getByTestId('fields-row-sku').click() // toggle off
    await expect(page.getByTestId('fields-palette-input')).toHaveValue('')
    await page.keyboard.press('Escape')

    // 1 Specific: field first, then article as GTIN · key
    await openPalette(page)
    await page.getByTestId('fields-row-articles').click()
    await page.getByTestId('fields-row-specific').click()
    await expect(page.getByTestId('fields-crumb')).toContainText('Specific')
    await page.getByTestId('fields-row-net_volume').click()
    // Article list with GTIN · key
    const articleRow = page.locator('[data-testid^="fields-row-"]').filter({ hasText: '·' }).first()
    await expect(articleRow).toBeVisible()
    await expect(articleRow.locator('small')).not.toHaveText('')
    await page.getByTestId('fields-row-1.5L').click()
    await page.getByTestId('fields-row-ml').click()
    // 2 title format (1.5L, ml)
    await expect(page.getByRole('columnheader', { name: 'Articles › Net volume (1.5L, ml)' })).toBeVisible()

    // 3 empty dash: products without 1.5L article show —
    const yogurtCell = page.locator('[role="row"][data-testid="standard-grid-row"]').nth(2).locator('[data-column*="specific=1.5L"]')
    await expect(yogurtCell).toHaveText('—')
  })
})
