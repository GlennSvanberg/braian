import { expect, test, type Page } from '@playwright/test'
import { mkdirSync } from 'node:fs'
import { demoCredentials } from './demo'

const shots = '/opt/cursor/artifacts/final2'

async function login(page: Page) {
  const demo = demoCredentials()
  await page.goto('/')
  await page.getByTestId('email').fill(demo.email)
  await page.getByTestId('password').fill(demo.password)
  await page.getByRole('button', { name: 'Sign in' }).click()
  await expect(page.getByTestId('object-table')).toBeVisible()
  await expect(page.getByTestId('object-row').first()).toBeVisible()
}

test('follow-up screenshots', async ({ page }) => {
  test.skip(!process.env.BRAIAN_FINAL, 'set BRAIAN_FINAL=1 against the demo server')
  test.setTimeout(180_000)
  mkdirSync(shots, { recursive: true })

  await login(page)
  await page.keyboard.press('Control+k')
  await page.getByTestId('palette-input').fill('Articles view')
  await page.getByTestId('command-palette').locator('.hit').filter({ hasText: 'Articles view' }).filter({ hasText: 'Open view' }).first().click()
  await expect(page.getByTestId('command-palette')).toHaveCount(0)
  await page.getByTestId('object-row').filter({ hasText: 'SYN-' }).filter({ hasText: 'Article' }).first().click()
  await page.getByRole('button', { name: 'Graph' }).click()
  const provenance = page.getByTestId('attribute-provenance')
  await expect(provenance).toContainText('Number')
  await expect(provenance).toContainText('Description')
  await expect(provenance).toContainText('Colour')
  await expect(provenance).not.toContainText('Nummer')
  await expect(provenance).not.toContainText('Beschreibung')
  await expect(provenance).not.toContainText('Farbe')
  await expect(provenance).not.toContainText('namn')
  await page.screenshot({ path: `${shots}/provenance.png` })

  await page.getByRole('button', { name: 'History' }).click()
  const history = page.getByTestId('history')
  await expect(history.locator('article').first()).toBeVisible()
  await expect(history).toContainText('Created by')
  await history.getByTestId('time-travel').click()
  await expect(history.locator('.asof dt', { hasText: 'Name' })).toBeVisible()
  await expect(history.locator('.asof dt', { hasText: 'Number' })).toBeVisible()
  const labels = await history.locator('.asof dt, .diff th').allTextContents()
  expect(labels.some((label) => /^(namn|nummer|Nummer|Beschreibung|Farbe|beskrivning|farg)$/i.test(label.trim()))).toBe(false)
  await expect(history.locator('.lang-inline .meta-chip').first()).toBeVisible()
  await expect(history.getByTestId('as-of')).toBeVisible()
  await expect(history.getByTestId('as-of-when')).toBeVisible()
  await page.screenshot({ path: `${shots}/history.png` })

  await page.getByRole('button', { name: 'Detail' }).click()
  const image = page.locator('.field', { has: page.locator('label', { hasText: 'Image' }) })
  await image.evaluate((node) => node.scrollIntoView({ block: 'center', inline: 'nearest' }))
  await expect(image.getByRole('button', { name: /Choose file|Replace/ })).toBeVisible()
  await image.getByTestId('file-input').setInputFiles({
    name: 'packshot.png',
    mimeType: 'image/png',
    buffer: Buffer.from('png'),
  })
  await expect(image.getByTestId('file-chip')).toContainText('packshot.png')
  await image.screenshot({ path: `${shots}/file-chip.png` })
  await page.getByRole('button', { name: 'Discard' }).click()

  await page.setViewportSize({ width: 980, height: 800 })
    const search = await page.getByTestId('quick-filter').boundingBox()
  const count = await page.getByTestId('row-count').boundingBox()
  if (!search || !count) throw new Error('missing toolbar')
  expect(Math.abs(search.y - count.y)).toBeLessThan(8)
  await page.screenshot({ path: `${shots}/toolbar-narrow.png` })

  await page.setViewportSize({ width: 1440, height: 900 })
  const objects = page.locator('section.pane').filter({ has: page.locator('.pane-tab', { hasText: 'Products' }) })
  const box = await objects.boundingBox()
  if (!box) throw new Error('missing objects pane')
  const graphTab = page.locator('.pane-tab', { hasText: 'Graph' }).first()
  const graphBox = await graphTab.boundingBox()
  if (!graphBox) throw new Error('missing graph tab')
  await page.mouse.move(graphBox.x + graphBox.width / 2, graphBox.y + graphBox.height / 2)
  await page.mouse.down()
  await page.mouse.move(box.x + box.width * 0.5, box.y + box.height * 0.5, { steps: 12 })
  await expect(page.getByTestId('drop-center')).toBeVisible()
  await page.mouse.up()
  await expect(objects.locator('.pane-tab')).toHaveCount(2)
  const leaves = await page.getByTestId('dock').getAttribute('data-leaves')
  await page.locator('.pane-tab', { hasText: 'Products' }).first().dblclick()
  await expect(page.locator('section.pane')).toHaveCount(1)
  await expect(page.locator('.pane-tab')).toHaveCount(1)
  await expect(page.locator('.pane-tab', { hasText: 'Graph' })).toHaveCount(0)
  await page.screenshot({ path: `${shots}/maximized.png` })
  await page.locator('.pane-tab', { hasText: 'Products' }).dblclick()
  await expect(page.getByTestId('dock')).toHaveAttribute('data-zoom', '')
  await expect(page.getByTestId('dock')).toHaveAttribute('data-leaves', leaves ?? '')
  await expect(page.locator('.pane-tab', { hasText: 'Graph' })).toBeVisible()
  await page.screenshot({ path: `${shots}/restored.png` })
})
