import { expect, test, type Page } from '@playwright/test'

/** Open via Ctrl+K (Braian's path). No click/retry fallbacks. */
async function openPalette(page: Page) {
  if (await page.getByTestId('fields-palette').isVisible().catch(() => false)) return
  await page.getByTestId('field-picker-prototype').focus()
  await page.keyboard.press('Control+k')
  await expect(page.getByTestId('fields-palette')).toBeVisible()
}

async function backToRoot(page: Page) {
  for (let i = 0; i < 10; i += 1) {
    const crumb = await page.getByTestId('fields-crumb').textContent()
    if (crumb === 'Fields ›') return
    await page.keyboard.press('ArrowLeft')
  }
}

/**
 * Move the active highlight with ArrowUp/ArrowDown onto rowTestId, then press Enter once.
 * No clicks and no retries of Enter — a swallowed key fails the test.
 */
async function enterOn(page: Page, rowTestId: string) {
  await expect(page.getByTestId(rowTestId)).toBeVisible()
  for (let i = 0; i < 40; i += 1) {
    const active = page.locator('[data-testid="fields-palette"] [data-active="true"]')
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

/** Move highlight onto a drill row, then press → once (same as Enter for drill rows). */
async function drillRight(page: Page, rowTestId: string) {
  await expect(page.getByTestId(rowTestId)).toBeVisible()
  for (let i = 0; i < 40; i += 1) {
    const active = page.locator('[data-testid="fields-palette"] [data-active="true"]')
    const activeId = await active.getAttribute('data-testid')
    if (activeId === rowTestId) {
      await page.keyboard.press('ArrowRight')
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
  throw new Error(`Could not move active highlight to ${rowTestId} for →`)
}

test.describe('field picker prototype', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/prototype/field-picker')
    await expect(page.getByTestId('field-picker-prototype')).toBeVisible()
    await expect(page.getByTestId('prototype-product-table')).toBeVisible()
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
    await enterOn(page, 'fields-row-articles')
    await expect(page.getByTestId('fields-crumb')).toContainText('Fields › Articles ›')
    await expect(page.getByTestId('fields-crumb')).not.toContainText('How many linked rows')
    await expect(page.getByTestId('fields-row-all')).toContainText('Every linked row')
    await drillRight(page, 'fields-row-all')
    await expect(page.getByTestId('fields-crumb')).toContainText('Fields › Articles › All ›')
    await enterOn(page, 'fields-row-net_volume')
    await expect(page.getByTestId('fields-row-ml')).toBeVisible()
    await expect(page.getByTestId('fields-row-l')).toBeVisible()
    await enterOn(page, 'fields-row-ml')
    await expect(page.getByRole('columnheader', { name: 'Articles › Net volume (all, ml)' })).toBeVisible()
    await expect(page.getByTestId('fields-check-ml')).toBeVisible()
  })

  test('5 SKU survives; Allergens→Label adds column with one Enter', async ({ page }) => {
    await expect(page.getByRole('columnheader', { name: 'SKU' })).toBeVisible()
    await openPalette(page)
    await enterOn(page, 'fields-row-articles')
    await enterOn(page, 'fields-row-all')
    await enterOn(page, 'fields-row-net_weight')
    await enterOn(page, 'fields-row-g')
    await expect(page.getByRole('columnheader', { name: 'Articles › Net weight (all, g)' })).toBeVisible()
    await page.keyboard.press('ArrowLeft')
    await page.keyboard.press('ArrowLeft')
    await expect(page.getByTestId('fields-crumb')).toContainText('Fields › Articles ›')
    // After ←← we are on cardinality; re-enter All then Net volume with one Enter each.
    await enterOn(page, 'fields-row-all')
    await enterOn(page, 'fields-row-net_volume')
    await enterOn(page, 'fields-row-ml')
    await expect(page.getByRole('columnheader', { name: 'Articles › Net volume (all, ml)' })).toBeVisible()
    await backToRoot(page)
    await expect(page.locator('[data-testid="fields-palette"] .hit.on .hit-title')).toHaveText('Articles')
    await page.keyboard.press('Enter')
    await expect(page.getByRole('columnheader', { name: 'SKU' })).toBeVisible()
    await backToRoot(page)
    // Exact Braian allergens sequence: one Enter on Allergens, one Enter on Label.
    await enterOn(page, 'fields-row-allergens')
    await expect(page.getByTestId('fields-row-label')).toBeVisible()
    await enterOn(page, 'fields-row-label')
    await expect(page.getByRole('columnheader', { name: 'Allergens (label)' })).toBeVisible()
    await page.keyboard.press('Escape')
    await expect(page.getByRole('columnheader', { name: 'SKU' })).toBeVisible()
    await expect(page.getByRole('columnheader', { name: 'Name' })).toBeVisible()
    await expect(page.getByRole('columnheader', { name: 'Allergens (label)' })).toBeVisible()
  })

  test('6 stay on level for Description; Edit field does not close; focus on checked SV', async ({ page }) => {
    await openPalette(page)
    await enterOn(page, 'fields-row-description')
    await enterOn(page, 'fields-row-en')
    await expect(page.getByRole('columnheader', { name: 'Description (en)' })).toBeVisible()
    await expect(page.getByTestId('fields-check-en')).toBeVisible()
    // Stay on level: add SV, then toggle EN off so only SV remains (Braian's case).
    await enterOn(page, 'fields-row-sv')
    await expect(page.getByRole('columnheader', { name: 'Description (sv)' })).toBeVisible()
    await enterOn(page, 'fields-row-en')
    await expect(page.getByRole('columnheader', { name: 'Description (en)' })).toHaveCount(0)
    await expect(page.getByTestId('fields-check-sv')).toBeVisible()
    await expect(page.getByTestId('fields-check-en')).toHaveCount(0)
    await page.keyboard.press('Escape')

    await openPalette(page)
    await enterOn(page, 'fields-row-description')
    // Focus must land on already-selected SV, not EN
    await expect(page.getByTestId('fields-row-sv')).toHaveAttribute('data-active', 'true')
    await expect(page.getByTestId('fields-check-sv')).toBeVisible()
    await page.keyboard.press('Escape')

    const header = page.getByRole('columnheader', { name: 'Description (sv)' })
    await header.click({ button: 'right' })
    await page.getByTestId('edit-field').click()
    await expect(page.getByTestId('fields-palette')).toBeVisible()
    await expect(page.getByTestId('fields-row-sv')).toHaveAttribute('data-active', 'true')
    await enterOn(page, 'fields-row-en')
    await expect(page.getByTestId('fields-palette')).toBeVisible()
    await expect(page.getByRole('columnheader', { name: 'Description (en)' })).toBeVisible()
  })

  test('7 Edit field opens first qualifier; check mirrors column; All→First in place', async ({ page }) => {
    await openPalette(page)
    await enterOn(page, 'fields-row-articles')
    await enterOn(page, 'fields-row-all')
    await enterOn(page, 'fields-row-net_volume')
    await enterOn(page, 'fields-row-ml')
    await page.keyboard.press('Escape')

    await openPalette(page)
    await enterOn(page, 'fields-row-articles')
    await enterOn(page, 'fields-row-specific')
    await enterOn(page, 'fields-row-gtin')
    await enterOn(page, 'fields-row-7300007000071')
    await page.keyboard.press('Escape')

    const header = page.getByRole('columnheader', { name: 'Articles › Net volume (all, ml)' })
    await header.click({ button: 'right' })
    await page.getByTestId('edit-field').click()
    await expect(page.getByTestId('fields-row-all')).toBeVisible()
    await expect(page.getByTestId('fields-check-all')).toBeVisible()
    await expect(page.getByTestId('fields-row-all')).toHaveAttribute('data-active', 'true')
    await expect(page.getByTestId('fields-check-first')).toHaveCount(0)
    await expect(page.getByTestId('fields-check-specific')).toHaveCount(0)
    await enterOn(page, 'fields-row-first')
    await expect(page.getByRole('columnheader', { name: 'Articles › Net volume (first, ml)' })).toBeVisible()
    await expect(page.getByTestId('fields-palette')).toBeVisible()
    await expect(page.getByTestId('fields-check-first')).toBeVisible()
    await expect(page.getByTestId('fields-check-all')).toHaveCount(0)
    const firstCell = page.locator('[role="row"][data-testid="standard-grid-row"]').first().locator('[data-column*="net_volume"]')
    await expect(firstCell).toHaveText('1000 ml')
  })

  test('8 unit conversion and storage °C; Remove column', async ({ page }) => {
    await openPalette(page)
    await enterOn(page, 'fields-row-articles')
    await enterOn(page, 'fields-row-all')
    await enterOn(page, 'fields-row-net_volume')
    await enterOn(page, 'fields-row-ml')
    await page.keyboard.press('Escape')
    const oatVolume = page.locator('[role="row"][data-testid="standard-grid-row"]').first().locator('[data-column*="net_volume"]')
    await expect(oatVolume).toHaveText('1000 ml, 250 ml')

    await openPalette(page)
    await enterOn(page, 'fields-row-storage_temperature')
    await enterOn(page, 'fields-row-min')
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

  test('Specific GTIN identity; Cheddar Net weight → g with one Enter', async ({ page }) => {
    await openPalette(page)
    await enterOn(page, 'fields-row-articles')
    await enterOn(page, 'fields-row-specific')
    await enterOn(page, 'fields-row-gtin')
    await enterOn(page, 'fields-row-7300007000071')
    await expect(page.getByRole('columnheader', { name: 'Articles › GTIN (500ml · 7300007000071)' })).toBeVisible()
    const olive = page.locator('[role="row"][data-testid="standard-grid-row"]').nth(6)
    const soy = page.locator('[role="row"][data-testid="standard-grid-row"]').nth(9)
    await expect(olive.locator('[data-column*="gtin"]')).toHaveText('7300007000071')
    await expect(soy.locator('[data-column*="gtin"]')).toHaveText('—')
    await page.keyboard.press('Escape')

    await openPalette(page)
    await enterOn(page, 'fields-row-articles')
    await enterOn(page, 'fields-row-specific')
    await enterOn(page, 'fields-row-net_weight')
    await enterOn(page, 'fields-row-7300008000081') // cheddar 200g
    await expect(page.getByTestId('fields-row-g')).toBeVisible()
    await expect(page.getByTestId('fields-row-kg')).toBeVisible()
    // One Enter on g must add the column (no swallowed first press).
    await enterOn(page, 'fields-row-g')
    await expect(page.getByRole('columnheader', { name: 'Articles › Net weight (200g, g)' })).toBeVisible()
    const cheddar = page.locator('[role="row"][data-testid="standard-grid-row"]').nth(7)
    await expect(cheddar.locator('[data-column*="net_weight"][data-column*="specific="]')).toHaveText('200 g')
  })

  test('braian full click-through: How to verify + allergens Enter + cheddar Enter + focus', async ({ page }) => {
    await expect(page).toHaveTitle('Field picker prototype · Braian')
    await expect(page.getByRole('columnheader', { name: 'SKU' })).toBeVisible()
    await expect(page.getByRole('columnheader', { name: 'Name' })).toBeVisible()
    await openPalette(page)
    await expect(page.getByTestId('fields-check-sku')).toBeVisible()
    await expect(page.getByTestId('fields-check-name')).toBeVisible()

    await enterOn(page, 'fields-row-articles')
    await enterOn(page, 'fields-row-all')
    await enterOn(page, 'fields-row-net_volume')
    await enterOn(page, 'fields-row-ml')
    await expect(page.getByRole('columnheader', { name: 'Articles › Net volume (all, ml)' })).toBeVisible()
    const oat = page.locator('[role="row"][data-testid="standard-grid-row"]').first()
    await expect(oat.locator('[data-column*="net_volume"]')).toHaveText('1000 ml, 250 ml')

    await backToRoot(page)
    await enterOn(page, 'fields-row-supplier')
    await enterOn(page, 'fields-row-country')
    await page.keyboard.press('ArrowLeft')
    await expect(page.getByTestId('fields-check-supplier')).toBeVisible()
    await enterOn(page, 'fields-row-description')
    await enterOn(page, 'fields-row-sv')
    await page.keyboard.press('ArrowLeft')
    await expect(page.getByTestId('fields-row-description')).toContainText('Description — sv')
    // Re-enter Description: focus on SV
    await enterOn(page, 'fields-row-description')
    await expect(page.getByTestId('fields-row-sv')).toHaveAttribute('data-active', 'true')
    await page.keyboard.press('ArrowLeft')

    // Net weight (all, g)
    await enterOn(page, 'fields-row-articles')
    await enterOn(page, 'fields-row-all')
    await enterOn(page, 'fields-row-net_weight')
    await enterOn(page, 'fields-row-g')
    await expect(page.getByRole('columnheader', { name: 'Articles › Net weight (all, g)' })).toBeVisible()

    // Braian allergens sequence after weight/volume path
    await page.keyboard.press('ArrowLeft')
    await page.keyboard.press('ArrowLeft')
    await backToRoot(page)
    await enterOn(page, 'fields-row-allergens')
    await enterOn(page, 'fields-row-label')
    await expect(page.getByRole('columnheader', { name: 'Allergens (label)' })).toBeVisible()
    await expect(page.getByRole('columnheader', { name: 'SKU' })).toBeVisible()
    const juice = page.locator('[role="row"][data-testid="standard-grid-row"]').nth(3)
    await expect(juice.locator('[data-column*="allergens"]')).toHaveText('None')

    // Specific → Net weight → Cheddar → g (one Enter each)
    await backToRoot(page)
    await enterOn(page, 'fields-row-articles')
    await drillRight(page, 'fields-row-specific')
    await enterOn(page, 'fields-row-net_weight')
    await enterOn(page, 'fields-row-7300008000081')
    await enterOn(page, 'fields-row-g')
    await expect(page.getByRole('columnheader', { name: 'Articles › Net weight (200g, g)' })).toBeVisible()

    await page.keyboard.press('Escape')
    const weightAll = page.getByRole('columnheader', { name: 'Articles › Net weight (all, g)' })
    await weightAll.click({ button: 'right' })
    await page.getByTestId('edit-field').click()
    await expect(page.getByTestId('fields-check-all')).toBeVisible()
    await expect(page.getByTestId('fields-row-all')).toHaveAttribute('data-active', 'true')
    await enterOn(page, 'fields-row-first')
    await expect(page.getByRole('columnheader', { name: 'Articles › Net weight (first, g)' })).toBeVisible()
    await page.keyboard.press('Escape')

    await expect(page.getByRole('columnheader', { name: 'SKU' })).toBeVisible()
    await expect(page.getByRole('columnheader', { name: 'Name' })).toBeVisible()
  })
})
