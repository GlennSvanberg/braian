import { expect, test, type Page } from '@playwright/test'
import { demoCredentials } from './demo'

type Workspace = {
  id: string
  views: Array<{ id: string; type: string; title: string }>
}

async function login(page: Page) {
  const demo = demoCredentials()
  await page.goto('/')
  await page.getByTestId('email').fill(demo.email)
  await page.getByTestId('password').fill(demo.password)
  await page.getByRole('button', { name: 'Sign in' }).click()
  await expect(page.getByTestId('dock')).toBeVisible()
  await expect(page.locator('.pane-tab', { hasText: 'Objects' })).toBeVisible()
}

async function enrich(page: Page): Promise<Workspace> {
  const listed = await page.request.get('/v1/workspaces')
  const body = (await listed.json()) as { workspaces: Array<{ id: string; name: string }> }
  const row = body.workspaces.find((item) => item.name === 'Enrich objects')
  if (!row) throw new Error('missing workspace Enrich objects')
  const full = await page.request.get(`/v1/workspaces/${row.id}`)
  return (await full.json()) as Workspace
}

async function clearPersonal(page: Page, workspaceId: string) {
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

async function leaves(page: Page) {
  return page.getByTestId('dock').getAttribute('data-leaves')
}

async function dragTab(
  page: Page,
  tabName: string,
  pane: ReturnType<Page['locator']>,
  zone: 'left' | 'right' | 'top' | 'bottom' | 'center',
) {
  const tab = page.locator('.pane-tab', { hasText: tabName }).first()
  const tabBox = await tab.boundingBox()
  const paneBox = await pane.boundingBox()
  if (!tabBox || !paneBox) throw new Error(`missing ${tabName}`)
  const point = {
    left: { x: paneBox.x + paneBox.width * 0.1, y: paneBox.y + paneBox.height * 0.5 },
    right: { x: paneBox.x + paneBox.width * 0.9, y: paneBox.y + paneBox.height * 0.5 },
    top: { x: paneBox.x + paneBox.width * 0.5, y: paneBox.y + paneBox.height * 0.12 },
    bottom: { x: paneBox.x + paneBox.width * 0.5, y: paneBox.y + paneBox.height * 0.88 },
    center: { x: paneBox.x + paneBox.width * 0.5, y: paneBox.y + paneBox.height * 0.5 },
  }[zone]
  await page.mouse.move(tabBox.x + tabBox.width / 2, tabBox.y + tabBox.height / 2)
  await page.mouse.down()
  await page.mouse.move(point.x, point.y, { steps: 16 })
  await expect(page.getByTestId(`drop-${zone}`)).toBeVisible()
  await page.mouse.up()
}

test('divider drag, dock zones, tab order, maximize, reload and keyboard', async ({ page }) => {
  test.setTimeout(120_000)
  await login(page)
  const workspace = await enrich(page)
  await clearPersonal(page, workspace.id)
  const table = workspace.views.find((view) => view.type === 'object_table')
  const detail = workspace.views.find((view) => view.title === 'Detail')
  const graph = workspace.views.find((view) => view.title === 'Graph')
  if (!table || !detail || !graph) throw new Error('missing enrich views')

  const beforeFlex = await page.locator('.split-cell').first().evaluate((node) => (node as HTMLElement).style.flexBasis)
  const divider = page.getByTestId('split-divider').first()
  const box = await divider.boundingBox()
  if (!box) throw new Error('missing divider')
  await page.mouse.move(box.x + box.width / 2, box.y + 30)
  await page.mouse.down()
  await page.mouse.move(box.x + 140, box.y + 30, { steps: 8 })
  await page.mouse.up()
  await expect.poll(async () => page.locator('.split-cell').first().evaluate((node) => (node as HTMLElement).style.flexBasis)).not.toBe(beforeFlex)

  const start = await leaves(page)
  const tablePane = page.locator(`section.pane[data-anchor="${table.id}"]`)
  for (const zone of ['left', 'right', 'top', 'bottom', 'center'] as const) {
    await clearPersonal(page, workspace.id)
    await dragTab(page, 'Graph', tablePane, zone)
    await expect.poll(() => leaves(page)).not.toBe(start)
    if (zone === 'center') {
      expect(await leaves(page)).toMatch(new RegExp(`${table.id}\\+${graph.id}|${graph.id}\\+${table.id}`))
    }
  }

  await clearPersonal(page, workspace.id)
  const detailTab = page.locator('.pane-tab', { hasText: 'Detail' }).first()
  const graphTab = page.locator('.pane-tab', { hasText: 'Graph' }).first()
  const detailBox = await detailTab.boundingBox()
  const graphBox = await graphTab.boundingBox()
  if (!detailBox || !graphBox) throw new Error('missing tabs')
  await page.mouse.move(graphBox.x + graphBox.width / 2, graphBox.y + graphBox.height / 2)
  await page.mouse.down()
  await page.mouse.move(detailBox.x + 6, detailBox.y + detailBox.height / 2, { steps: 10 })
  await page.mouse.up()
  const graphPane = page.locator('section.pane').filter({ has: page.locator(`[data-tab="${graph.id}"]`) })
  await expect(graphPane).toHaveAttribute('data-views', new RegExp(`^${graph.id},${detail.id}`))

  await clearPersonal(page, workspace.id)
  const focus = await page.getByTestId('dock').getAttribute('data-focus')
  await page.locator(`[data-pane="${focus}"]`).getByTestId('pane-maximize').click()
  await expect(page.getByTestId('dock')).toHaveAttribute('data-zoom', focus ?? '')
  await expect(page.locator('section.pane')).toHaveCount(1)
  await page.getByTestId('pane-maximize').click()
  await expect(page.getByTestId('dock')).toHaveAttribute('data-zoom', '')
  await expect(page.getByTestId('split-divider').first()).toBeVisible()

  await clearPersonal(page, workspace.id)
  await dragTab(page, 'Graph', tablePane, 'right')
  const docked = await leaves(page)
  await expect.poll(async () => {
    const response = await page.request.get(`/v1/workspaces/${workspace.id}/state?environment=default`)
    const body = (await response.json()) as { document?: { layout?: unknown } }
    return body.document?.layout ? 'saved' : 'missing'
  }).toBe('saved')
  await page.reload()
  await expect(page.getByTestId('dock')).toHaveAttribute('data-leaves', docked ?? '')

  await clearPersonal(page, workspace.id)
  const beforeMove = await leaves(page)
  await page.getByTestId('dock').click()
  await page.keyboard.press('Control+Alt+ArrowRight')
  await expect.poll(() => leaves(page)).not.toBe(beforeMove)

  await clearPersonal(page, workspace.id)
  const stable = await leaves(page)
  const tab = page.locator('.pane-tab', { hasText: 'Graph' }).first()
  const tabBox = await tab.boundingBox()
  const paneBox = await tablePane.boundingBox()
  if (!tabBox || !paneBox) throw new Error('missing drag target')
  await page.mouse.move(tabBox.x + tabBox.width / 2, tabBox.y + tabBox.height / 2)
  await page.mouse.down()
  await page.mouse.move(paneBox.x + paneBox.width * 0.5, paneBox.y + paneBox.height * 0.5, { steps: 10 })
  await expect(page.getByTestId('drag-ghost')).toBeVisible()
  await page.keyboard.press('Escape')
  await page.mouse.up()
  await expect(page.getByTestId('drag-ghost')).toHaveCount(0)
  expect(await leaves(page)).toBe(stable)

  await clearPersonal(page, workspace.id)
})
