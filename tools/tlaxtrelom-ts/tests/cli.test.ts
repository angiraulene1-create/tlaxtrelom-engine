// SPDX-License-Identifier: LicenseRef-Tlaxtrelom-Proprietary
// Copyright (c) 2026 Tlaxtrelom Engine. All rights reserved.
// CONFIDENTIAL — internal use only.
/** Tests de la CLI de TypeScript. */

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import { DEMO_HASH, demoHir } from "../src/builder.ts";

const CLI = fileURLToPath(new URL("../src/cli.ts", import.meta.url));

function run(args: string[], input?: string) {
  return spawnSync(process.execPath, [CLI, ...args], {
    encoding: "utf8",
    input,
  });
}

test("selftest --json reporta ok y paridad con Python", () => {
  const result = run(["selftest", "--json"]);
  assert.equal(result.status, 0, result.stderr);
  const report = JSON.parse(result.stdout);
  assert.equal(report.ok, true);
  assert.equal(report.hash, DEMO_HASH);
  assert.ok(report.checks.includes("python_parity"));
  assert.ok(report.checks.includes("float_parity"));
});

test("check y hash aceptan stdin", () => {
  const hir = demoHir();
  const input = JSON.stringify(hir);

  const check = run(["check", "-"], input);
  assert.equal(check.status, 0, check.stderr);
  assert.equal(JSON.parse(check.stdout).hash, DEMO_HASH);

  const hash = run(["hash", "-"], input);
  assert.equal(hash.status, 0, hash.stderr);
  assert.equal(hash.stdout.trim(), DEMO_HASH);
});

test("check rechaza un hash manipulado", () => {
  const hir = { ...demoHir(), module: "tampered" };
  const result = run(["check", "-"], JSON.stringify(hir));
  assert.equal(result.status, 2);
  assert.match(result.stderr, /hash inválido/);
});

test("check rechaza JSON inválido sin abortar", () => {
  const result = run(["check", "-"], "{");
  assert.equal(result.status, 2);
  assert.match(result.stderr, /error:/);
});

test("un comando desconocido devuelve error de uso", () => {
  const result = run(["nope"]);
  assert.equal(result.status, 2);
  assert.match(result.stderr, /uso:/);
});

test("check lee un archivo del disco", () => {
  const dir = mkdtempSync(join(tmpdir(), "tlaxtrelom-ts-"));
  const path = join(dir, "hir.json");
  writeFileSync(path, JSON.stringify(demoHir()), "utf8");
  const result = run(["check", path]);
  assert.equal(result.status, 0, result.stderr);
  assert.equal(JSON.parse(result.stdout).module, "player_locomotion");
});
