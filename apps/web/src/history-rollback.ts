/**
 * History rows are change events. Restore undoes the selected change by rolling
 * back to the previous sequence (state before that change). View as it was still
 * shows state at the selected seq.
 */
export function rollbackTargetSeq(seqs: number[], selected: number): number {
  const prior = [...seqs].filter((seq) => seq < selected).sort((left, right) => left - right).at(-1)
  return prior ?? selected
}
