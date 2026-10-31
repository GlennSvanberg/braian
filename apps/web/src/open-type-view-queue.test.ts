import { describe, expect, it } from 'vitest'
import {
  clearQueuedOpenTypeView,
  peekQueuedOpenTypeView,
  queueOpenTypeView,
  takeQueuedOpenTypeView,
} from './open-type-view-queue'

describe('open-type-view queue', () => {
  it('stores one pending type id and clears on take', () => {
    clearQueuedOpenTypeView()
    expect(peekQueuedOpenTypeView()).toBeNull()
    queueOpenTypeView('type-a')
    queueOpenTypeView('type-b')
    expect(peekQueuedOpenTypeView()).toBe('type-b')
    expect(takeQueuedOpenTypeView()).toBe('type-b')
    expect(peekQueuedOpenTypeView()).toBeNull()
    expect(takeQueuedOpenTypeView()).toBeNull()
  })
})
