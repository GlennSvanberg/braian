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

  test('full flow: Description en, Allergens code, Articles net weight kg, Specific GTIN', async ({ page }) => {
    await login(page)
    await openProducts(page)
    await expect(page.getByText('Organic oat drink').or(page.getByText('PROD-OAT'))).toBeVisible({ timeout: 60_000 })

    await openFieldsPalette(page)
    await expect(page.getByTestId('fields-crumb')).toHaveText(/Fields/)

    // Description → en (seed has English only)
    await enterOn(page, 'fields-row-description')
    await enterOn(page, 'fields-row-en')
    await expect(page.getByTestId('fields-check-description')).toBeVisible()

    await backToRoot(page)

    // Allergens → code
    await enterOn(page, 'fields-row-allergens')
    await enterOn(page, 'fields-row-code')
    await expect(page.getByTestId('fields-check-allergens')).toBeVisible()

    await backToRoot(page)

    // Articles › All, as list › Net weight › kg
    await drillRight(page, 'fields-row-articles')
    await enterOn(page, 'fields-row-all')
    await enterOn(page, 'fields-row-net_weight')
    await enterOn(page, 'fields-row-kg')
    await expect(page.getByTestId('col-articles>net_weight#cardinality:all|unit:kg').or(page.locator('[data-testid^="col-articles>net_weight"]'))).toBeVisible()

    await backToRoot(page)

    // Specific… → Net weight → Cheddar article (GTIN) → g
    await drillRight(page, 'fields-row-articles')
    await enterOn(page, 'fields-row-specific')
    await enterOn(page, 'fields-row-net_weight')
    await page.getByTestId('fields-mode-input').fill('07310000000024')
    const cheddar = page.locator('[data-testid^="fields-row-"]').filter({ hasText: '07310000000024' }).first()
    await expect(cheddar).toBeVisible({ timeout: 15_000 })
    const cheddarId = await cheddar.getAttribute('data-testid')
    if (!cheddarId) throw new Error('cheddar row missing test id')
    await enterOn(page, cheddarId)
    await enterOn(page, 'fields-row-g')

    await page.keyboard.press('Escape')
    await expect(page.getByTestId('fields-mode')).toHaveCount(0)

    // Cell values from demo seed: Mature cheddar description + 0.2 kg / 0.4 kg articles
    const row = cheddarRow(page)
    await expect(row).toBeVisible()
    await expect(row).toContainText(/firm cow'?s-milk cheddar|Synthetic demo recipe/i)
    // Net weight all, kg: ART-CHED-200 = 0.2 kg, ART-CHED-400 = 0.4 kg → "0.2 kg, 0.4 kg" (order by peer key)
    await expect(row.locator('[data-testid^="hop-cell-articles>net_weight"]').first()).toContainText(/0\.[24] kg/)
  })

  test('column prefs survive an immediate reload', async ({ page }) => {
    await login(page)
    await openProducts(page)
    await expect(page.getByText('Mature cheddar').or(page.getByText('PROD-CHEDDAR'))).toBeVisible({ timeout: 60_000 })

    await openFieldsPalette(page)
    await enterOn(page, 'fields-row-brand')
    await page.keyboard.press('Escape')
    await expect(page.getByTestId('fields-mode')).toHaveCount(0)
    await expect(page.locator('[data-testid^="col-brand"]').first()).toBeVisible()

    // Reload immediately — must not race the prefs PUT
    await page.reload()
    await expect(page.getByTestId('object-table')).toBeVisible({ timeout: 60_000 })
    await expect(page.locator('[data-testid^="col-brand"]').first()).toBeVisible({ timeout: 30_000 })
  })
})
