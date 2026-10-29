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
    await expect(page.getByText('Mature cheddar').or(page.getByText('PROD-CHEDDAR'))).toBeVisible({ timeout: 60_000 })

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
    await page.keyboard.press('Escape')

    await expect(page.locator('[data-testid^="col-articles>net_weight"]').first()).toContainText(/first/i)
    await expect(page.locator('[data-testid^="col-articles>net_weight"]').first()).not.toContainText(/specific/i)
    // First of ART-CHED-200 / ART-CHED-400 by peer key → 200 g or 0.2 kg depending on unit
    await expect(row.locator('[data-testid^="hop-cell-articles>net_weight"]').first()).toContainText(/200\s*g|0\.2\s*kg/)
  })

  test('Edit field All→First updates header and cell', async ({ page }) => {
    await login(page)
    await openProducts(page)
    await expect(page.getByText('Mature cheddar')).toBeVisible({ timeout: 60_000 })

    await openFieldsPalette(page)
    await drillRight(page, 'fields-row-articles')
    await enterOn(page, 'fields-row-all')
    await enterOn(page, 'fields-row-net_weight')
    await enterOn(page, 'fields-row-kg')
    await page.keyboard.press('Escape')

    const row = cheddarRow(page)
    await expect(row.locator('[data-testid^="hop-cell-articles>net_weight"]').first()).toContainText(/0\.[24]\s*kg/)

    const header = page.locator('[data-testid^="col-articles>net_weight"]').first()
    await header.click({ button: 'right' })
    await page.getByTestId('context-menu').getByRole('menuitem', { name: /Edit field/i }).click()
    await expect(page.getByTestId('fields-mode')).toBeVisible()
    await enterOn(page, 'fields-row-first')
    await page.keyboard.press('Escape')

    await expect(page.locator('[data-testid^="col-articles>net_weight"]').first()).toContainText(/first/i)
    await expect(page.locator('[data-testid^="col-articles>net_weight"]').first()).not.toContainText(/\ball\b/i)
    await expect(row.locator('[data-testid^="hop-cell-articles>net_weight"]').first()).toContainText(/0\.[24]\s*kg/)
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
})
