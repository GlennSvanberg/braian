import { expect, test, type APIRequestContext, type Page } from '@playwright/test'
import { demoCredentials } from './demo'

type Workspace = {
  id: string
  name: string
  locked: boolean
  share_token?: string
  layout: unknown
  views: Array<{ id: string; type: string; title: string; pane: string; position: number; state: Record<string, unknown> }>
}

async function login(page: Page) {
  const demo = demoCredentials()
  await page.goto('/')
  await page.getByTestId('email').fill(demo.email)
  await page.getByTestId('password').fill(demo.password)
  await page.getByRole('button', { name: 'Sign in' }).click()
  await expect(page.getByTestId('object-row').first()).toBeVisible()
}

async function workspaceByName(request: APIRequestContext, name: string) {
  const listed = await request.get('/v1/workspaces')
  const body = (await listed.json()) as { workspaces: Array<{ id: string; name: string }> }
  const row = body.workspaces.find((item) => item.name === name)
  if (!row) throw new Error(`missing workspace ${name}`)
  const full = await request.get(`/v1/workspaces/${row.id}`)
  return (await full.json()) as Workspace
}

async function clearPersonalLayout(page: Page, workspaceId: string) {
  await page.evaluate(() => sessionStorage.setItem('braian.layout.suppressFlush', '1'))
  const me = await page.request.get('/v1/me')
  const userId = ((await me.json()) as { user_id?: string }).user_id
  if (userId) {
    await page.evaluate((key) => localStorage.removeItem(key), `braian.layout.${userId}.default.${workspaceId}`)
  }
  const deleted = await page.request.delete(`/v1/workspaces/${workspaceId}/state?environment=default`)
  expect(deleted.ok() || deleted.status() === 204).toBeTruthy()
  await page.reload()
  await expect(page.getByTestId('dock')).toBeVisible()
  await page.evaluate(() => sessionStorage.removeItem('braian.layout.suppressFlush'))
}

test('column width is restored after reload', async ({ page }) => {
  await login(page)
  const before = await workspaceByName(page.request, 'Enrich')
  const view = before.views.find((item) => item.type === 'object_table')
  if (!view) throw new Error('missing object table')
  const handle = page.getByTestId('resize-name')
  const box = await handle.boundingBox()
  if (!box) throw new Error('missing resize handle')
  await page.mouse.move(box.x + 2, box.y + box.height / 2)
  await page.mouse.down()
  await page.mouse.move(box.x + 70, box.y + box.height / 2)
  await page.mouse.up()
  await expect.poll(async () => {
    const current = await workspaceByName(page.request, 'Enrich')
    const table = current.views.find((item) => item.id === view.id)?.state.table as { sizes?: { name?: number } } | undefined
    return table?.sizes?.name ?? 0
  }).toBeGreaterThan(200)
  await page.reload()
  await expect(page.getByTestId('col-name')).toBeVisible()
  const restored = await page.getByTestId('col-name').evaluate((node) => (node as HTMLElement).style.width)
  expect(Number.parseFloat(restored)).toBeGreaterThan(200)
  await page.request.post('/v1/views', {
    data: {
      id: view.id,
      workspace_id: before.id,
      type: view.type,
      title: view.title,
      pane: view.pane,
      position: view.position,
      state: view.state,
    },
  })
})

test('dragging a pane tab docks it and can be restored', async ({ page }) => {
  await login(page)
  const before = await workspaceByName(page.request, 'Enrich')
  await clearPersonalLayout(page, before.id)
  const dividers = await page.getByTestId('split-divider').count()
  try {
    const tab = page.locator('.pane-tab', { hasText: 'Graph' })
    const pane = page.locator('section.pane').filter({ has: tab })
    const tabBox = await tab.boundingBox()
    const paneBox = await pane.boundingBox()
    if (!tabBox || !paneBox) throw new Error('missing graf tab')
    await page.mouse.move(tabBox.x + tabBox.width / 2, tabBox.y + tabBox.height / 2)
    await page.mouse.down()
    await page.mouse.move(paneBox.x + paneBox.width * 0.9, paneBox.y + paneBox.height * 0.5, { steps: 12 })
    await expect(page.getByTestId('drop-right')).toBeVisible()
    await page.mouse.up()
    await expect(page.getByTestId('split-divider')).toHaveCount(dividers + 1)
  } finally {
    await clearPersonalLayout(page, before.id)
    await expect(page.getByTestId('split-divider')).toHaveCount(dividers)
  }
})

test('a group shows attributes inherited from its ancestors', async ({ page }) => {
  test.setTimeout(60_000)
  await login(page)
  await page.getByRole('button', { name: 'Data model' }).click()
  const tree = page.getByTestId('data-model').first()
  await tree.getByTestId('group-sovsack').getByRole('button').click()
  const table = page.getByTestId('attribute-table')
  await expect(table).toContainText('Weight')
  await expect(table).toContainText('from Outdoor')
  await expect(table.getByTestId('attr-add')).toBeVisible()
})
