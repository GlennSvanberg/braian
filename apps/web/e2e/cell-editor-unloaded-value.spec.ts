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

test.describe('cell editor unloaded path-values (Notion 111)', () => {
  test.setTimeout(180_000)

  test('blocks empty editor while pathValues pending; seeds from data; unchanged skips PATCH', async ({
    page,
  }) => {
    const patches: string[] = []
    await page.route('**/v1/objects/*', async (route) => {
      if (route.request().method() === 'PATCH') {
        patches.push(route.request().postData() ?? '')
      }
      await route.continue()
    })

    await login(page)
    await openProducts(page)
    await expect(page.getByTestId('object-table').getByText('Mature cheddar')).toBeVisible({ timeout: 60_000 })

    // Ensure Description (en) is a visible pathValues column.
    await openFieldsPalette(page)
    await enterOn(page, 'fields-row-description')
    await enterOn(page, 'fields-row-en')
    await page.keyboard.press('Escape')
    await expect(page.locator('.head [data-testid*="description"]').first()).toBeVisible()

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

    const loading = page.locator('[data-testid^="path-cell-description"][data-loading="true"]').first()
    await expect(loading).toBeVisible({ timeout: 60_000 })

    const loadingRow = page.locator('[data-testid="object-row"]').filter({
      has: page.locator('[data-testid^="path-cell-description"][data-loading="true"]'),
    }).first()
    const descCell = loadingRow.locator('[data-column^="description"]').first()
    await descCell.dblclick()
    await expect(page.locator('[data-testid^="cell-editor-description"]')).toHaveCount(0)

    release?.()
    // Prefer the seed cheddar row once Names are painted after the batch lands.
    await expect(page.getByTestId('object-table').getByText('Mature cheddar')).toBeVisible({ timeout: 60_000 })
    const row = page.locator('[data-testid="object-row"]').filter({ hasText: 'Mature cheddar' }).first()
    await expect(row.locator('[data-testid^="path-cell-description"]').first()).toContainText(
      /firm cow'?s-milk cheddar/i,
      { timeout: 30_000 },
    )
    await expect(row.locator('[data-testid^="path-cell-description"][data-loading="true"]')).toHaveCount(0)

    await row.locator('[data-column^="description"]').first().dblclick()
    const editor = page.locator('[data-testid^="cell-editor-description"]').first()
    await expect(editor).toBeVisible({ timeout: 10_000 })
    await expect(editor).toHaveValue(/firm cow'?s-milk cheddar/i)

    // Unchanged Enter must not PATCH.
    const before = patches.length
    await editor.press('Enter')
    await expect(page.locator('[data-testid^="cell-editor-description"]')).toHaveCount(0)
    expect(patches.slice(before)).toEqual([])
  })
})
