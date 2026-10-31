import { expect, test, type Page, type Request } from '@playwright/test'
import { demoCredentials } from './demo'

async function login(page: Page) {
  await page.addInitScript(() => {
    localStorage.setItem('braian.theme', JSON.stringify('dark'))
  })
  const demo = demoCredentials()
  await page.goto('/')
  await page.getByTestId('email').fill(demo.email)
  await page.getByTestId('password').fill(demo.password)
  await page.getByRole('button', { name: 'Sign in' }).click()
  await expect(page.getByTestId('dock')).toBeVisible()
}

/** Only count writes that touch this workspace (body id / workspace_id or URL). */
function isWorkspaceWrite(request: Request, workspaceId: string) {
  if (!['POST', 'PUT', 'PATCH', 'DELETE'].includes(request.method())) return false
  const url = request.url()
  if (!/\/v1\/(workspaces|views)(\/|$|\?)/.test(url)) return false
  if (url.includes(workspaceId)) return true
  try {
    const data = request.postDataJSON() as { id?: string; workspace_id?: string } | null
    if (data && (data.id === workspaceId || data.workspace_id === workspaceId)) return true
  } catch {
    /* ignore non-JSON bodies */
  }
  return false
}

/**
 * Reconstructs the demo "Data model 8" failure: layout references view ids that
 * were never persisted (create failed after layout write), so tabs paint as UUIDs.
 *
 * Name must NOT contain the substring "Data model" — Playwright's getByRole name
 * match is substring-based and would break other e2e specs that open that tab.
 * Detection still works via the Structures `data_model` view.
 */
async function seedDanglingModelWorkspace(page: Page, name: string) {
  const workspaceId = crypto.randomUUID()
  const structuresId = crypto.randomUUID()
  const ghostLeft = crypto.randomUUID()
  const ghostMid = crypto.randomUUID()
  const ghostRight = crypto.randomUUID()
  const layout = {
    type: 'split',
    dir: 'horizontal',
    ratios: [0.22, 0.78],
    children: [
      { type: 'tabs', views: [structuresId, ghostLeft], active: 0 },
      {
        type: 'split',
        dir: 'horizontal',
        ratios: [0.55, 0.45],
        children: [
          { type: 'tabs', views: [ghostMid], active: 0 },
          { type: 'tabs', views: [ghostRight], active: 0 },
        ],
      },
    ],
  }
  const created = await page.request.post('/v1/workspaces', {
    data: {
      id: workspaceId,
      name,
      locked: false,
      share_token: `dangling-model-${workspaceId.slice(0, 8)}`,
      layout,
    },
  })
  expect(created.ok(), `create workspace ${created.status()}`).toBeTruthy()
  const structures = await page.request.post('/v1/views', {
    data: {
      id: structuresId,
      workspace_id: workspaceId,
      type: 'data_model',
      title: 'Structures',
      pane: 'main',
      position: 0,
      state: { mode: 'tree' },
    },
  })
  expect(structures.ok(), `create Structures ${structures.status()}`).toBeTruthy()
  // Intentionally do not create ghostLeft/Mid/Right — that is the bug.
  const listed = await page.request.get('/v1/workspaces')
  expect(listed.ok()).toBeTruthy()
  const body = (await listed.json()) as { workspaces: Array<{ id: string; name: string }> }
  expect(body.workspaces.some((row) => row.id === workspaceId)).toBe(true)
  return { workspaceId, ghostLeft, ghostMid, ghostRight }
}

async function openWorkspace(page: Page, name: string) {
  await page.evaluate(() => sessionStorage.setItem('braian.layout.suppressFlush', '1'))
  await page.reload()
  await expect(page.getByTestId('dock')).toBeVisible()
  await page.evaluate(() => sessionStorage.removeItem('braian.layout.suppressFlush'))
  // Tab buttons append a shortcut index (<span class="tab-index">), so the
  // accessible name is "Name N". Match the name as a substring (same as legacy).
  const escaped = name.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
  const tab = page.locator('nav.tabs').getByRole('button', { name: new RegExp(escaped) })
  await expect(tab).toBeVisible({ timeout: 30_000 })
  await tab.click()
}

async function waitForWritesToSettle(writes: string[], quietMs = 1200) {
  let last = writes.length
  let quietSince = Date.now()
  await expect
    .poll(
      () => {
        if (writes.length !== last) {
          last = writes.length
          quietSince = Date.now()
        }
        return Date.now() - quietSince >= quietMs ? 'quiet' : 'busy'
      },
      { timeout: 60_000 },
    )
    .toBe('quiet')
}

async function watchUnavailableDuringLoad(page: Page) {
  // Monitor the workspace main area for the whole first heal — not only the end state.
  await page.evaluate(() => {
    const w = window as unknown as { __braianHealHits?: string[]; __braianHealObs?: MutationObserver }
    w.__braianHealHits = []
    const scan = () => {
      const root = document.querySelector('main.workspace')
      if (!root) return
      const text = root.textContent ?? ''
      if (text.includes('Unavailable') || text.includes('Nothing to show')) {
        w.__braianHealHits!.push(text.replace(/\s+/g, ' ').trim().slice(0, 240))
      }
    }
    w.__braianHealObs?.disconnect()
    const obs = new MutationObserver(scan)
    obs.observe(document.documentElement, { subtree: true, childList: true, characterData: true })
    w.__braianHealObs = obs
    scan()
  })
}

async function stopUnavailableWatch(page: Page) {
  return page.evaluate(() => {
    const w = window as unknown as { __braianHealHits?: string[]; __braianHealObs?: MutationObserver }
    w.__braianHealObs?.disconnect()
    const hits = w.__braianHealHits ?? []
    w.__braianHealHits = []
    return hits
  })
}

test('dangling Data model panes heal to the standard template without UUID tabs', async ({ page }) => {
  test.setTimeout(180_000)
  await login(page)
  // Avoid the substring "Data model" so other specs' getByRole({ name: 'Data model' }) stay unique.
  const name = `Heal dangling panes ${Date.now()}`
  const { workspaceId, ghostLeft, ghostMid, ghostRight } = await seedDanglingModelWorkspace(page, name)

  const firstWrites: string[] = []
  const onFirst = (request: Request) => {
    if (isWorkspaceWrite(request, workspaceId)) {
      firstWrites.push(`${request.method()} ${new URL(request.url()).pathname}`)
    }
  }
  page.on('request', onFirst)
  await watchUnavailableDuringLoad(page)
  await openWorkspace(page, name)

  const paneTabs = page.locator('.pane-tab')
  await expect(paneTabs.filter({ hasText: 'Structures' })).toBeVisible({ timeout: 30_000 })
  await expect(paneTabs.filter({ hasText: 'Object types' })).toBeVisible()
  await expect(paneTabs.filter({ hasText: 'Link types' })).toBeVisible()
  await expect(paneTabs.filter({ hasText: 'Value lists' })).toBeVisible()
  await expect(paneTabs.filter({ hasText: 'Units' })).toBeVisible()
  await expect(paneTabs.filter({ hasText: 'Type attributes' })).toBeVisible()
  await expect(paneTabs.filter({ hasText: 'Attribute definitions' })).toBeVisible()
  await expect(paneTabs.filter({ hasText: 'List values' })).toBeVisible()
  for (const ghost of [ghostLeft, ghostMid, ghostRight]) {
    await expect(paneTabs.filter({ hasText: ghost })).toHaveCount(0)
  }
  await expect(paneTabs.filter({ hasText: /[0-9a-f]{8}-[0-9a-f]{4}-/i })).toHaveCount(0)
  await expect(paneTabs.filter({ hasText: 'Unavailable' })).toHaveCount(0)
  await expect(page.locator('main.workspace')).not.toContainText('Nothing to show')
  const healHits = await stopUnavailableWatch(page)
  expect(healHits, `Unavailable/Nothing to show during first heal: ${healHits.join(' | ')}`).toEqual([])
  await expect.poll(() => firstWrites.length).toBeGreaterThan(0)
  await waitForWritesToSettle(firstWrites)
  page.off('request', onFirst)

  const secondWrites: string[] = []
  const onSecond = (request: Request) => {
    if (isWorkspaceWrite(request, workspaceId)) {
      secondWrites.push(`${request.method()} ${new URL(request.url()).pathname}`)
    }
  }
  page.on('request', onSecond)
  await openWorkspace(page, name)
  await expect(paneTabs.filter({ hasText: 'Structures' })).toBeVisible()
  await expect(paneTabs.filter({ hasText: 'Object types' })).toHaveCount(1)
  await expect(paneTabs.filter({ hasText: 'Unavailable' })).toHaveCount(0)
  await expect(page.locator('main.workspace')).not.toContainText('Nothing to show')
  await waitForWritesToSettle(secondWrites, 1500)
  expect(secondWrites, `unexpected writes on reload: ${secondWrites.join(', ')}`).toEqual([])
})
