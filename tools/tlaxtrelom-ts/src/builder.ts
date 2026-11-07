// SPDX-License-Identifier: LicenseRef-Tlaxtrelom-Proprietary
// Copyright (c) 2026 Tlaxtrelom Engine. All rights reserved.
// CONFIDENTIAL — internal use only.
/**
 * Constructor de HIR para el editor y los tests de TypeScript.
 *
 * El builder no tiene semántica de gameplay: solo garantiza que el HIR sea
 * SSA, tipado de forma consistente y determinista por defecto. El compilador
 * de Rust sigue siendo el único que valida efectos y lowering real.
 */

import { IrError, IR_SCHEMA, seal, type Hir, type IrOp, type IrOutput, type IrValue } from "./ir.ts";

export const T = {
  BOOL: "bool",
  I32: "i32",
  I64: "i64",
  F32: "f32",
  F64: "f64",
  VEC3_F32: "vec3<f32>",
} as const;

export type ScalarType = (typeof T)[keyof typeof T];

export interface ValueRef {
  readonly id: number;
  readonly type: string;
}

export interface BuilderOptions {
  deterministic?: boolean;
}

const MODULE_RE = /^[A-Za-z_][A-Za-z0-9_]*$/;

export class HirBuilder {
  readonly module: string;
  readonly entry: string;
  readonly deterministic: boolean;
  readonly values: IrValue[] = [];
  readonly ops: IrOp[] = [];
  readonly outputs: IrOutput[] = [];

  constructor(module: string, options: BuilderOptions = {}) {
    if (!MODULE_RE.test(module)) {
      throw new IrError(`nombre de módulo inválido: ${module}`);
    }
    this.module = module;
    this.entry = "main";
    this.deterministic = options.deterministic ?? true;
  }

  private pushValue(value: Omit<IrValue, "id">): ValueRef {
    const id = this.values.length;
    this.values.push({ id, ...value });
    return { id, type: String(value.type) };
  }

  param(name: string, type: string): ValueRef {
    if (!name) throw new IrError("un parámetro necesita nombre");
    return this.pushValue({ kind: "param", type, name });
  }

  const(type: string, value: unknown): ValueRef {
    if (typeof value === "number" && !Number.isFinite(value)) {
      throw new IrError(`constante no finita en ${this.module}`);
    }
    return this.pushValue({ kind: "const", type, value });
  }

  op(
    name: string,
    type: string,
    args: readonly ValueRef[],
    effects: readonly string[] = ["pure"],
  ): ValueRef {
    const id = this.values.length;
    for (const arg of args) {
      if (!Number.isInteger(arg.id) || arg.id < 0 || arg.id >= id) {
        throw new IrError(`op ${name}: argumento no SSA (${arg.id})`);
      }
    }
    const ref = this.pushValue({ kind: "op", type });
    const op: IrOp = {
      id,
      op: name,
      type,
      args: args.map((arg) => arg.id),
      effects: [...effects],
    };
    this.ops.push(op);
    return ref;
  }

  output(name: string, value: ValueRef): void {
    if (!MODULE_RE.test(name)) {
      throw new IrError(`nombre de output inválido: ${name}`);
    }
    if (this.outputs.some((output) => output.name === name)) {
      throw new IrError(`output duplicado: ${name}`);
    }
    this.outputs.push({ name, value: value.id, type: value.type });
  }

  /** Emite un HIR sellado y validado. */
  build(): Hir {
    if (this.outputs.length === 0) {
      throw new IrError("el HIR necesita al menos un output");
    }
    const hir: Hir = {
      schema: IR_SCHEMA,
      module: this.module,
      entry: this.entry,
      deterministic: this.deterministic,
      values: this.values,
      ops: this.ops,
      outputs: this.outputs,
    };
    return seal(hir);
  }
}

/**
 * Grafo de referencia idéntico a `demo_graph()` de Python.
 * Su hash es un fixture de compatibilidad entre lenguajes.
 */
export function demoHir(): Hir {
  const builder = new HirBuilder("player_locomotion");
  const speed = builder.param("speed", T.F32);
  const forward = builder.op("vec3.compose", T.VEC3_F32, [
    builder.const(T.F32, 0),
    builder.const(T.F32, 0),
    builder.const(T.F32, 1),
  ]);
  const velocity = builder.op("vec3.scale", T.VEC3_F32, [forward, speed]);
  builder.output("velocity", velocity);
  return builder.build();
}

export const DEMO_HASH = "blake2b512-128:b2861452eb9b8267652b896c81d00774";
