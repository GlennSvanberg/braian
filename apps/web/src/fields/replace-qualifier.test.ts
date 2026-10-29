import { describe, expect, it } from "vitest"
import { readFileSync } from "node:fs"
import { fieldCatalogForType } from "./catalog"
import { makeViewColumn, withReplacedQualifier, parseColumnId, columnTitle } from "./columns"

const defs = JSON.parse(readFileSync(new URL("../../../../crates/server/demo/definitions.json", import.meta.url), "utf8"))

describe("withReplacedQualifier Specific→First", () => {
  it("changes column id, title and drops peer value", () => {
    const catalog = fieldCatalogForType(defs, "product")
    const col = makeViewColumn(
      ["articles", "net_weight"],
      [
        { kind: "cardinality", optionId: "specific", label: "0731", value: "peer-uuid" },
        { kind: "unit", optionId: "g", label: "g" },
      ],
      catalog,
    )
    expect(col).toBeTruthy()
    expect(col!.id).toContain("specific=peer-uuid")
    expect(columnTitle(col!)).toMatch(/specific|0731/i)

    const next = withReplacedQualifier(col!, { kind: "cardinality", optionId: "first", label: "First" }, catalog)
    expect(next.id).not.toBe(col!.id)
    expect(next.id).toContain("cardinality:first")
    expect(next.id).not.toContain("specific")
    expect(next.qualifiers.find((q) => q.kind === "cardinality")?.value).toBeUndefined()
    expect(columnTitle(next)).toMatch(/first/i)
    expect(columnTitle(next)).not.toMatch(/specific/i)

    // Rebuild from id as the table does
    const { path, qualifiers } = parseColumnId(next.id)
    const leaf = (path.split(">").pop() ?? path).split(".")[0]!.split("@")[0]!
    const via = path.includes(">") ? path.split(">")[0]! : null
    const rebuilt = makeViewColumn(via ? [via, leaf] : [leaf], qualifiers, catalog)
    expect(rebuilt?.id).toBe(next.id)
    expect(columnTitle(rebuilt!)).toMatch(/first/i)
  })
})
