import { expect, test, type Page } from '@playwright/test'

async function openPalette(page: Page) {
  if (await page.getByTestId('fields-palette').isVisible().catch(() => false)) return
  // DOM click avoids Playwright's obscured-element retry mousedown on the backdrop.
  await page.getByTestId('open-fields-command').evaluate((el: HTMLElement) => el.click())
  const visible = await page.getByTestId('fields-palette').isVisible().catch(() => false)
  if (!visible) {
    await page.getByTestId('field-picker-prototype').focus()
    await page.keyboard.press('Control+k')
  }
  await expect(page.getByTestId('fields-palette')).toBeVisible()
}

async function backToRoot(page: Page) {
  for (let i = 0; i < 10; i += 1) {
    const crumb = await page.getByTestId('fields-crumb').textContent()
    if (crumb === 'Fields ›') return
    await page.keyboard.press('ArrowLeft')
  }
}

test.describe('field picker prototype', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/prototype/field-picker')
    await expect(page.getByTestId('field-picker-prototype')).toBeVisible()
    await expect(page.getByTestId('prototype-product-table')).toBeVisible()
    // document.title is set in useEffect — waiting ensures keydown (Ctrl+K) is armed.
    await expect(page).toHaveTitle('Field picker prototype · Braian')
    await page.getByTestId('field-picker-prototype').focus()
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
    await openPalette(page)
    await expect(page.getByTestId('fields-crumb')).toHaveText(/Fields ›$/)
    const articles = page.getByTestId('fields-row-articles')
    await expect(articles.locator('.hit-title')).toHaveText('Articles')
    await expect(page.getByTestId('fields-check-sku')).toBeVisible()
    await expect(page.getByTestId('fields-check-name')).toBeVisible()
    await articles.click()
    await expect(page.getByTestId('fields-crumb')).toContainText('Fields › Articles ›')
    await expect(page.getByTestId('fields-crumb')).not.toContainText('How many linked rows')
    await expect(page.getByTestId('fields-row-all')).toContainText('Every linked row')
    // → drills into All just like Enter
    await page.getByTestId('fields-row-all').hover()
    await page.keyboard.press('ArrowRight')
    await expect(page.getByTestId('fields-crumb')).toContainText('Fields › Articles › All ›')
    await page.getByTestId('fields-row-net_volume').click()
    await expect(page.getByTestId('fields-row-ml')).toBeVisible()
    await expect(page.getByTestId('fields-row-l')).toBeVisible()
    await page.getByTestId('fields-row-ml').click()
    await expect(page.getByRole('columnheader', { name: 'Articles › Net volume (all, ml)' })).toBeVisible()
    await expect(page.getByTestId('fields-check-ml')).toBeVisible()
  })

  test('5 SKU survives Articles→weight→volume→Allergens (exact Braian sequence)', async ({ page }) => {
    await expect(page.getByRole('columnheader', { name: 'SKU' })).toBeVisible()
    await openPalette(page)
    await page.getByTestId('fields-row-articles').click()
    await page.getByTestId('fields-row-all').click()
    await page.getByTestId('fields-row-net_weight').click()
    await page.getByTestId('fields-row-g').click()
    await expect(page.getByRole('columnheader', { name: 'Articles › Net weight (all, g)' })).toBeVisible()
    await page.keyboard.press('ArrowLeft')
    await page.keyboard.press('ArrowLeft')
    // After ← ← we are on Articles cardinality with All focused (not root SKU).
    await expect(page.getByTestId('fields-crumb')).toContainText('Fields › Articles ›')
    if (await page.getByTestId('fields-row-all').count()) {
      await page.getByTestId('fields-row-all').click()
    }
    await page.getByTestId('fields-row-net_volume').click()
    await page.getByTestId('fields-row-ml').click()
    await expect(page.getByRole('columnheader', { name: 'Articles › Net volume (all, ml)' })).toBeVisible()
    await backToRoot(page)
    await expect(page.getByTestId('fields-crumb')).toHaveText(/Fields ›$/)
    // Active row must be Articles (restored), not SKU — Enter must not strip SKU.
    await expect(page.locator('[data-testid="fields-palette"] .hit.on .hit-title')).toHaveText('Articles')
    await page.keyboard.press('Enter') // would have toggled SKU before the pop-restore fix
    await expect(page.getByRole('columnheader', { name: 'SKU' })).toBeVisible()
    await backToRoot(page)
    await page.getByTestId('fields-row-allergens').click()
    await page.getByTestId('fields-row-label').click()
    await page.keyboard.press('Escape')
    await expect(page.getByRole('columnheader', { name: 'SKU' })).toBeVisible()
    await expect(page.getByRole('columnheader', { name: 'Name' })).toBeVisible()
    await expect(page.getByRole('columnheader', { name: 'Allergens (label)' })).toBeVisible()
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

  test('7 Edit field opens first qualifier; check mirrors column; All→First in place', async ({ page }) => {
    await openPalette(page)
    await page.getByTestId('fields-row-articles').click()
    await page.getByTestId('fields-row-all').click()
    await page.getByTestId('fields-row-net_volume').click()
    await page.getByTestId('fields-row-ml').click()
    await page.keyboard.press('Escape')
    // Also add a Specific column so a naive "any articles column" check would mark Specific.
    await openPalette(page)
    await page.getByTestId('fields-row-articles').click()
    await page.getByTestId('fields-row-specific').click()
    await page.getByTestId('fields-row-gtin').click()
    await page.getByTestId('fields-row-7300007000071').click()
    await page.keyboard.press('Escape')

    const header = page.getByRole('columnheader', { name: 'Articles › Net volume (all, ml)' })
    await header.click({ button: 'right' })
    await page.getByTestId('edit-field').click()
    await expect(page.getByTestId('fields-row-all')).toBeVisible()
    await expect(page.getByTestId('fields-check-all')).toBeVisible()
    await expect(page.getByTestId('fields-check-first')).toHaveCount(0)
    await expect(page.getByTestId('fields-check-specific')).toHaveCount(0)
    await page.getByTestId('fields-row-first').click()
    await expect(page.getByRole('columnheader', { name: 'Articles › Net volume (first, ml)' })).toBeVisible()
    await expect(page.getByTestId('fields-palette')).toBeVisible()
    await expect(page.getByTestId('fields-check-first')).toBeVisible()
    await expect(page.getByTestId('fields-check-all')).toHaveCount(0)
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

  test('Specific GTIN identity + Net weight article→g adds column', async ({ page }) => {
    await openPalette(page)
    await page.getByTestId('fields-row-articles').click()
    await page.getByTestId('fields-row-specific').click()
    await page.getByTestId('fields-row-gtin').click()
    // Olive oil 500ml — shared size key with soy
    await page.getByTestId('fields-row-7300007000071').click()
    await expect(page.getByRole('columnheader', { name: 'Articles › GTIN (500ml · 7300007000071)' })).toBeVisible()
    const olive = page.locator('[role="row"][data-testid="standard-grid-row"]').nth(6)
    const soy = page.locator('[role="row"][data-testid="standard-grid-row"]').nth(9)
    await expect(olive.locator('[data-column*="gtin"]')).toHaveText('7300007000071')
    await expect(soy.locator('[data-column*="gtin"]')).toHaveText('—')
    await page.keyboard.press('Escape')

    await openPalette(page)
    await page.getByTestId('fields-row-articles').click()
    await page.getByTestId('fields-row-specific').click()
    await page.getByTestId('fields-row-net_weight').click()
    await page.getByTestId('fields-row-7300008000081').click() // cheddar 200g
    await expect(page.getByTestId('fields-row-g')).toBeVisible()
    await expect(page.getByTestId('fields-row-kg')).toBeVisible()
    await page.getByTestId('fields-row-g').click()
    await expect(page.getByRole('columnheader', { name: 'Articles › Net weight (200g, g)' })).toBeVisible()
    const cheddar = page.locator('[role="row"][data-testid="standard-grid-row"]').nth(7)
    await expect(cheddar.locator('[data-column*="net_weight"][data-column*="specific="]')).toHaveText('200 g')
  })

  test('braian full click-through: How to verify + weight/specific + SKU sequence', async ({ page }) => {
    // 1. Title / blurb / Ctrl+K / SKU+Name
    await expect(page).toHaveTitle('Field picker prototype · Braian')
    await expect(page.getByText('Prototype with mock data. Press Ctrl+K to pick fields.')).toBeVisible()
    await expect(page.getByRole('columnheader', { name: 'SKU' })).toBeVisible()
    await expect(page.getByRole('columnheader', { name: 'Name' })).toBeVisible()
    await openPalette(page)
    await expect(page.getByTestId('fields-check-sku')).toBeVisible()
    await expect(page.getByTestId('fields-check-name')).toBeVisible()
    await expect(page.getByTestId('fields-row-sku')).toHaveClass(/fields-hit-active/)
    await expect(page.getByTestId('fields-check-sku')).toHaveClass(/fields-check/)

    // 2–3. Articles → All → Net volume → ml; +N tooltip; root checks/subtitles
    await page.getByTestId('fields-row-articles').click()
    await page.getByTestId('fields-row-all').click()
    await page.getByTestId('fields-row-net_volume').click()
    await expect(page.getByTestId('fields-row-ml')).toBeVisible()
    await expect(page.getByTestId('fields-row-l')).toBeVisible()
    await page.getByTestId('fields-row-ml').click()
    await expect(page.getByRole('columnheader', { name: 'Articles › Net volume (all, ml)' })).toBeVisible()
    const oat = page.locator('[role="row"][data-testid="standard-grid-row"]').first()
    await expect(oat.locator('[data-column*="net_volume"]')).toHaveText('1000 ml, 250 ml')
    const milk = page.locator('[role="row"][data-testid="standard-grid-row"]').nth(1)
    await expect(milk.locator('[data-column*="net_volume"]')).toContainText('+1')
    await expect(milk.locator('[data-column*="net_volume"]')).toHaveAttribute('title', /1000 ml, 1500 ml, 250 ml/)

    await backToRoot(page)
    await page.getByTestId('fields-row-supplier').click()
    await page.getByTestId('fields-row-country').click()
    await page.keyboard.press('ArrowLeft')
    await expect(page.getByTestId('fields-check-supplier')).toBeVisible()
    await page.getByTestId('fields-row-description').click()
    await page.getByTestId('fields-row-sv').click()
    await page.keyboard.press('ArrowLeft')
    await expect(page.getByTestId('fields-row-description')).toContainText('Description — sv')

    // 4. Filter clears after toggle
    await page.getByTestId('fields-palette-input').fill('sku')
    await page.getByTestId('fields-row-sku').click() // toggle off
    await expect(page.getByTestId('fields-palette-input')).toHaveValue('')
    await page.getByTestId('fields-row-sku').click() // toggle back on
    await expect(page.getByRole('columnheader', { name: 'SKU' })).toBeVisible()

    // Net weight (solids) + empty dash on liquids
    await page.getByTestId('fields-row-articles').click()
    await page.getByTestId('fields-row-all').click()
    await page.getByTestId('fields-row-net_weight').click()
    await page.getByTestId('fields-row-g').click()
    await expect(page.getByRole('columnheader', { name: 'Articles › Net weight (all, g)' })).toBeVisible()
    const yogurt = page.locator('[role="row"][data-testid="standard-grid-row"]').nth(2)
    await expect(yogurt.locator('[data-column*="net_weight"]')).toHaveText('450 g')
    await expect(yogurt.locator('[data-column*="net_volume"]')).toHaveText('—')

    // Exact SKU-survival sequence mid-session: ← ←, volume already present; allergens
    await page.keyboard.press('ArrowLeft')
    await page.keyboard.press('ArrowLeft')
    await backToRoot(page)
    await page.getByTestId('fields-row-allergens').click()
    await page.getByTestId('fields-row-label').click()
    await expect(page.getByRole('columnheader', { name: 'SKU' })).toBeVisible()
    await expect(page.getByRole('columnheader', { name: 'Allergens (label)' })).toBeVisible()
    const juice = page.locator('[role="row"][data-testid="standard-grid-row"]').nth(3)
    await expect(juice.locator('[data-column*="allergens"]')).toHaveText('None')

    // 5. Specific field-first, GTIN identity, Net weight 200g→g
    await backToRoot(page)
    await page.getByTestId('fields-row-articles').click()
    await page.getByTestId('fields-row-specific').click()
    await expect(page.getByTestId('fields-crumb')).toContainText('Specific')
    await page.getByTestId('fields-row-net_volume').click()
    const articleRow = page.locator('[data-testid^="fields-row-730"]').filter({ hasText: '1.5L' }).first()
    await expect(articleRow.locator('small')).toHaveText('Whole milk')
    await expect(articleRow.locator('small')).not.toHaveText(/,/)
    await page.getByTestId('fields-row-7300002000022').click()
    await page.getByTestId('fields-row-ml').click()
    await expect(page.getByRole('columnheader', { name: 'Articles › Net volume (1.5L, ml)' })).toBeVisible()
    await expect(yogurt.locator('[data-column*="specific=7300002000022"]')).toHaveText('—')

    await backToRoot(page)
    await page.getByTestId('fields-row-articles').click()
    // → into Specific
    await page.getByTestId('fields-row-specific').hover()
    await page.keyboard.press('ArrowRight')
    await page.getByTestId('fields-row-net_weight').click()
    await page.getByTestId('fields-row-7300008000081').click()
    await page.getByTestId('fields-row-g').click()
    await expect(page.getByRole('columnheader', { name: 'Articles › Net weight (200g, g)' })).toBeVisible()

    // 6. Edit field on First column — only First checked
    await page.keyboard.press('Escape')
    const weightAll = page.getByRole('columnheader', { name: 'Articles › Net weight (all, g)' })
    await weightAll.click({ button: 'right' })
    await page.getByTestId('edit-field').click()
    await expect(page.getByTestId('fields-check-all')).toBeVisible()
    await page.getByTestId('fields-row-first').click()
    await expect(page.getByRole('columnheader', { name: 'Articles › Net weight (first, g)' })).toBeVisible()
    await expect(page.getByTestId('fields-check-first')).toBeVisible()
    await expect(page.getByTestId('fields-check-all')).toHaveCount(0)
    await expect(page.getByTestId('fields-check-specific')).toHaveCount(0)
    await page.keyboard.press('Escape')

    // Final: SKU and Name still present
    await expect(page.getByRole('columnheader', { name: 'SKU' })).toBeVisible()
    await expect(page.getByRole('columnheader', { name: 'Name' })).toBeVisible()
  })
})
