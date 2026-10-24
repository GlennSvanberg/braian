import { readFileSync } from "node:fs";
import path from "node:path";
import { performance } from "node:perf_hooks";

import { Validator } from "./pkg/openpim_rules_wasm.js";

const dir = process.argv[2];
if (!dir) {
  console.error("usage: node packages/rules-wasm/demo-1.mjs <dir>");
  process.exit(2);
}

const definitions = readFileSync(path.join(dir, "definitions.json"), "utf8");
const objects = readFileSync(path.join(dir, "objects.jsonl"), "utf8")
  .split(/\r?\n/)
  .filter((line) => line.length > 0);
const rustReports = readFileSync(path.join(dir, "rust-reports.jsonl"), "utf8")
  .split(/\r?\n/)
  .filter((line) => line.length > 0);
const summary = JSON.parse(readFileSync(path.join(dir, "summary.json"), "utf8"));
const validator = new Validator(definitions);

if (objects.length !== rustReports.length) {
  console.error(`object/report count mismatch: ${objects.length} vs ${rustReports.length}`);
  process.exit(1);
}

console.log("object  rust_us  wasm_us  result");
let failed = 0;
for (let index = 0; index < objects.length; index += 1) {
  const started = performance.now();
  const wasmReport = validator.validate(objects[index]);
  const wasmUs = Math.round((performance.now() - started) * 1000);
  const rustUs = summary.rust_validate_us[index] ?? "";
  const match = wasmReport === rustReports[index];
  const codes = JSON.parse(wasmReport).map((error) => error.code);
  const label = codes.length === 0 ? "ok" : codes.join(",");
  if (!match) {
    failed += 1;
    console.log(
      `${String(index).padStart(6)}  ${String(rustUs).padStart(7)}  ${String(wasmUs).padStart(7)}  MISMATCH ${label}`,
    );
    console.error(`rust ${rustReports[index]}`);
    console.error(`wasm ${wasmReport}`);
  } else {
    console.log(
      `${String(index).padStart(6)}  ${String(rustUs).padStart(7)}  ${String(wasmUs).padStart(7)}  ${label}`,
    );
  }
}

if (failed > 0) {
  console.error(`${failed} of ${objects.length} objects disagreed`);
  process.exit(1);
}
console.log(`match: ${objects.length}/${objects.length}`);
