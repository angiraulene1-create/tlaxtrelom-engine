#!/usr/bin/env node
// SPDX-License-Identifier: LicenseRef-Tlaxtrelom-Proprietary
// Copyright (c) 2026 Tlaxtrelom Engine. All rights reserved.
// CONFIDENTIAL — internal use only.
/**
 * CLI de autoría de Tlaxtrelom Engine (TypeScript).
 *
 *   node src/cli.ts selftest [--json]
 *   node src/cli.ts demo [--json]
 *   node src/cli.ts check <hir.json|-> [--json]
 *   node src/cli.ts hash <hir.json|->
 *
 * Es la contraparte de `python -m tlaxtrelom_tools.cli`; ambos deben emitir
 * el mismo hash para el mismo proyecto.
 */

import { readFileSync } from "node:fs";
import { demoHir, DEMO_HASH } from "./builder.ts";
import {
  HASH_PREFIX,
  IR_SCHEMA,
  IrError,
  canonicalText,
  fromBytes,
  hashHir,
  verify,
  type Hir,
} from "./ir.ts";

const ENGINE_NAME = "tlaxtrelom-engine";
const VERSION = "0.2.0";

interface SelftestReport {
  engine: string;
  version: string;
  ok: boolean;
  schema: string;
  hash: string;
  ops: number;
  checks: string[];
}

function selftest(): SelftestReport {
  const checks: string[] = [];

  const hir = demoHir();
  verify(hir);
  checks.push("hir_build");

  if (hir.hash !== DEMO_HASH) {
    throw new IrError(`el demo no coincide con Python: ${String(hir.hash)} != ${DEMO_HASH}`);
  }
  checks.push("python_parity");

  if (typeof hir.hash !== "string" || !hir.hash.startsWith(HASH_PREFIX)) {
    throw new IrError("prefijo de hash inesperado");
  }
  checks.push("hash_prefix");

  const again = demoHir();
  if (again.hash !== hir.hash) {
    throw new IrError("el builder no es determinista");
  }
  checks.push("builder_deterministic");

  const canonical = canonicalText(hir);
  if (!canonical.startsWith("{")) {
    throw new IrError("la serialización canónica no es JSON");
  }
  if (canonical.includes(" ")) {
    throw new IrError("la serialización canónica contiene espacios");
  }
  checks.push("canonical_bytes");

  // Fixtures de formato de float: cubren los umbrales donde JS y CPython
  // discrepan si se usa JSON.stringify directamente.
  const fixtures: Array<[number, string]> = [
    [0.00001, '{"x":1e-05}'],
    [0.0001, '{"x":0.0001}'],
    [1e16, '{"x":10000000000000000}'],
    [1e21, '{"x":1000000000000000000000}'],
    [-0, '{"x":0}'],
  ];
  for (const [value, expected] of fixtures) {
    const actual = canonicalText({ x: value });
    if (actual !== expected) {
      throw new IrError(`formato de float incompatible: ${actual} != ${expected}`);
    }
  }
  checks.push("float_parity");

  const tampered: Hir = { ...hir, module: "tampered" };
  let rejected = false;
  try {
    verify(tampered);
  } catch (error) {
    if (error instanceof IrError) rejected = true;
    else throw error;
  }
  if (!rejected) {
    throw new IrError("un HIR manipulado pasó la verificación");
  }
  checks.push("tamper_detection");

  // Un HIR determinista no puede contener efectos neuronales.
  let neuralRejected = false;
  try {
    const illegal = { ...hir, ops: [{ ...(hir.ops as object[])[0], effects: ["neural"] }] };
    verify(illegal as Hir);
  } catch (error) {
    if (error instanceof IrError) neuralRejected = true;
    else throw error;
  }
  if (!neuralRejected) {
    throw new IrError("un HIR determinista aceptó neural");
  }
  checks.push("determinism_gate");

  const roundtrip = fromBytes(new TextEncoder().encode(JSON.stringify(hir)));
  if (roundtrip.hash !== hir.hash) {
    throw new IrError("el round-trip cambió el hash");
  }
  checks.push("roundtrip");

  return {
    engine: ENGINE_NAME,
    version: VERSION,
    ok: true,
    schema: IR_SCHEMA,
    hash: String(hir.hash),
    ops: (hir.ops as unknown[]).length,
    checks,
  };
}

function readHir(path: string): Hir {
  const text = path === "-" ? readFileSync(0, "utf8") : readFileSync(path, "utf8");
  let parsed: unknown;
  try {
    parsed = JSON.parse(text);
  } catch (error) {
    throw new IrError(`JSON inválido: ${String(error)}`);
  }
  if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) {
    throw new IrError("el HIR raíz debe ser un objeto");
  }
  return parsed as Hir;
}

function printJson(payload: unknown): void {
  process.stdout.write(`${JSON.stringify(payload, null, 2)}\n`);
}

function usage(): void {
  process.stderr.write(
    [
      "uso: tlaxtrelom-ts <comando>",
      "",
      "  selftest [--json]     valida el pipeline TypeScript",
      "  demo [--json]         emite el HIR de referencia",
      "  check <archivo|->     valida un HIR y su hash",
      "  hash <archivo|->      calcula el hash de un HIR",
      "",
    ].join("\n"),
  );
}

function main(argv: string[]): number {
  const [command, ...rest] = argv;
  const asJson = rest.includes("--json");

  try {
    switch (command) {
      case "selftest": {
        const report = selftest();
        if (asJson) {
          printJson(report);
        } else {
          process.stdout.write(`${report.engine} ${report.version} — selftest OK\n`);
          for (const check of report.checks) {
            process.stdout.write(`  ✓ ${check}\n`);
          }
          process.stdout.write(`  hash: ${report.hash}\n`);
          process.stdout.write(`  ops : ${report.ops}\n`);
        }
        return 0;
      }
      case "demo": {
        const hir = demoHir();
        printJson(asJson ? hir : hir);
        return 0;
      }
      case "check": {
        const path = rest.find((arg) => arg !== "--json");
        if (!path) {
          usage();
          return 2;
        }
        const hir = readHir(path);
        verify(hir);
        printJson({
          ok: true,
          engine: ENGINE_NAME,
          hash: hir.hash,
          module: hir.module,
          ops: (hir.ops as unknown[]).length,
        });
        return 0;
      }
      case "hash": {
        const path = rest.find((arg) => arg !== "--json");
        if (!path) {
          usage();
          return 2;
        }
        process.stdout.write(`${hashHir(readHir(path))}\n`);
        return 0;
      }
      default:
        usage();
        return 2;
    }
  } catch (error) {
    process.stderr.write(`error: ${error instanceof Error ? error.message : String(error)}\n`);
    return 2;
  }
}

process.exitCode = main(process.argv.slice(2));
