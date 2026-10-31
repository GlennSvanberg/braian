import { setTheme } from './ui'
import { expect, test, type Page } from '@playwright/test'
import { mkdirSync } from 'node:fs'
import { demoCredentials } from './demo'

const shots = '/opt/cursor/artifacts/final'
mkdirSync(shots, { recursive: true })

async function login(page: Page) {
  const demo = demoCredentials()
  await page.goto('/')
  await page.getByTestId('email').fill(demo.email)
  await page.getByTestId('password').fill(demo.password)
  await page.getByRole('button', { name: 'Sign in' }).click()
  await expect(page.getByTestId('object-table')).toBeVisible()
}

test('final data-model and polish shots', async ({ page }) => {
  test.skip(!process.env.BRAIAN_FINAL, 'set BRAIAN_FINAL=1 against the demo server')
  test.setTimeout(180_000)
  await login(page)

  await page.getByRole('button', { name: 'Data model' }).click()
  const tree = page.getByTestId('data-model').first()
  await tree.getByTestId('group-dairy').locator('.twist').click()
  await expect(tree.getByTestId('group-yoghurt')).toBeVisible()
  await page.screenshot({ path: `${shots}/data-model-workspace.png` })

  await tree.getByTestId('group-yoghurt').locator('.model-node').click()
  await expect(page.getByTestId('attribute-table')).toContainText('Fat content')
  await expect(page.getByTestId('attribute-table')).toContainText('from Dairy')
  await page.screenshot({ path: `${shots}/class-card.png` })
  await tree.getByTestId('group-cheese').locator('.model-node').click()
  await expect(page.getByTestId('attribute-table')).toContainText('Fat content')

  const drafted = await page.request.post('http://127.0.0.1:8090/v1/operations/model.draft', {
    headers: { authorization: 'Bearer dev-agent' },
    data: { text: 'add attribute voltage to Drill' },
  })
  expect(drafted.ok()).toBeTruthy()

  await page.locator('nav.tabs').getByRole('button', { name: /^Review/ }).click()
  await page.getByRole('button', { name: 'add attribute voltage to Drill', exact: true }).first().click()
  await expect(page.getByTestId('proposal-tree')).toContainText('Drill')
  await page.screenshot({ path: `${shots}/agent-proposal-tree.png` })
  await page.getByRole('button', { name: 'Review the change before it is saved.' }).click()
  await expect(page.getByTestId('consequence')).toContainText('agent')
  await page.screenshot({ path: `${shots}/agent-consequence.png` })
  await page.getByRole('button', { name: 'Cancel' }).click()

  await page.locator('nav.tabs').getByRole('button', { name: 'Products' }).click()
  await page.getByTestId('object-row').first().click()
  await expect(page.getByTestId('object-detail')).toBeVisible()
  await page.screenshot({ path: `${shots}/detail-dark.png` })
  await setTheme(page, 'light')
  await page.screenshot({ path: `${shots}/detail-light.png` })
  await setTheme(page, 'dark')

  await page.getByRole('button', { name: 'Graph' }).click()
  await expect(page.locator('canvas[data-testid=graph]')).toBeVisible()
  await page.getByRole('button', { name: 'Open full size' }).click()
  await expect(page.getByTestId('graph-full')).toBeVisible()
  await page.screenshot({ path: `${shots}/graph-full.png` })
  await page.getByRole('button', { name: 'Close full size' }).click()

})

test('agent tree shot', async ({ page }) => {
  test.skip(!process.env.BRAIAN_FINAL, 'set BRAIAN_FINAL=1 against the demo server')
  await login(page)
  const drafted = await page.request.post('http://127.0.0.1:8090/v1/operations/model.draft', {
    headers: { authorization: 'Bearer dev-agent' },
    data: { text: 'add attribute voltage to Drill' },
  })
  expect(drafted.ok()).toBeTruthy()
  await page.locator('nav.tabs').getByRole('button', { name: /^Review/ }).click()
  await page.getByRole('button', { name: 'add attribute voltage to Drill', exact: true }).first().click()
  await expect(page.getByTestId('proposal-tree')).toContainText('Add group Drill')
  await page.screenshot({ path: `${shots}/agent-proposal-tree.png` })
  await page.getByRole('button', { name: 'Review the change before it is saved.' }).click()
  await expect(page.getByTestId('model-diff')).toContainText('Drill')
  await page.screenshot({ path: `${shots}/agent-consequence.png` })
  await page.getByTestId('consequence').getByRole('button', { name: 'Cancel' }).click()
})

test('ctrl+k shot', async ({ page }) => {
  test.skip(!process.env.BRAIAN_FINAL, 'set BRAIAN_FINAL=1 against the demo server')
  await login(page)
  await page.keyboard.press('Control+k')
  await expect(page.getByTestId('command-palette')).toBeVisible()
  await page.getByTestId('palette-input').fill('add attribute weight (kg) to Drill')
  await expect(page.locator('.hit', { hasText: 'Proposal · review before it is saved' })).toBeVisible()
  await page.screenshot({ path: `${shots}/ctrl-k.png` })
  await page.keyboard.press('Escape')
})
