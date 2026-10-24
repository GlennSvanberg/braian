import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import { Validator } from "../pkg/openpim_rules_wasm.js";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../../testdata/rules");
const validator = new Validator(readFileSync(path.join(root, "definitions.json"), "utf8"));
const caseDir = path.join(root, "cases");
const files = readdirSync(caseDir)
  .filter((name) => name.endsWith(".json"))
  .sort();

function locate(error) {
  return {
    attribute: error.attribute ?? null,
    language: error.language ?? null,
    index: error.index ?? null,
    code: error.code,
  };
}

function key(error) {
  return [error.attribute ?? "", error.language ?? "", error.index ?? "", error.code].join("\0");
}

for (const file of files) {
  test(file, () => {
    const spec = JSON.parse(readFileSync(path.join(caseDir, file), "utf8"));
    const expected = spec.expected.map(locate).sort((left, right) => key(left).localeCompare(key(right)));
    const warnings = (spec.warnings ?? []).map(locate).sort((left, right) => key(left).localeCompare(key(right)));
    if (spec.patch) {
      const report = JSON.parse(
        validator.validate_patch(JSON.stringify(spec.object), JSON.stringify(spec.patch)),
      );
      const errors = report.errors.map(locate).sort((left, right) => key(left).localeCompare(key(right)));
      const gotWarnings = report.warnings.map(locate).sort((left, right) => key(left).localeCompare(key(right)));
      assert.deepEqual(errors, expected);
      assert.deepEqual(gotWarnings, warnings);
      return;
    }
    const got = JSON.parse(validator.validate(JSON.stringify(spec.object))).map(locate);
    got.sort((left, right) => key(left).localeCompare(key(right)));
    assert.deepEqual(got, expected);
    assert.deepEqual(warnings, []);
  });
}
