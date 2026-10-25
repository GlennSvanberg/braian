/** Search filter for an object table.
 *
 * With no structure group, the table stays on the view's type. With a group,
 * a saved assortment for that group decides the type, so a Products list does
 * not hide the articles that assortment already shows. A group with no
 * assortment lists every object in the group.
 */
export function objectTableFilter(
  typeKey: string,
  group: string | null,
  assortments: ReadonlyArray<{ query?: unknown }>,
): string {
  if (!group) return typeKey ? `type_key:=${typeKey}` : ''
  const types = new Set<string>()
  for (const item of assortments) {
    if (!item.query || typeof item.query !== 'object') continue
    const query = item.query as { type?: unknown; group?: unknown }
    if (query.group !== group) continue
    if (typeof query.type === 'string' && query.type) types.add(query.type)
  }
  const groupFilter = `groups:=${group}`
  const list = [...types]
  if (list.length === 0) return groupFilter
  if (list.length === 1) return `type_key:=${list[0]} && ${groupFilter}`
  return `(${list.map((type) => `type_key:=${type}`).join(' || ')}) && ${groupFilter}`
}
