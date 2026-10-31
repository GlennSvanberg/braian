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

async function openFieldsPalette(page: Page) {
  await page.getByTestId('object-table').focus()
  await page.keyboard.press('Control+k')
  const palette = page.getByTestId('command-palette')
  await expect(palette).toBeVisible()
  await page.getByTestId('palette-input').fill('Select fields')
  await page.keyboard.press('Enter')
  await expect(page.getByTestId('fields-mode')).toBeVisible()
}

async function backToRoot(page: Page) {
  for (let i = 0; i < 12; i += 1) {
    const crumb = await page.getByTestId('fields-crumb').textContent()
    if ((crumb ?? '').trim() === 'Fields ›') return
    await page.keyboard.press('ArrowLeft')
  }
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

async function drillRight(page: Page, rowTestId: string) {
  await expect(page.getByTestId(rowTestId)).toBeVisible()
  for (let i = 0; i < 40; i += 1) {
    const active = page.locator('[data-testid="fields-mode"] [data-active="true"]')
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

function cheddarRow(page: Page) {
  return page.locator('[data-testid="object-row"]').filter({ hasText: 'Mature cheddar' }).first()
}

test.describe('field picker on Products', () => {
  test.setTimeout(180_000)

  test('Description en shows seed text; Esc does not PATCH; Edit field First updates column', async ({ page }) => {
    const patches: string[] = []
    await page.route('**/v1/objects/*', async (route) => {
      if (route.request().method() === 'PATCH') {
        patches.push(route.request().postData() ?? '')
      }
      await route.continue()
    })

    await login(page)
    await openProducts(page)
    await expect(page.getByText('Mature cheddar')).toBeVisible({ timeout: 60_000 })

    await openFieldsPalette(page)

    // Description → en
    await enterOn(page, 'fields-row-description')
    await enterOn(page, 'fields-row-en')
    await expect(page.getByTestId('fields-check-description')).toBeVisible()
    await page.keyboard.press('Escape')
    await expect(page.getByTestId('fields-mode')).toHaveCount(0)

    const row = cheddarRow(page)
    await expect(row).toBeVisible()
    // Seed text must appear in the grid cell (pathValues), not only the detail panel.
    await expect(row.locator('[data-testid^="path-cell-description"]').first()).toContainText(
      /firm cow'?s-milk cheddar/i,
      { timeout: 30_000 },
    )

    // Version before Esc experiment
    const versionBefore = (await row.locator('[data-column="version"]').textContent())?.trim() ?? ''

    // Double-click Description → Esc must not PATCH (and must not clear the value)
    const descCell = row.locator('[data-column^="description"]').first()
    await descCell.dblclick()
    const editor = page.locator('[data-testid^="cell-editor-description"]').first()
    await expect(editor).toBeVisible()
    await expect(editor).toHaveValue(/firm cow'?s-milk cheddar/i)
    patches.length = 0
    await editor.press('Escape')
    await expect(page.locator('[data-testid^="cell-editor-description"]')).toHaveCount(0)
    expect(patches.filter((body) => body.includes('description'))).toEqual([])
    await expect(row.locator('[data-testid^="path-cell-description"]').first()).toContainText(
      /firm cow'?s-milk cheddar/i,
    )
    const versionAfterEsc = (await row.locator('[data-column="version"]').textContent())?.trim() ?? ''
    expect(versionAfterEsc).toBe(versionBefore)

    // Unchanged blur (open editor, blur without edits) → no PATCH
    await descCell.dblclick()
    await expect(page.locator('[data-testid^="cell-editor-description"]').first()).toBeVisible()
    patches.length = 0
    await page.locator('[data-testid="object-table"]').click({ position: { x: 8, y: 8 } })
    await page.waitForTimeout(200)
    expect(patches.filter((body) => body.includes('description'))).toEqual([])

    // Specific net weight → Edit field → First
    await openFieldsPalette(page)
    await drillRight(page, 'fields-row-articles')
    await enterOn(page, 'fields-row-specific')
    await enterOn(page, 'fields-row-net_weight')
    await page.getByTestId('fields-mode-input').fill('07310000000024')
    const cheddar = page.locator('[data-testid^="fields-row-"]').filter({ hasText: '07310000000024' }).first()
    await expect(cheddar).toBeVisible({ timeout: 15_000 })
    const cheddarId = await cheddar.getAttribute('data-testid')
    if (!cheddarId) throw new Error('missing cheddar peer row')
    await enterOn(page, cheddarId)
    await enterOn(page, 'fields-row-g')
    await page.keyboard.press('Escape')

    await expect(row.locator('[data-testid^="hop-cell-articles>net_weight"]').first()).toContainText(/200\s*g/)

    // Edit field… on the specific column → First
    const specificHeader = page.locator('[data-testid^="col-articles>net_weight"]').filter({ hasText: /specific|200|g/i }).first()
    await specificHeader.click({ button: 'right' })
    const menu = page.getByTestId('context-menu')
    await expect(menu).toBeVisible()
    await menu.getByRole('menuitem', { name: /Edit field/i }).click()
    await expect(page.getByTestId('fields-mode')).toBeVisible()
    await enterOn(page, 'fields-row-first')
    await enterOn(page, 'fields-row-net_weight')
    await enterOn(page, 'fields-row-g')
    await page.keyboard.press('Escape')

    await expect(page.locator('[data-testid^="col-articles>net_weight"]').first()).toContainText(/first/i)
    await expect(page.locator('[data-testid^="col-articles>net_weight"]').first()).not.toContainText(/specific/i)
    // First of ART-CHED-200 / ART-CHED-400 by peer key → 200 g or 0.2 kg depending on unit
    await expect(row.locator('[data-testid^="hop-cell-articles>net_weight"]').first()).toContainText(/200\s*g|0\.2\s*kg/)
  })


  test('typing in fields palette does not open New Product', async ({ page }) => {
    await login(page)
    await openProducts(page)
    await openFieldsPalette(page)
    await page.getByTestId('fields-mode-input').fill('net')
    await page.keyboard.press('n')
    await page.keyboard.press('Enter')
    await expect(page.getByTestId('fields-mode')).toBeVisible()
    await expect(page.locator('form.create-row, [data-testid="new-object"]')).toHaveCount(0)
  })

  test('full GTIN Specific search finds cheddar; breadcrumb shows ART-CHED key', async ({ page }) => {
    await login(page)
    await openProducts(page)
    await expect(page.getByText('Mature cheddar')).toBeVisible({ timeout: 60_000 })

    await openFieldsPalette(page)
    await drillRight(page, 'fields-row-articles')
    await enterOn(page, 'fields-row-specific')
    await enterOn(page, 'fields-row-net_weight')
    await page.getByTestId('fields-mode-input').fill('07310000000024')
    const cheddar = page
      .locator('[data-testid^="fields-row-"]')
      .filter({ hasText: /07310000000024|ART-CHED-200/i })
      .first()
    await expect(cheddar).toBeVisible({ timeout: 20_000 })
    const cheddarId = await cheddar.getAttribute('data-testid')
    if (!cheddarId) throw new Error('missing cheddar peer')
    await enterOn(page, cheddarId)
    await expect(page.getByTestId('fields-crumb')).toContainText(/ART-CHED-200/i)
    await expect(page.getByTestId('fields-crumb')).not.toContainText(/018f1000/i)
    await enterOn(page, 'fields-row-g')
    await page.keyboard.press('Escape')
    const row = cheddarRow(page)
    await expect(row.locator('[data-testid^="hop-cell-articles>net_weight"]').first()).toContainText(/200\s*g/)
    await expect(page.locator('[data-testid^="col-articles>net_weight"]').filter({ hasText: /ART-CHED-200/i })).toBeVisible()
  })

  test('Edit field replaces all→first in place after back navigation', async ({ page }) => {
    await login(page)
    await openProducts(page)
    await expect(page.getByText('Mature cheddar')).toBeVisible({ timeout: 60_000 })

    await openFieldsPalette(page)
    await drillRight(page, 'fields-row-articles')
    await enterOn(page, 'fields-row-all')
    await enterOn(page, 'fields-row-net_weight')
    await enterOn(page, 'fields-row-kg')
    await page.keyboard.press('Escape')

    const headers = () => page.locator('[data-testid^="col-articles>net_weight"]')
    const allHeader = page.locator('[data-testid^="col-articles>net_weight"]').filter({ hasText: /\ball\b/i }).first()
    await expect(allHeader).toBeVisible()
    const beforeHeaders = await headers().allTextContents()
    const beforeCount = beforeHeaders.length
    const allIndex = beforeHeaders.findIndex((text) => /\ball\b/i.test(text))
    expect(allIndex).toBeGreaterThanOrEqual(0)

    await allHeader.click({ button: 'right' })
    await page.getByTestId('context-menu').getByRole('menuitem', { name: /Edit field/i }).click()
    await expect(page.getByTestId('fields-mode')).toBeVisible()
    await expect(page.getByTestId('fields-mode-input')).toBeFocused({ timeout: 5_000 })

    // Exact repro: ←, back in, First, Net weight, kg, Enter
    await page.keyboard.press('ArrowLeft')
    await drillRight(page, 'fields-row-articles')
    await enterOn(page, 'fields-row-first')
    await enterOn(page, 'fields-row-net_weight')
    await enterOn(page, 'fields-row-kg')
    await page.keyboard.press('Escape')

    const afterHeaders = await headers().allTextContents()
    expect(afterHeaders.length).toBe(beforeCount)
    expect(afterHeaders.some((text) => /\ball\b/i.test(text))).toBe(false)
    expect(afterHeaders[allIndex] ?? '').toMatch(/first/i)

    await page.reload()
    await expect(page.getByTestId('object-table')).toBeVisible({ timeout: 60_000 })
    const reloaded = await headers().allTextContents()
    expect(reloaded.length).toBe(beforeCount)
    expect(reloaded.some((text) => /\ball\b/i.test(text))).toBe(false)
    expect(reloaded.some((text) => /first/i.test(text))).toBe(true)
  })

  test('Edit field merges into an existing identical first column', async ({ page }) => {
    await login(page)
    await openProducts(page)
    await expect(page.getByText('Mature cheddar')).toBeVisible({ timeout: 60_000 })

    // first kg
    await openFieldsPalette(page)
    await drillRight(page, 'fields-row-articles')
    await enterOn(page, 'fields-row-first')
    await enterOn(page, 'fields-row-net_weight')
    await enterOn(page, 'fields-row-kg')
    await page.keyboard.press('Escape')
    // all kg
    await openFieldsPalette(page)
    await drillRight(page, 'fields-row-articles')
    await enterOn(page, 'fields-row-all')
    await enterOn(page, 'fields-row-net_weight')
    await enterOn(page, 'fields-row-kg')
    await page.keyboard.press('Escape')

    const headers = page.locator('[data-testid^="col-articles>net_weight"]')
    await expect(headers).toHaveCount(2)
    const allHeader = page.locator('[data-testid^="col-articles>net_weight"]').filter({ hasText: /\ball\b/i }).first()
    await allHeader.click({ button: 'right' })
    await page.getByTestId('context-menu').getByRole('menuitem', { name: /Edit field/i }).click()
    await expect(page.getByTestId('fields-mode')).toBeVisible()
    await page.keyboard.press('ArrowLeft')
    await drillRight(page, 'fields-row-articles')
    await enterOn(page, 'fields-row-first')
    await enterOn(page, 'fields-row-net_weight')
    await enterOn(page, 'fields-row-kg')
    await page.keyboard.press('Escape')

    await expect(headers).toHaveCount(1)
    await expect(headers.first()).toContainText(/first/i)
  })

  test('re-adding hidden hops appends All then Specific then First', async ({ page }) => {
    await login(page)
    await openProducts(page)
    await expect(page.getByText('Mature cheddar')).toBeVisible({ timeout: 60_000 })

    async function addHop(card: 'all' | 'specific' | 'first') {
      await openFieldsPalette(page)
      await drillRight(page, 'fields-row-articles')
      if (card === 'specific') {
        await enterOn(page, 'fields-row-specific')
        await enterOn(page, 'fields-row-net_weight')
        await page.getByTestId('fields-mode-input').fill('ART-CHED-200')
        const peer = page.locator('[data-testid^="fields-row-"]').filter({ hasText: /ART-CHED-200/i }).first()
        await expect(peer).toBeVisible({ timeout: 20_000 })
        const id = await peer.getAttribute('data-testid')
        if (!id) throw new Error('peer')
        await enterOn(page, id)
        await enterOn(page, 'fields-row-kg')
      } else {
        await enterOn(page, `fields-row-${card}`)
        await enterOn(page, 'fields-row-net_weight')
        await enterOn(page, 'fields-row-kg')
      }
      await page.keyboard.press('Escape')
    }

    // First pass leaves hidden prefs entries when we Hide column.
    for (const card of ['first', 'all', 'specific'] as const) {
      await addHop(card)
    }
    const hopHeaders = () => page.locator('[data-testid^="col-articles>net_weight"]')
    await expect(hopHeaders()).toHaveCount(3)

    // Braian: hide every hop column (visible:false, old order kept in prefs).
    for (let i = 0; i < 5 && (await hopHeaders().count()) > 0; i += 1) {
      await hopHeaders().first().click({ button: 'right' })
      const menu = page.getByTestId('context-menu')
      await expect(menu).toBeVisible()
      await menu.getByRole('menuitem', { name: 'Hide column' }).click()
      await expect(menu).toHaveCount(0)
    }
    await expect(hopHeaders()).toHaveCount(0)

    // Re-add in Braian’s order: All → Specific ART-CHED-200 → First
    for (const card of ['all', 'specific', 'first'] as const) {
      await addHop(card)
    }
    const texts = await hopHeaders().allTextContents()
    expect(texts).toHaveLength(3)
    expect(texts[0]).toMatch(/\ball\b/i)
    expect(texts[1]).toMatch(/ART-CHED-200/i)
    expect(texts[2]).toMatch(/first/i)

    await page.reload()
    await expect(page.getByTestId('object-table')).toBeVisible({ timeout: 60_000 })
    const reloaded = await hopHeaders().allTextContents()
    expect(reloaded).toHaveLength(3)
    expect(reloaded[0]).toMatch(/\ball\b/i)
    expect(reloaded[1]).toMatch(/ART-CHED-200/i)
    expect(reloaded[2]).toMatch(/first/i)
  })

  test('header and body x/width match for system and hop columns (≤1px)', async ({ page }) => {
    await login(page)
    await openProducts(page)
    await expect(page.getByText('Mature cheddar')).toBeVisible({ timeout: 60_000 })

    // Food-seed Products standard columns are already visible; add one hop.
    await openFieldsPalette(page)
    await drillRight(page, 'fields-row-articles')
    await enterOn(page, 'fields-row-all')
    await enterOn(page, 'fields-row-net_weight')
    await enterOn(page, 'fields-row-kg')
    await page.keyboard.press('Escape')

    const headers = page.locator('.head [data-testid^="col-"]')
    const count = await headers.count()
    expect(count).toBeGreaterThan(5)
    // Must include system columns that previously drifted (Type/Status/Version).
    await expect(page.locator('.head [data-testid="col-type"]')).toBeVisible()
    await expect(page.locator('.head [data-testid="col-status"]')).toBeVisible()
    await expect(page.locator('.head [data-testid="col-version"]')).toBeVisible()

    const row = page.locator('[data-testid="object-row"]').filter({ hasText: 'Mature cheddar' }).first()
    await expect(row).toBeVisible()
    for (let i = 0; i < count; i += 1) {
      const header = headers.nth(i)
      const column = await header.getAttribute('data-column')
      expect(column).toBeTruthy()
      const cell = row.locator(`[data-column=${JSON.stringify(column)}]`)
      await expect(cell).toBeVisible()
      const hb = await header.boundingBox()
      const cb = await cell.boundingBox()
      expect(hb && cb, `missing box for ${column}`).toBeTruthy()
      expect(Math.abs(hb!.x - cb!.x), `x drift on ${column}`).toBeLessThanOrEqual(1)
      expect(Math.abs(hb!.width - cb!.width), `width drift on ${column}`).toBeLessThanOrEqual(1)
    }
  })

  test('double-click Description en opens cell editor, not only Detail', async ({ page }) => {
    await login(page)
    await openProducts(page)
    await expect(page.getByText('Mature cheddar')).toBeVisible({ timeout: 60_000 })

    await openFieldsPalette(page)
    await enterOn(page, 'fields-row-description')
    await enterOn(page, 'fields-row-en')
    await page.keyboard.press('Escape')

    // Close the Detail pane if present so a regression that only reopens Detail is visible.
    const detailClose = page.locator('.pane').filter({ hasText: /^Product|^Detail/ }).getByRole('button', { name: /close|×|x/i }).first()
    if (await detailClose.count()) {
      await detailClose.click({ force: true }).catch(() => undefined)
    }
    // Also try the dock tab close affordance.
    const tabClose = page.locator('.dock-root button, .pane-tab button, .tabs button').filter({ hasText: /^×$|^x$/i }).first()
    if (await tabClose.count()) {
      await tabClose.click({ force: true }).catch(() => undefined)
    }

    const row = page.locator('[data-testid="object-row"]').filter({ hasText: 'Mature cheddar' }).first()
    const descCell = row.locator('[data-column^="description"]').first()
    await expect(descCell).toBeVisible({ timeout: 30_000 })
    // Description is loaded through pathValues. The editor copies the painted
    // text once, so a double-click on the loading placeholder opens an empty
    // input and the value never shows up.
    await expect(descCell).toContainText(/firm cow'?s-milk cheddar/i, { timeout: 30_000 })
    await descCell.dblclick()
    const editor = page.locator('[data-testid^="cell-editor-description"]').first()
    await expect(editor).toBeVisible({ timeout: 10_000 })
    await expect(editor).toBeFocused()
    await expect(editor).toHaveValue(/firm cow'?s-milk cheddar/i)
    // Editor present means Detail-only reopen did not win the double-click.
  })

  test('column hide survives Articles round-trip without waiting', async ({ page }) => {
    await login(page)
    await openProducts(page)
    await expect(page.getByText('Mature cheddar')).toBeVisible({ timeout: 60_000 })

    const headCols = () => page.locator('.head [data-testid^="col-"]')
    await expect(page.locator('.head [data-testid="col-brand"]')).toBeVisible()

    // Column change: hide Brand, then switch tabs immediately (no wait for PUT).
    await page.locator('.head [data-testid="col-brand"]').click({ button: 'right' })
    const menu = page.getByTestId('context-menu')
    await expect(menu).toBeVisible()
    await menu.getByRole('menuitem', { name: 'Hide column' }).click()
    await expect(menu).toHaveCount(0)
    await expect(page.locator('.head [data-testid="col-brand"]')).toHaveCount(0)

    const expected = await headCols().evaluateAll((nodes) =>
      nodes.map((n) => n.getAttribute('data-testid')),
    )
    expect(expected).toContain('col-number')
    expect(expected).toContain('col-name')
    expect(expected).toContain('col-status')
    expect(expected).not.toContain('col-brand')

    await page.locator('nav.tabs').getByRole('button', { name: /^Articles/ }).click()
    await expect(page.locator('.pane-tab', { hasText: 'Articles' })).toBeVisible({ timeout: 30_000 })
    await openProducts(page)
    await expect(page.getByText('Mature cheddar')).toBeVisible({ timeout: 60_000 })

    await expect.poll(async () =>
      headCols().evaluateAll((nodes) => nodes.map((n) => n.getAttribute('data-testid'))),
    ).toEqual(expected)

    await page.reload()
    await expect(page.getByTestId('object-table')).toBeVisible({ timeout: 60_000 })
    await expect(page.getByText('Mature cheddar')).toBeVisible({ timeout: 60_000 })
    await expect.poll(async () =>
      headCols().evaluateAll((nodes) => nodes.map((n) => n.getAttribute('data-testid'))),
    ).toEqual(expected)
  })

  test('right-click header with palette open closes palette then shows Hide column', async ({ page }) => {
    await login(page)
    await openProducts(page)
    await expect(page.getByText('Mature cheddar')).toBeVisible({ timeout: 60_000 })
    await expect(page.locator('.head [data-testid="col-brand"]')).toBeVisible()

    await openFieldsPalette(page)
    await expect(page.getByTestId('fields-mode')).toBeVisible()

    // Palette may overlay the header; force the contextmenu onto the Brand cell so
    // shared-context-menu still receives it (same path as a real header right-click).
    const brand = page.locator('.head [data-testid="col-brand"]')
    await brand.dispatchEvent('contextmenu', { bubbles: true, cancelable: true, button: 2 })
    const menu = page.getByTestId('context-menu')
    await expect(menu).toBeVisible()
    await expect(menu.getByRole('menuitem', { name: 'Hide column' })).toBeVisible()
    // Current #105 behaviour: opening the header menu closes the palette first.
    // Glenn can decide later whether the palette should stay open (decision 9).
    await expect(page.getByTestId('fields-mode')).toHaveCount(0)
    await expect(page.getByTestId('command-palette')).toHaveCount(0)
  })
})
