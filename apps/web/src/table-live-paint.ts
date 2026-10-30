/**
 * While Typesense hits and pathValues refetch after a live edit, keep painting
 * the previous row so Description and Version swap in one sweep.
 */
export function shouldHoldTablePaint(input: {
  hasPaintedData: boolean
  objectsFetching: boolean
  pathBusy: boolean
}): boolean {
  return input.hasPaintedData && (input.objectsFetching || input.pathBusy)
}
