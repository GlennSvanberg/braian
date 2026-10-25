import { expect, test, type APIRequestContext, type Page } from '@playwright/test'

type Workspace = {
  id: string
  name: string
  locked: boolean
  share_token?: string
  layout: unknown
  views: Array<{ id: string; type: string; title: string; pane: string; position: number; state: Record<string, unknown> }>
}

async function login(page: Page) {
  await page.goto('/')
  await page.getByTestId('email').fill(process.env.BRAIAN_DEMO_EMAIL ?? 'admin@braian.local')
  await page.getByTestId('password').fill(process.env.BRAIAN_DEMO_PASSWORD ?? 'braian-demo')
  await page.getByRole('button', { name: 'Logga in' }).click()
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

async function saveWorkspace(request: APIRequestContext, workspace: Workspace) {
  const response = await request.post('/v1/workspaces', {
    data: {
      id: workspace.id,
      name: workspace.name,
      locked: workspace.locked,
      share_token: workspace.share_token,
      layout: workspace.layout,
    },
  })
  expect(response.ok()).toBeTruthy()
}

test('column width is restored after reload', async ({ page }) => {
  await login(page)
  const before = await workspaceByName(page.request, 'Berika objekt')
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
    const current = await workspaceByName(page.request, 'Berika objekt')
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
  const before = await workspaceByName(page.request, 'Berika objekt')
  const dividers = await page.getByTestId('split-divider').count()
  try {
    const tab = page.locator('.pane-tab', { hasText: 'Graf' })
    const box = await tab.boundingBox()
    if (!box) throw new Error('missing graf tab')
    await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2)
    await page.mouse.down()
    await page.mouse.move(box.x + box.width / 2, box.y + 90, { steps: 12 })
    await expect(page.getByTestId('drop-overlay').first()).toBeVisible()
    const zone = page.getByTestId('drop-right').last()
    const zoneBox = await zone.boundingBox()
    if (!zoneBox) throw new Error('missing drop zone')
    await page.mouse.move(zoneBox.x + zoneBox.width / 2, zoneBox.y + zoneBox.height / 2, { steps: 8 })
    await page.mouse.up()
    await expect(page.getByTestId('split-divider')).toHaveCount(dividers + 1)
  } finally {
    await saveWorkspace(page.request, before)
    await page.reload()
    await expect(page.getByTestId('split-divider')).toHaveCount(dividers)
  }
})

test('drag a class onto another group and show impact', async ({ page }) => {
  test.setTimeout(60_000)
  await login(page)
  const defs = await page.request.get('/v1/definitions')
  const original = ((await defs.json()) as { structures: Array<{ key: string }> }).structures.find((item) => item.key === 'sortiment')
  try {
    await page.getByRole('button', { name: 'Datamodell' }).click()
    const tree = page.getByTestId('data-model').first()
    await expect(tree.getByTestId('group-sovsack').getByRole('button', { name: 'sovsack', exact: true })).toBeVisible()
    await tree.getByRole('button', { name: 'sovsack', exact: true }).dragTo(tree.getByTestId('group-bygg'))
    await expect(tree).toContainText('Klassen flyttades')
    await expect(tree.getByTestId('group-bygg').getByRole('button', { name: 'sovsack', exact: true })).toBeVisible()
    await tree.getByTestId('group-bygg').getByRole('button', { name: 'sovsack', exact: true }).dragTo(tree.getByTestId('group-sovsack'))
    await expect(tree.getByTestId('group-sovsack').getByRole('button', { name: 'sovsack', exact: true })).toBeVisible()
    await page.locator('.pane-tab', { hasText: 'Konsekvens' }).click()
    await page.getByTestId('impact-run').click()
    await expect(page.getByTestId('impact-result')).toContainText('Berörda objekt')
    await expect(page.getByTestId('impact-result')).toContainText('Statuskontroll')
  } finally {
    if (original) {
      await page.request.put('/v1/structures/sortiment', { data: original })
    }
  }
})
