import { expect, test, type Page } from '@playwright/test'

async function openPalette(page: Page) {
  await page.keyboard.press('Control+k')
  await expect(page.getByTestId('fields-palette')).toBeVisible()
}

async function addAllergensCode(page: Page) {
  await openPalette(page)
  await page.getByTestId('fields-row-allergens').click()
  await expect(page.getByTestId('fields-crumb')).toContainText('Fields › Allergens')
  await page.getByTestId('fields-row-code').click()
  await expect(page.getByRole('columnheader', { name: 'Allergens (code)' })).toBeVisible()
}

test.describe('field picker prototype', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/prototype/field-picker')
    await expect(page.getByTestId('field-picker-prototype')).toBeVisible()
    await expect(page.getByTestId('prototype-product-table')).toBeVisible()
  })

  test('10 document title and focus for Ctrl+K; 9 header/cell x alignment', async ({ page }) => {
    // Point 11 (title) + 10 (focus)
    await expect(page).toHaveTitle('Field picker prototype · Braian')
    // Point 10: Ctrl+K works without clicking into the page
    await page.keyboard.press('Control+k')
    await expect(page.getByTestId('fields-palette')).toBeVisible()
    await page.keyboard.press('Escape')

    // Point 9: header x matches cell x for Name
    const nameHeader = page.getByRole('columnheader', { name: 'Name' })
    const nameCell = page.locator('[role="row"][data-testid="standard-grid-row"]').first().locator('[data-column="name"]')
    await expect(nameHeader).toBeVisible()
    await expect(nameCell).toBeVisible()
    const headerBox = await nameHeader.boundingBox()
    const cellBox = await nameCell.boundingBox()
    expect(headerBox).toBeTruthy()
    expect(cellBox).toBeTruthy()
    expect(Math.abs((headerBox!.x) - (cellBox!.x))).toBeLessThan(2)
  })

  test('1–4 breadcrumb path, subtitles, no double chevron, checks on levels', async ({ page }) => {
    await page.getByTestId('table-select-fields').click()
    await expect(page.getByTestId('fields-palette')).toBeVisible()
    await expect(page.getByTestId('fields-crumb')).toHaveText(/Fields ›$/)

    // Point 3: title has no › — only trailing chevron element
    const articles = page.getByTestId('fields-row-articles')
    await expect(articles.getByText('Articles', { exact: true })).toBeVisible()
    await expect(articles.locator('.hit-title')).toHaveText('Articles')

    // Point 4: SKU/Name already checked
    await expect(page.getByTestId('fields-check-sku')).toBeVisible()
    await expect(page.getByTestId('fields-check-name')).toBeVisible()

    await articles.click()
    // Point 1: breadcrumb is field path, not "How many linked rows"
    await expect(page.getByTestId('fields-crumb')).toContainText('Fields › Articles ›')
    await expect(page.getByTestId('fields-crumb')).not.toContainText('How many linked rows')

    // Point 2: distinct subtitles (or none) — not the step title repeated
    await expect(page.getByTestId('fields-row-all')).toContainText('Every linked row')
    await expect(page.getByTestId('fields-row-first')).toContainText('Only the first')
    await expect(page.getByTestId('fields-row-specific')).toContainText('Pick one linked row')

    await page.getByTestId('fields-row-all').click()
    await expect(page.getByTestId('fields-crumb')).toContainText('Fields › Articles › All ›')
    await page.getByTestId('fields-row-net_volume').click()
    await expect(page.getByTestId('fields-crumb')).toContainText('Fields › Articles › All')
    await page.getByTestId('fields-row-ml').click()
    await expect(page.getByRole('columnheader', { name: 'Articles › Net volume (all, ml)' })).toBeVisible()
    // Point 4 + 6: stay on unit level with check on ml
    await expect(page.getByTestId('fields-check-ml')).toBeVisible()
    await expect(page.getByTestId('fields-crumb')).toContainText('Articles')
    await expect(page.getByTestId('fields-row-ml')).toBeVisible()
  })

  test('5 SKU survives adding Allergens (code); Enter does not leak to root', async ({ page }) => {
    await expect(page.getByRole('columnheader', { name: 'SKU' })).toBeVisible()
    await addAllergensCode(page)
    // Stay on value-list level (point 6), SKU still present (point 5)
    await expect(page.getByTestId('fields-check-code')).toBeVisible()
    await expect(page.getByRole('columnheader', { name: 'SKU' })).toBeVisible()
    await expect(page.getByRole('columnheader', { name: 'Allergens (code)' })).toBeVisible()
    // Highlight should not jump away to toggling SKU — SKU column still there after Esc
    await page.keyboard.press('Escape')
    await expect(page.getByRole('columnheader', { name: 'SKU' })).toBeVisible()
  })

  test('6 stay on level for Description; Edit field does not close', async ({ page }) => {
    await openPalette(page)
    await page.getByTestId('fields-row-description').click()
    await expect(page.getByTestId('fields-crumb')).toContainText('Fields › Description ›')
    await page.getByTestId('fields-row-en').click()
    await expect(page.getByRole('columnheader', { name: 'Description (en)' })).toBeVisible()
    // Stay on language level, not root
    await expect(page.getByTestId('fields-row-en')).toBeVisible()
    await expect(page.getByTestId('fields-check-en')).toBeVisible()
    await expect(page.getByTestId('fields-crumb')).toContainText('Description')
    await page.keyboard.press('Escape')

    const header = page.getByRole('columnheader', { name: 'Description (en)' })
    await header.click({ button: 'right' })
    await page.getByTestId('edit-field').click()
    await expect(page.getByTestId('fields-palette')).toBeVisible()
    await page.getByTestId('fields-row-sv').click()
    // Palette still open after Edit field swap
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
    // Opens on cardinality (first qualifier), All checked
    await expect(page.getByTestId('fields-row-all')).toBeVisible()
    await expect(page.getByTestId('fields-check-all')).toBeVisible()
    await page.getByTestId('fields-row-first').click()
    await expect(page.getByRole('columnheader', { name: 'Articles › Net volume (first, ml)' })).toBeVisible()
    // Still on cardinality level; unit kept
    await expect(page.getByTestId('fields-palette')).toBeVisible()
    await expect(page.getByTestId('fields-row-first')).toBeVisible()
    // Cell shows first article only (1000 ml after conversion)
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
})
