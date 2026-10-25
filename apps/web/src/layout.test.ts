import { describe, expect, it } from 'vitest'
import {
  LayoutError,
  activate,
  applyLayoutCommand,
  closeView,
  dockEdge,
  ensureInLayout,
  focusDirection,
  insert,
  leaves,
  migrateLayout,
  moveDir,
  moveView,
  neighbour,
  normalize,
  placeView,
  remove,
  resetLayout,
  resize,
  setDivider,
  setTabIndex,
  splitPair,
  swap,
  tabGroup,
  views,
  zoneFromPoint,
  zoomView,
  type LayoutDoc,
  type Node,
} from './layout'

const tabs = (ids: string[], active = 0) => tabGroup(ids, active)

function doc(layout: Node, extra: Partial<LayoutDoc> = {}): LayoutDoc {
  return { id: 'ws', title: 'Workspace', layout, focus: views(layout)[0], ...extra }
}

const catalog = [
  { id: 'a', title: 'Assortment', type: 'structure_tree' },
  { id: 'b', title: 'Objects', type: 'object_table' },
  { id: 'c', title: 'Detail', type: 'object_detail' },
  { id: 'd', title: 'Graph', type: 'graph' },
]

describe('migrateLayout', () => {
  it('turns a binary row and column tree into ratios that sum to 1', () => {
    const next = migrateLayout({
      direction: 'row',
      ratio: 0.22,
      first: { tabs: ['a'], active: 'a' },
      second: {
        direction: 'column',
        ratio: 0.4,
        first: { tabs: ['b', 'c'], active: 'c' },
        second: { tabs: ['d'] },
      },
    })
    expect(next).toEqual(
      splitPair(
        'horizontal',
        0.22,
        tabs(['a']),
        splitPair('vertical', 0.4, tabs(['b', 'c'], 1), tabs(['d'])),
      ),
    )
    expect(views(next)).toEqual(['a', 'b', 'c', 'd'])
  })

  it('is idempotent on the v4 shape', () => {
    const node = splitPair('horizontal', 0.3, tabs(['a']), tabs(['b']))
    expect(migrateLayout(node)).toEqual(node)
  })
})

describe('views and leaves', () => {
  const tree = splitPair('horizontal', 0.25, tabs(['a']), splitPair('vertical', 0.5, tabs(['b']), tabs(['c'])))

  it('lists views depth-first', () => {
    expect(views(tree)).toEqual(['a', 'b', 'c'])
  })

  it('places leaves in the unit square', () => {
    expect(leaves(tree).map((leaf) => ({ views: leaf.views, rect: leaf.rect }))).toEqual([
      { views: ['a'], rect: { x: 0, y: 0, w: 0.25, h: 1 } },
      { views: ['b'], rect: { x: 0.25, y: 0, w: 0.75, h: 0.5 } },
      { views: ['c'], rect: { x: 0.25, y: 0.5, w: 0.75, h: 0.5 } },
    ])
  })
})

describe('normalize', () => {
  it('drops an empty tab group, rescales the remaining ratios, and collapses a single child', () => {
    const node: Node = {
      type: 'split',
      dir: 'horizontal',
      ratios: [0.2, 0.5, 0.3],
      children: [
        tabs([]),
        {
          type: 'split',
          dir: 'vertical',
          ratios: [1],
          children: [tabs(['a'])],
        },
        tabs(['b']),
      ],
    }
    expect(normalize(node)).toEqual(splitPair('horizontal', 0.5 / 0.8, tabs(['a']), tabs(['b'])))
  })
})

describe('remove and activate', () => {
  it('removes a view and collapses the pane that held it', () => {
    const tree = splitPair('horizontal', 0.4, tabs(['a', 'b'], 1), tabs(['c']))
    expect(remove(tree, 'c')).toEqual(tabs(['a', 'b'], 1))
  })

  it('keeps the active view when an earlier tab closes', () => {
    expect(remove(tabs(['a', 'b', 'c'], 2), 'a')).toEqual(tabs(['b', 'c'], 1))
  })

  it('activates a view wherever it sits', () => {
    const tree = splitPair('horizontal', 0.5, tabs(['a']), tabs(['b', 'c']))
    expect(activate(tree, 'c')).toEqual(splitPair('horizontal', 0.5, tabs(['a']), tabs(['b', 'c'], 1)))
  })
})

describe('insert', () => {
  it('adds a centre drop as the active tab', () => {
    expect(insert(tabs(['a', 'b']), 'a', 'c', 'center')).toEqual(tabs(['a', 'b', 'c'], 2))
  })

  it('puts a left or top pane first at 0.5', () => {
    expect(insert(tabs(['a']), 'a', 'b', 'left')).toEqual(splitPair('horizontal', 0.5, tabs(['b']), tabs(['a'])))
    expect(insert(tabs(['a']), 'a', 'b', 'top')).toEqual(splitPair('vertical', 0.5, tabs(['b']), tabs(['a'])))
    expect(insert(tabs(['a']), 'a', 'b', 'right')).toEqual(splitPair('horizontal', 0.5, tabs(['a']), tabs(['b'])))
    expect(insert(tabs(['a']), 'a', 'b', 'bottom')).toEqual(splitPair('vertical', 0.5, tabs(['a']), tabs(['b'])))
  })

  it('does not flatten a same-direction split into its parent', () => {
    const tree = splitPair('horizontal', 0.3, tabs(['a']), tabs(['b']))
    const next = insert(tree, 'b', 'c', 'right')
    expect(next).toEqual({
      type: 'split',
      dir: 'horizontal',
      ratios: [0.3, 0.7],
      children: [tabs(['a']), splitPair('horizontal', 0.5, tabs(['b']), tabs(['c']))],
    })
  })
})

describe('moveView', () => {
  it('is a no-op when the view is dropped onto its own tab group', () => {
    const tree = tabs(['a', 'b'], 0)
    expect(moveView(tree, 'b', 'a', 'center')).toBe(tree)
  })

  it('moves a view to an edge of another pane', () => {
    const tree = splitPair('horizontal', 0.5, tabs(['a']), tabs(['b']))
    expect(moveView(tree, 'b', 'a', 'top')).toEqual(splitPair('vertical', 0.5, tabs(['b']), tabs(['a'])))
  })

  it('errors when removing the view also removes the target', () => {
    expect(() => moveView(tabs(['a']), 'a', 'a', 'right')).toThrow(LayoutError)
    try {
      moveView(tabs(['a']), 'a', 'a', 'right')
    } catch (error) {
      expect(error).toMatchObject({ code: 'target-removed' })
    }
  })
})

describe('dockEdge', () => {
  const tree = splitPair('horizontal', 0.5, tabs(['a']), tabs(['b']))

  it('wraps the tree and clamps the share', () => {
    expect(dockEdge(tree, 'c', 'left')).toEqual(splitPair('horizontal', 0.3, tabs(['c']), tree))
    expect(dockEdge(tree, 'c', 'right', 0.99)).toEqual(splitPair('horizontal', 0.1, tree, tabs(['c'])))
    expect(dockEdge(tree, 'c', 'top', 0.01)).toEqual(splitPair('vertical', 0.1, tabs(['c']), tree))
    expect(dockEdge(tree, 'c', 'bottom', 0.4)).toEqual(splitPair('vertical', 0.6, tree, tabs(['c'])))
  })

  it('lifts an open view out of its old pane before wrapping', () => {
    expect(dockEdge(tree, 'b', 'right', 0.3)).toEqual(splitPair('horizontal', 0.7, tabs(['a']), tabs(['b'])))
  })
})

describe('setDivider and resize', () => {
  it('moves one boundary and keeps each neighbour at least 0.04', () => {
    const tree: Node = { type: 'split', dir: 'horizontal', ratios: [0.2, 0.5, 0.3], children: [tabs(['a']), tabs(['b']), tabs(['c'])] }
    expect(setDivider(tree, [], 0, 0.01)).toMatchObject({ ratios: [0.04, 0.66, 0.3] })
    const moved = setDivider(tree, [], 1, 0.8)
    expect(moved.type === 'split' && moved.ratios[0]).toBeCloseTo(0.2)
    expect(moved.type === 'split' && moved.ratios[1]).toBeCloseTo(0.6)
    expect(moved.type === 'split' && moved.ratios[2]).toBeCloseTo(0.2)
  })

  it('resizes the nearest split of that direction and stops at 0.05', () => {
    const tree = splitPair('vertical', 0.4, splitPair('horizontal', 0.5, tabs(['a']), tabs(['b'])), tabs(['c']))
    const grown = resize(tree, 'a', 'horizontal', 0.9)
    expect(grown).toEqual(splitPair('vertical', 0.4, splitPair('horizontal', 0.95, tabs(['a']), tabs(['b'])), tabs(['c'])))
    const vertical = resize(tree, 'a', 'vertical', 0.2)
    expect(vertical).toEqual(splitPair('vertical', 0.6, splitPair('horizontal', 0.5, tabs(['a']), tabs(['b'])), tabs(['c'])))
    const floored = resize(tree, 'a', 'horizontal', -1)
    expect(floored).toEqual(splitPair('vertical', 0.4, splitPair('horizontal', 0.05, tabs(['a']), tabs(['b'])), tabs(['c'])))
  })
})

describe('neighbour and moveDir', () => {
  const tree = splitPair('horizontal', 0.5, tabs(['a']), splitPair('vertical', 0.5, tabs(['b']), tabs(['c'])))

  it('picks the fully-on-that-side pane with the nearest centre', () => {
    expect(neighbour(tree, 'b', 'left')).toBe('a')
    expect(neighbour(tree, 'b', 'bottom')).toBe('c')
    expect(neighbour(tree, 'a', 'right')).toBe('b')
    expect(neighbour(tree, 'a', 'left')).toBeNull()
  })

  it('swaps when the view is alone and has a neighbour', () => {
    expect(moveDir(splitPair('horizontal', 0.4, tabs(['a']), tabs(['b'])), 'a', 'right')).toEqual(
      splitPair('horizontal', 0.4, tabs(['b']), tabs(['a'])),
    )
  })

  it('joins the neighbour as a tab when the pane is shared', () => {
    const shared = splitPair('horizontal', 0.5, tabs(['a', 'd'], 0), tabs(['b']))
    expect(moveDir(shared, 'a', 'right')).toEqual(splitPair('horizontal', 0.5, tabs(['d']), tabs(['b', 'a'], 1)))
  })

  it('docks at 0.5 when there is no neighbour', () => {
    expect(moveDir(tabs(['a', 'b']), 'a', 'right')).toEqual(splitPair('horizontal', 0.5, tabs(['b']), tabs(['a'])))
  })
})

describe('swap and setTabIndex', () => {
  it('exchanges two views and keeps each pane on the view that stayed', () => {
    const tree = splitPair('horizontal', 0.5, tabs(['a', 'b'], 0), tabs(['c']))
    expect(swap(tree, 'a', 'c')).toEqual(splitPair('horizontal', 0.5, tabs(['c', 'b'], 0), tabs(['a'])))
  })

  it('moves a tab to an index and clamps it', () => {
    expect(setTabIndex(tabs(['a', 'b', 'c'], 2), 'c', 0)).toEqual(tabs(['c', 'a', 'b'], 0))
    expect(setTabIndex(tabs(['a', 'b'], 1), 'b', 9)).toEqual(tabs(['a', 'b'], 1))
  })
})

describe('placeView and ensureInLayout', () => {
  it('docks an edge place at share 0.3', () => {
    const next = placeView(doc(tabs(['a'])), 'b', { dock: 'edge', zone: 'left' })
    expect(next.layout).toEqual(splitPair('horizontal', 0.3, tabs(['b']), tabs(['a'])))
    expect(next.focus).toBe('b')
  })

  it('inserts to the right by default, moves when the view is open, then applies tabIndex and activates', () => {
    const inserted = placeView(doc(tabs(['a', 'b']), { focus: 'a' }), 'c', {})
    expect(inserted.layout).toEqual(splitPair('horizontal', 0.5, tabs(['a', 'b']), tabs(['c'])))
    const moved = placeView(doc(splitPair('horizontal', 0.5, tabs(['a']), tabs(['b', 'c'], 0)), { focus: 'a' }), 'c', {
      zone: 'center',
      tabIndex: 0,
    })
    expect(moved.layout).toEqual(splitPair('horizontal', 0.5, tabs(['c', 'a'], 0), tabs(['b'])))
    expect(moved.focus).toBe('c')
  })

  it('activates an open view, inserts to the right of the focus, and starts from an empty tree', () => {
    const open = ensureInLayout(doc(tabs(['a', 'b'], 0), { focus: 'a' }), 'b')
    expect(open.layout).toEqual(tabs(['a', 'b'], 1))
    const added = ensureInLayout(doc(tabs(['a']), { focus: 'a' }), 'b')
    expect(added.layout).toEqual(splitPair('horizontal', 0.5, tabs(['a']), tabs(['b'])))
    expect(ensureInLayout(doc(tabs([])), 'a').layout).toEqual(tabs(['a']))
  })
})

describe('close, focus, zoom, reset', () => {
  it('closes a view, moves focus to the first remaining view, and clears zoom', () => {
    const next = closeView(doc(tabs(['a', 'b']), { focus: 'a', zoom: 'a' }), 'a')
    expect(next.layout).toEqual(tabs(['b']))
    expect(next.focus).toBe('b')
    expect(next.zoom).toBeUndefined()
  })

  it('focuses the neighbour and toggles zoom', () => {
    const tree = doc(splitPair('horizontal', 0.5, tabs(['a']), tabs(['b'])), { focus: 'a' })
    expect(focusDirection(tree, 'right').focus).toBe('b')
    expect(zoomView(tree, 'a').zoom).toBe('a')
    expect(zoomView(zoomView(tree, 'a'), 'a').zoom).toBeUndefined()
  })

  it('resets to the canonical tree', () => {
    const canonical = tabs(['a', 'b', 'c'])
    const next = resetLayout(doc(splitPair('horizontal', 0.5, tabs(['a']), tabs(['b'])), { zoom: 'a' }), canonical)
    expect(next.layout).toEqual(canonical)
    expect(next.zoom).toBeUndefined()
    expect(next.focus).toBe('a')
  })
})

describe('commands', () => {
  const start = doc(splitPair('horizontal', 0.5, tabs(['a']), tabs(['b', 'c'])), { focus: 'b' })

  it('maps open, place, and link onto the v4 operations', () => {
    const opened = applyLayoutCommand(start, 'open_view', { view: 'd' }, { views: catalog })
    expect(opened.doc.layout).toEqual(
      splitPair('horizontal', 0.5, tabs(['a']), splitPair('horizontal', 0.5, tabs(['b', 'c']), tabs(['d']))),
    )
    const placed = applyLayoutCommand(start, 'ui.place', { id: 'a', zone: 'bottom' }, { views: catalog })
    expect(views(placed.doc.layout)).toContain('a')
    expect(placed.doc.focus).toBe('a')
    const linked = applyLayoutCommand(start, 'ui.link', { id: 'c' }, { views: catalog })
    expect(linked.doc.layout).toEqual(activate(start.layout, 'c'))
    const tab = applyLayoutCommand(start, 'ui.place', { title: 'Detail', zone: 'new tab' }, { views: catalog })
    expect(findPane(tab.doc.layout, 'c')?.views).toEqual(['b', 'c'])
  })

  it('runs move, split, resize, swap, focus, zoom, close, and reset', () => {
    const moved = applyLayoutCommand(start, 'move_view_right', {}, { views: catalog })
    expect(moved.doc.focus).toBe('b')
    expect(findPane(moved.doc.layout, 'b')?.views).not.toEqual(['b', 'c'])
    const split = applyLayoutCommand(start, 'split', { view: 'b', zone: 'bottom' }, { views: catalog })
    expect(split.doc.layout.type).toBe('split')
    const sized = applyLayoutCommand(start, 'resize', { view: 'a', direction: 'right', delta: 0.1 }, { views: catalog })
    expect(sized.doc.layout).toMatchObject({ ratios: [0.6, 0.4] })
    const swapped = applyLayoutCommand(start, 'swap', { a: 'a', b: 'c' }, { views: catalog })
    expect(views(swapped.doc.layout)).toEqual(['c', 'b', 'a'])
    const focused = applyLayoutCommand(start, 'focus', { direction: 'left' }, { views: catalog })
    expect(focused.doc.focus).toBe('a')
    const zoomed = applyLayoutCommand(start, 'zoom', { view: 'b' }, { views: catalog })
    expect(zoomed.doc.zoom).toBe('b')
    const closed = applyLayoutCommand(zoomed.doc, 'close_view', { view: 'b' }, { views: catalog })
    expect(closed.doc.zoom).toBeUndefined()
    expect(closed.doc.focus).toBe('a')
    const reset = applyLayoutCommand(start, 'reset_layout', {}, { views: catalog, canonical: tabs(['a']) })
    expect(reset.doc.layout).toEqual(tabs(['a']))
    const read = applyLayoutCommand(start, 'get_layout', {}, { views: catalog })
    expect(JSON.parse(read.summary ?? '{}')).toMatchObject({ workspace: 'ws', focus: 'b', views: ['a', 'b', 'c'] })
  })
})

describe('zoneFromPoint', () => {
  it('checks the horizontal edges before the vertical ones', () => {
    expect(zoneFromPoint(0.1, 0.1)).toBe('left')
    expect(zoneFromPoint(0.9, 0.9)).toBe('right')
    expect(zoneFromPoint(0.5, 0.1)).toBe('top')
    expect(zoneFromPoint(0.5, 0.9)).toBe('bottom')
    expect(zoneFromPoint(0.5, 0.5)).toBe('center')
  })
})

function findPane(node: Node, view: string): { views: string[] } | null {
  if (node.type === 'tabs') return node.views.includes(view) ? node : null
  for (const child of node.children) {
    const found = findPane(child, view)
    if (found) return found
  }
  return null
}
