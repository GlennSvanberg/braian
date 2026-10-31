/** True when a cell editor draft should be PATCH'ed. Esc / unchanged must return false. */
export function cellEditShouldPatch(original: string, nextText: string): boolean {
  return nextText !== original
}

/**
 * Build PATCH `values` for an inline cell edit.
 *
 * Language-dependent attributes (e.g. `name`) must be sent as
 * `{ [attrKey]: { [lang]: text } }` — the same shape the detail Editor uses —
 * even when the grid column id has no `@lang` suffix. Bare strings are rejected
 * by the API and leave the old value on screen after refresh.
 */
export function cellEditValuesBody(args: {
  attrKey: string
  nextText: string
  /** Language from a column id like `name@en` or `description@sv`. */
  columnLanguage?: string
  languageDependent: boolean
  uiLanguage: string
}): { [key: string]: unknown } {
  if (args.columnLanguage) {
    return { [args.attrKey]: { [args.columnLanguage]: args.nextText } }
  }
  if (args.languageDependent) {
    return { [args.attrKey]: { [args.uiLanguage]: args.nextText } }
  }
  return { [args.attrKey]: args.nextText }
}


/** True when the event target (or document focus) is inside an inline `.cell-editor`. */
export function isTextEditingTarget(event: { target: EventTarget | null }): boolean {
  if (event.target instanceof Element && event.target.closest('.cell-editor')) {
    return true
  }
  if (typeof document !== 'undefined') {
    const active = document.activeElement
    if (active instanceof Element && active.closest('.cell-editor')) {
      return true
    }
  }
  return false
}

/**
 * True while ObjectTable has an open cell editor (`editingCell`) or focus is in `.cell-editor`.
 * Call first in grid / global keydown handlers so Home/End/Ctrl+K/etc. never steal keys from the caret.
 */
export function isEditingCell(
  event: { target: EventTarget | null },
  editingCell?: { rowId: string; columnId: string } | null,
): boolean {
  return Boolean(editingCell) || isTextEditingTarget(event)
}
