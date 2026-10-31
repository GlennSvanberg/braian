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
