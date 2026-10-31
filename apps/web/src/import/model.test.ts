import { describe, expect, it } from 'vitest'
import {
  acceptsFile,
  storedTargetId,
  targetForStored,
  ariaMap,
  cellClass,
  confidenceLabel,
  curve,
  firstDroppedFile,
  importCellText,
  isFileDrag,
  lineTone,
  mappedLines,
  alternativeChoices,
  confidenceTone,
  dedupedTargets,
  mappingRank,
  matchBanner,
  matchSummary,
  showTypeConfidence,
  redoColumns,
  rowClass,
  saveLabel,
  setTarget,
  stepIndex,
  undoColumns,
  type ColumnLine,
} from './model'

const lines: ColumnLine[] = [
  { col: 0, target: 'type:sku', confidence: 0.97, source: 'deterministic' },
  { col: 1, target: 'type:name', confidence: 0.42, source: 'decisions', low: true },
  { col: 2, target: 'unmapped', confidence: 0, source: 'deterministic', reason: 'empty' },
]

describe('import mapping helpers', () => {
  it('accepts spreadsheets and treats tsv and txt as csv', () => {
    expect(acceptsFile('Prices.XLSX')).toBe(true)
    expect(acceptsFile('a.xls')).toBe(true)
    expect(acceptsFile('a.csv')).toBe(true)
    expect(acceptsFile('a.tsv')).toBe(true)
    expect(acceptsFile('notes.txt')).toBe(true)
    expect(acceptsFile('photo.png')).toBe(false)
  })

  it('opens the drop overlay only for file drags', () => {
    expect(isFileDrag(['Files'])).toBe(true)
    expect(isFileDrag(['text/plain'])).toBe(false)
  })

  it('takes the first file and names it', () => {
    expect(firstDroppedFile(['a.csv', 'b.csv'])).toEqual({
      name: 'a.csv',
      notice: 'One file at a time — importing a.csv',
    })
    expect(firstDroppedFile(['photo.png'])).toEqual({
      error: 'Only .xlsx, .xls and .csv can be imported',
    })
  })

  it('draws one line per mapped column and flags low confidence', () => {
    expect(mappedLines(lines).map((line) => line.col)).toEqual([0, 1])
    expect(lineTone(lines[0]!)).toBe('ok')
    expect(lineTone(lines[1]!)).toBe('warn')
    expect(confidenceLabel('deterministic', 0.97)).toBe('97 %')
    expect(confidenceLabel('manual', 1)).toBe('✓')
    expect(matchBanner(lines)).toEqual({ matched: 2, total: 3, marked: 1 })
    expect(matchSummary(lines)).toBe('2 of 3 columns are mapped. 1 needs a look.')
    expect(matchSummary([lines[0]!])).toBe('The column is mapped.')
    expect(matchSummary([])).toBe('No columns in this file.')
    expect(mappingRank(lines[2])).toBeLessThan(mappingRank(lines[1]))
    expect(mappingRank(lines[1])).toBeLessThan(mappingRank(lines[0]))
    expect(confidenceTone(lines[1])).toBe('low')
    expect(confidenceTone(lines[2])).toBe('unmapped')
    expect(showTypeConfidence('article', 'article', 1)).toBeNull()
    expect(showTypeConfidence('article', 'article', 0.72)).toBe(0.72)
    expect(showTypeConfidence('variant', 'article', 0.72)).toBeNull()
    expect(
      dedupedTargets([
        { id: 'type:description', label: 'Description (sv)' },
        { id: 'type:description@sv', label: 'Description (sv)' },
        { id: 'type:description@en', label: 'Description (en)' },
      ]).map((target) => target.id),
    ).toEqual(['type:description', 'type:description@en'])
    expect(
      alternativeChoices(
        {
          col: 2,
          target: 'type:description',
          confidence: 1,
          source: 'deterministic',
          alternatives: [
            { value: 'type:description@sv', p: 0.86 },
            { value: 'type:description@en', p: 0.86 },
          ],
        },
        [
          { id: 'type:description', label: 'Description (sv)' },
          { id: 'type:description@sv', label: 'Description (sv)' },
          { id: 'type:description@en', label: 'Description (en)' },
        ],
      ).map((alt) => alt.label),
    ).toEqual(['Description (en)'])
    expect(ariaMap('Number', 'SKU', 0.97)).toBe('Column Number maps to SKU, 97 percent')
  })

  it('changes, removes, adds and undoes a line', () => {
    const removed = setTarget(lines, 0, 'unmapped')
    expect(removed.columns[0]?.target).toBe('unmapped')
    const added = setTarget(removed.columns, 2, 'type:weight')
    expect(added.columns[2]?.target).toBe('type:weight')
    expect(added.columns[2]?.source).toBe('manual')
    const stolen = setTarget(lines, 1, 'type:sku')
    expect(stolen.displaced?.col).toBe(0)
    expect(stolen.columns[0]?.target).toBe('unmapped')
    expect(stolen.columns[1]?.target).toBe('type:sku')
    const undone = undoColumns([lines], stolen.columns, [])
    expect(undone.current).toEqual(lines)
    const redone = redoColumns(undone.history, undone.current, undone.future)
    expect(redone.current[1]?.target).toBe('type:sku')
  })

  it('bends a mapping line the same way as the graph', () => {
    const path = curve({ x: 0, y: 10 }, { x: 200, y: 40 })
    expect(path.width).toBe(1.25)
    expect(path.widthHover).toBe(1.8)
    expect(path.control.x).toBeGreaterThan(0)
  })

  it('formats measure-shaped cell payloads for display', () => {
    expect(importCellText('500 g')).toBe('500 g')
    expect(importCellText({ value: '500', unit: 'g' })).toBe('500 g')
    expect(importCellText({ value: '12' })).toBe('12')
  })

  it('paints preview rows and the save label', () => {
    expect(rowClass('new')).toContain('new')
    expect(rowClass('delete')).toContain('delete')
    expect(cellClass('changed')).toContain('changed')
    expect(cellClass('error')).toContain('error')
    expect(saveLabel(0, 12)).toBe('Approve 12 rows')
    expect(saveLabel(3, 12)).toBe('Approve 12 rows')
    expect(stepIndex('mapping')).toBe(1)
    expect(stepIndex('preview')).toBe(2)
    expect(stepIndex('saving')).toBe(3)
  })

  it('matches a saved attribute id to the key-form target', () => {
    const targets = [{
      id: 'type:sku',
      label: 'Article number',
      attribute_id: '018f0000-0000-7000-8000-000000000100',
    }]
    expect(storedTargetId(targets[0])).toBe('018f0000-0000-7000-8000-000000000100')
    expect(targetForStored(targets, '018f0000-0000-7000-8000-000000000100')?.label).toBe('Article number')
    expect(targetForStored(targets, 'type:018f0000-0000-7000-8000-000000000100')?.label).toBe('Article number')
    expect(targetForStored(targets, 'type:sku')?.id).toBe('type:sku')
  })
})
