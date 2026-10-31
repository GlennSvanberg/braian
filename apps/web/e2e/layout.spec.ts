import { setTheme } from './ui'
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
  await expect(page.locator('.pane-tab', { hasText: 'Products' })).toBeVisible()
}

async function enrich(page: Page): Promise<Workspace> {
  const listed = await page.request.get('/v1/workspaces')
  const body = (await listed.json()) as { workspaces: Array<{ id: string; name: string }> }
  const row = body.workspaces.find((item) => item.name === 'Products')
  if (!row) throw new Error('missing workspace Products')
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

test('double-click a tab maximizes that view and restores the layout', async ({ page }) => {
  test.setTimeout(60_000)
  await login(page)
  const workspace = await enrich(page)
  await clearPersonal(page, workspace.id)
  const table = workspace.views.find((view) => view.type === 'object_table')
  const graph = workspace.views.find((view) => view.title === 'Graph')
  if (!table || !graph) throw new Error('missing enrich views')
  const tablePane = page.locator(`section.pane[data-anchor="${table.id}"]`)
  await dragTab(page, 'Graph', tablePane, 'center')
  const pane = page.locator('section.pane').filter({ has: page.locator(`[data-tab="${table.id}"]`) })
  await expect(pane.locator('.pane-tab')).toHaveCount(2)
  const beforeLeaves = await leaves(page)
  const beforeViews = await pane.getAttribute('data-views')
  const beforePane = await pane.getAttribute('data-pane')
  const dock = page.getByTestId('dock')
  await pane.getByTestId('pane-maximize').hover()
  await expect(page.getByRole('tooltip')).toContainText('Maximize')
  await expect(page.getByRole('tooltip')).toContainText('Z')
  const tab = pane.locator('.pane-tab', { hasText: 'Products' })
  await tab.hover()
  await expect(page.getByRole('tooltip')).toContainText('Double-click to maximize')
  await tab.dblclick()
  await expect(dock).toHaveAttribute('data-zoom', table.id)
  await expect(page.locator('section.pane')).toHaveCount(1)
  await expect(page.locator('.pane-tab')).toHaveCount(1)
  await expect(page.locator('.pane-tab', { hasText: 'Products' })).toBeVisible()
  await expect(page.locator('.pane-tab', { hasText: 'Graph' })).toHaveCount(0)
  await page.locator('.pane-tab', { hasText: 'Products' }).dblclick()
  await expect(dock).toHaveAttribute('data-zoom', '')
  expect(await leaves(page)).toBe(beforeLeaves)
  const restored = page.locator('section.pane').filter({ has: page.locator(`[data-tab="${table.id}"]`) })
  await expect(restored).toHaveAttribute('data-views', beforeViews ?? '')
  await expect(restored).toHaveAttribute('data-pane', beforePane ?? '')
  await expect(restored.locator('.pane-tab')).toHaveCount(2)
  await expect(page.getByTestId('split-divider').first()).toBeVisible()
  await page.getByTestId('dock').click()
  const focus = await dock.getAttribute('data-focus')
  await page.keyboard.press('Control+Alt+Z')
  await expect(dock).toHaveAttribute('data-zoom', focus ?? '')
  await expect(page.locator('section.pane')).toHaveCount(1)
  await expect(page.locator('.pane-tab')).toHaveCount(1)
  await page.keyboard.press('Control+Alt+Z')
  await expect(dock).toHaveAttribute('data-zoom', '')
  expect(await leaves(page)).toBe(beforeLeaves)
  await expect(restored).toHaveAttribute('data-views', beforeViews ?? '')
  await expect(restored).toHaveAttribute('data-pane', beforePane ?? '')
  await clearPersonal(page, workspace.id)
})

type Box = { x: number; y: number; width: number; height: number }

function near(actual: number, expected: number, label: string, tolerance = 2) {
  expect(Math.abs(actual - expected), `${label}: ${actual} vs ${expected}`).toBeLessThanOrEqual(tolerance)
}

function expectPaneZone(overlay: Box, pane: Box, zone: 'left' | 'right' | 'top' | 'bottom' | 'center') {
  const inset = 4
  const expected = {
    left: { x: pane.x + inset, y: pane.y + inset, width: pane.width / 2 - inset * 2, height: pane.height - inset * 2 },
    right: { x: pane.x + pane.width / 2 + inset, y: pane.y + inset, width: pane.width / 2 - inset * 2, height: pane.height - inset * 2 },
    top: { x: pane.x + inset, y: pane.y + inset, width: pane.width - inset * 2, height: pane.height / 2 - inset * 2 },
    bottom: { x: pane.x + inset, y: pane.y + pane.height / 2 + inset, width: pane.width - inset * 2, height: pane.height / 2 - inset * 2 },
    center: { x: pane.x + inset, y: pane.y + inset, width: pane.width - inset * 2, height: pane.height - inset * 2 },
  }[zone]
  near(overlay.x, expected.x, `${zone} x`)
  near(overlay.y, expected.y, `${zone} y`)
  near(overlay.width, expected.width, `${zone} width`)
  near(overlay.height, expected.height, `${zone} height`)
}

function expectEdgeZone(overlay: Box, dock: Box, zone: 'left' | 'right' | 'top' | 'bottom') {
  const inset = 4
  const gap = 6
  const share = 0.3
  const paneW = (dock.width - gap) * share
  const paneH = (dock.height - gap) * share
  const expected = {
    left: { x: dock.x + inset, y: dock.y + inset, width: paneW - inset * 2, height: dock.height - inset * 2 },
    right: { x: dock.x + (dock.width - gap) * (1 - share) + gap + inset, y: dock.y + inset, width: paneW - inset * 2, height: dock.height - inset * 2 },
    top: { x: dock.x + inset, y: dock.y + inset, width: dock.width - inset * 2, height: paneH - inset * 2 },
    bottom: { x: dock.x + inset, y: dock.y + (dock.height - gap) * (1 - share) + gap + inset, width: dock.width - inset * 2, height: paneH - inset * 2 },
  }[zone]
  near(overlay.x, expected.x, `edge ${zone} x`)
  near(overlay.y, expected.y, `edge ${zone} y`)
  near(overlay.width, expected.width, `edge ${zone} width`)
  near(overlay.height, expected.height, `edge ${zone} height`)
}

async function holdTab(page: Page, tabName: string, point: { x: number; y: number }) {
  const tab = page.locator('.pane-tab', { hasText: tabName }).first()
  const tabBox = await tab.boundingBox()
  if (!tabBox) throw new Error(`missing ${tabName}`)
  await page.mouse.move(tabBox.x + tabBox.width / 2, tabBox.y + tabBox.height / 2)
  await page.mouse.down()
  await page.mouse.move(point.x, point.y, { steps: 12 })
  await expect(page.getByTestId('drop-overlay')).toBeVisible()
  await expect.poll(async () => page.getByTestId('drag-ghost').boundingBox()).not.toBeNull()
}

test('dock overlay matches the pane fraction and the window edge', async ({ page }) => {
  test.setTimeout(120_000)
  await login(page)
  const workspace = await enrich(page)
  await clearPersonal(page, workspace.id)
  const table = workspace.views.find((view) => view.type === 'object_table')
  if (!table) throw new Error('missing object table')
  const tablePane = page.locator(`section.pane[data-anchor="${table.id}"]`)
  await expect(page.getByTestId('object-row').first()).toBeVisible()

  const strips = await page.locator('section.pane .pane-tabs').evaluateAll((nodes) => nodes.map((node) => node.getBoundingClientRect().top))
  expect(Math.max(...strips) - Math.min(...strips)).toBeLessThanOrEqual(1)

  const before = await page.locator('section.pane').evaluateAll((nodes) =>
    nodes.map((node) => {
      const box = node.getBoundingClientRect()
      return { x: box.x, y: box.y, width: box.width, height: box.height }
    }),
  )
  await tablePane.click()
  const after = await page.locator('section.pane').evaluateAll((nodes) =>
    nodes.map((node) => {
      const box = node.getBoundingClientRect()
      return { x: box.x, y: box.y, width: box.width, height: box.height }
    }),
  )
  expect(after).toHaveLength(before.length)
  for (let index = 0; index < before.length; index += 1) {
    near(after[index].x, before[index].x, `focus x ${index}`, 1)
    near(after[index].y, before[index].y, `focus y ${index}`, 1)
    near(after[index].width, before[index].width, `focus w ${index}`, 1)
    near(after[index].height, before[index].height, `focus h ${index}`, 1)
  }

  const grid = page.locator('[data-testid="object-table"] .grid')
  const head = grid.locator('.head')
  const headBefore = await head.boundingBox()
  const gridBefore = await grid.boundingBox()
  if (!headBefore || !gridBefore) throw new Error('missing table')
  const paneBox = await tablePane.boundingBox()
  if (!paneBox) throw new Error('missing objects pane')
  near(headBefore.y, gridBefore.y, 'header at top of table', 1)
  near(gridBefore.y + gridBefore.height, paneBox.y + paneBox.height - 8, 'table fills pane', 4)
  await grid.evaluate((node) => {
    node.scrollTop = 240
  })
  const headScrolled = await head.boundingBox()
  if (!headScrolled) throw new Error('missing header')
  near(headScrolled.y, headBefore.y, 'sticky header', 1)
  await grid.evaluate((node) => {
    node.scrollTop = 0
  })
  await page.getByTestId('quick-filter').fill('qqqqqqqqqqqq')
  const status = page.locator('.table-status')
  await expect(status).toContainText('no rows')
  const statusBox = await status.boundingBox()
  const gridEmpty = await grid.boundingBox()
  const headEmpty = await head.boundingBox()
  if (!statusBox || !gridEmpty || !headEmpty) throw new Error('missing empty state')
  const bodyTop = headEmpty.y + headEmpty.height
  const bodyMid = bodyTop + (gridEmpty.y + gridEmpty.height - bodyTop) / 2
  near(statusBox.y + statusBox.height / 2, bodyMid, 'empty state center', 6)
  expect(statusBox.y).toBeGreaterThanOrEqual(gridEmpty.y - 1)
  expect(statusBox.y + statusBox.height).toBeLessThanOrEqual(gridEmpty.y + gridEmpty.height + 1)
  await page.getByTestId('quick-filter').fill('')
  await expect(page.getByTestId('object-row').first()).toBeVisible()

  const zones = ['left', 'right', 'top', 'bottom', 'center'] as const
  for (const zone of zones) {
    const paneBox = await tablePane.boundingBox()
    if (!paneBox) throw new Error('missing objects pane')
    const point = {
      left: { x: paneBox.x + paneBox.width * 0.1, y: paneBox.y + paneBox.height * 0.5 },
      right: { x: paneBox.x + paneBox.width * 0.9, y: paneBox.y + paneBox.height * 0.5 },
      top: { x: paneBox.x + paneBox.width * 0.5, y: paneBox.y + paneBox.height * 0.12 },
      bottom: { x: paneBox.x + paneBox.width * 0.5, y: paneBox.y + paneBox.height * 0.88 },
      center: { x: paneBox.x + paneBox.width * 0.5, y: paneBox.y + paneBox.height * 0.5 },
    }[zone]
    await holdTab(page, 'Graph', point)
    await expect.poll(async () => {
      const overlay = await page.getByTestId('drop-overlay').boundingBox()
      const pane = await tablePane.boundingBox()
      if (!overlay || !pane) return 999
      const inset = 4
      const expected = {
        left: { x: pane.x + inset, y: pane.y + inset, width: pane.width / 2 - inset * 2, height: pane.height - inset * 2 },
        right: { x: pane.x + pane.width / 2 + inset, y: pane.y + inset, width: pane.width / 2 - inset * 2, height: pane.height - inset * 2 },
        top: { x: pane.x + inset, y: pane.y + inset, width: pane.width - inset * 2, height: pane.height / 2 - inset * 2 },
        bottom: { x: pane.x + inset, y: pane.y + pane.height / 2 + inset, width: pane.width - inset * 2, height: pane.height / 2 - inset * 2 },
        center: { x: pane.x + inset, y: pane.y + inset, width: pane.width - inset * 2, height: pane.height - inset * 2 },
      }[zone]
      return Math.max(
        Math.abs(overlay.x - expected.x),
        Math.abs(overlay.y - expected.y),
        Math.abs(overlay.width - expected.width),
        Math.abs(overlay.height - expected.height),
      )
    }).toBeLessThanOrEqual(2)
    const overlay = await page.getByTestId('drop-overlay').boundingBox()
    const pane = await tablePane.boundingBox()
    const chip = await page.getByTestId(`drop-${zone}`).boundingBox()
    const ghost = await page.getByTestId('drag-ghost').boundingBox()
    if (!overlay || !pane || !chip || !ghost) throw new Error(`missing overlay for ${zone}`)
    expect(await page.getByTestId('drop-overlay').getAttribute('data-zone')).toBe(zone)
    expectPaneZone(overlay, pane, zone)
    near(chip.x + chip.width / 2, overlay.x + overlay.width / 2, `${zone} chip x`)
    near(chip.y + chip.height / 2, overlay.y + overlay.height / 2, `${zone} chip y`)
    near(ghost.x, point.x + 18, `${zone} ghost x`)
    near(ghost.y, point.y + 22, `${zone} ghost y`)
    const clearOfChip =
      ghost.x >= chip.x + chip.width
      || chip.x >= ghost.x + ghost.width
      || ghost.y >= chip.y + chip.height
      || chip.y >= ghost.y + ghost.height
    expect(clearOfChip, `${zone} ghost overlaps the zone label`).toBe(true)
    if (process.env.BRAIAN_DOCK_SHOTS === '1') {
      await page.screenshot({ path: `/opt/cursor/artifacts/final/dock-${zone}.png` })
    }
    await page.mouse.up()
    await expect(page.getByTestId('drop-overlay')).toHaveCount(0)
    await clearPersonal(page, workspace.id)
  }

  const edges = ['left', 'right', 'top', 'bottom'] as const
  for (const zone of edges) {
    const dock = await page.getByTestId('dock').boundingBox()
    if (!dock) throw new Error('missing dock')
    const point = {
      left: { x: dock.x + 4, y: dock.y + dock.height / 2 },
      right: { x: dock.x + dock.width - 4, y: dock.y + dock.height / 2 },
      top: { x: dock.x + dock.width / 2, y: dock.y + 4 },
      bottom: { x: dock.x + dock.width / 2, y: dock.y + dock.height - 4 },
    }[zone]
    await holdTab(page, 'Graph', point)
    await expect.poll(async () => {
      const overlay = await page.getByTestId('drop-overlay').boundingBox()
      const dock = await page.getByTestId('dock').boundingBox()
      if (!overlay || !dock) return 999
      const inset = 4
      const gap = 6
      const share = 0.3
      const paneW = (dock.width - gap) * share
      const paneH = (dock.height - gap) * share
      const expected = {
        left: { x: dock.x + inset, y: dock.y + inset, width: paneW - inset * 2, height: dock.height - inset * 2 },
        right: { x: dock.x + (dock.width - gap) * (1 - share) + gap + inset, y: dock.y + inset, width: paneW - inset * 2, height: dock.height - inset * 2 },
        top: { x: dock.x + inset, y: dock.y + inset, width: dock.width - inset * 2, height: paneH - inset * 2 },
        bottom: { x: dock.x + inset, y: dock.y + (dock.height - gap) * (1 - share) + gap + inset, width: dock.width - inset * 2, height: paneH - inset * 2 },
      }[zone]
      return Math.max(
        Math.abs(overlay.x - expected.x),
        Math.abs(overlay.y - expected.y),
        Math.abs(overlay.width - expected.width),
        Math.abs(overlay.height - expected.height),
      )
    }).toBeLessThanOrEqual(2)
    const overlay = await page.getByTestId('drop-overlay').boundingBox()
    const current = await page.getByTestId('dock').boundingBox()
    if (!overlay || !current) throw new Error(`missing edge overlay for ${zone}`)
    expect(await page.getByTestId('drop-overlay').getAttribute('data-edge')).toBe('true')
    expectEdgeZone(overlay, current, zone)
    if (process.env.BRAIAN_DOCK_SHOTS === '1') {
      await page.screenshot({ path: `/opt/cursor/artifacts/final/dock-edge-${zone}.png` })
    }
    await page.mouse.up()
    await expect(page.getByTestId('drop-overlay')).toHaveCount(0)
  }

  await clearPersonal(page, workspace.id)
  if (process.env.BRAIAN_DOCK_SHOTS === '1') {
    await page.screenshot({ path: '/opt/cursor/artifacts/final/multi-split-dark.png' })
    await setTheme(page, 'light')
    await page.screenshot({ path: '/opt/cursor/artifacts/final/multi-split-light.png' })
    await setTheme(page, 'dark')
  }
})
