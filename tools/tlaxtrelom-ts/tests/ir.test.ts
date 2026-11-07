// SPDX-License-Identifier: LicenseRef-Tlaxtrelom-Proprietary
// Copyright (c) 2026 Tlaxtrelom Engine. All rights reserved.
// CONFIDENTIAL — internal use only.
/** Tests de la forma canónica y el validador de HIR (TypeScript). */

import assert from "node:assert/strict";
import { test } from "node:test";

import { DEMO_HASH, HirBuilder, T, demoHir } from "../src/builder.ts";
import {
  HASH_PREFIX,
  IrError,
  canonicalText,
  fromBytes,
  hashHir,
  seal,
  toBytes,
  verify,
  type Hir,
} from "../src/ir.ts";

function sampleHir(): Hir {
  return {
    schema: "tlaxtrelom.ir/1",
    module: "sample",
    entry: "main",
    deterministic: true,
    values: [
      { id: 0, type: "f32", kind: "const", value: 1.0 },
      { id: 1, type: "f32", kind: "op" },
    ],
    ops: [
      { id: 1, op: "math.add", args: [0, 0], type: "f32", effects: ["pure"] },
    ],
    outputs: [{ name: "out", value: 1, type: "f32" }],
  };
}

test("el demo coincide byte a byte con Python", () => {
  const hir = demoHir();
  assert.equal(hir.hash, DEMO_HASH);
  verify(hir);
});

test("seal es idempotente y el prefijo es el esperado", () => {
  const sealed = seal(sampleHir());
  assert.ok(String(sealed.hash).startsWith(HASH_PREFIX));
  assert.deepEqual(seal(sealed), sealed);
  verify(sealed);
});

test("1.0 y 1 producen el mismo hash", () => {
  const a = sampleHir();
  const b = sampleHir();
  (b.values as Array<Record<string, unknown>>)[0].value = 1;
  assert.equal(hashHir(a), hashHir(b));
});

test("el formato de floats replica a CPython", () => {
  const fixtures: Array<[number, string]> = [
    [1.5, '{"x":1.5}'],
    [0.0001, '{"x":0.0001}'],
    [0.00001, '{"x":1e-05}'],
    [1e-7, '{"x":1e-07}'],
    [1e15, '{"x":1000000000000000}'],
    [1e16, '{"x":10000000000000000}'],
    [1.5e16, '{"x":15000000000000000}'],
    [1.234e-7, '{"x":1.234e-07}'],
    [1e21, '{"x":1000000000000000000000}'],
    [-0, '{"x":0}'],
  ];
  for (const [value, expected] of fixtures) {
    assert.equal(canonicalText({ x: value }), expected, `valor ${value}`);
  }
});

test("las claves se ordenan por punto de código", () => {
  assert.equal(canonicalText({ b: 1, a: 2 }), '{"a":2,"b":1}');
});

test("la manipulación se detecta", () => {
  const sealed = seal(sampleHir());
  const tampered: Hir = { ...sealed, outputs: [{ name: "other", value: 1, type: "f32" }] };
  assert.throws(() => verify(tampered), IrError);
});

test("schema incorrecto es rechazado", () => {
  const payload = sampleHir();
  payload.schema = "tlaxtrelom.ir/999";
  assert.throws(() => seal(payload), IrError);
});

test("argumento colgante es rechazado", () => {
  const payload = sampleHir();
  (payload.ops as Array<Record<string, unknown>>)[0].args = [0, 99];
  assert.throws(() => seal(payload), IrError);
});

test("argumento no SSA es rechazado", () => {
  const payload = sampleHir();
  (payload.ops as Array<Record<string, unknown>>)[0].args = [1, 0];
  assert.throws(() => seal(payload), IrError);
});

test("un grafo determinista rechaza efectos neuronales", () => {
  const payload = sampleHir();
  (payload.ops as Array<Record<string, unknown>>)[0].effects = ["neural"];
  assert.throws(() => seal(payload), IrError);
});

test("un grafo no determinista acepta efectos neuronales", () => {
  const payload = sampleHir();
  payload.deterministic = false;
  (payload.ops as Array<Record<string, unknown>>)[0].effects = ["neural"];
  verify(seal(payload));
});

test("round-trip de bytes", () => {
  const sealed = seal(sampleHir());
  const blob = toBytes(sealed);
  assert.deepEqual(fromBytes(blob), sealed);
});

test("un HIR sin outputs es rechazado", () => {
  const payload = sampleHir();
  payload.outputs = [];
  assert.throws(() => seal(payload), IrError);
});

test("el tipo del output debe coincidir con el value", () => {
  const payload = sampleHir();
  (payload.outputs as Array<Record<string, unknown>>)[0].type = "i32";
  assert.throws(() => seal(payload), IrError);
});

test("NaN e Infinity son rechazados", () => {
  assert.throws(() => canonicalText({ x: Number.NaN }), IrError);
  assert.throws(() => canonicalText({ x: Number.POSITIVE_INFINITY }), IrError);
});

test("claves con undefined son rechazadas en vez de desaparecer", () => {
  assert.throws(() => canonicalText({ x: undefined }), IrError);
});

test("el builder emite HIR verificado y determinista", () => {
  const build = (): Hir => {
    const builder = new HirBuilder("locomotion");
    const speed = builder.param("speed", T.F32);
    const value = builder.op("vec3.scale", T.VEC3_F32, [builder.const(T.F32, 1), speed]);
    builder.output("velocity", value);
    return builder.build();
  };
  const first = build();
  const second = build();
  assert.equal(first.hash, second.hash);
  verify(first);
});

test("el builder rechaza outputs duplicados", () => {
  const builder = new HirBuilder("duplicate");
  const value = builder.const(T.F32, 1);
  builder.output("out", value);
  assert.throws(() => builder.output("out", value), IrError);
});
