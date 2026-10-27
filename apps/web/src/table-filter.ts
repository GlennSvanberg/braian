/** Search filter for an object table.
 *
 * The table stays on the view's type. A selected group narrows that type;
 * a selection cannot switch the table to another type.
 */
export function objectTableFilter(typeKey: string, group: string | null): string {
  const parts: string[] = []
  if (typeKey) parts.push(`type_key:=${typeKey}`)
  if (group) parts.push(`groups:=${group}`)
  return parts.join(' && ')
}
