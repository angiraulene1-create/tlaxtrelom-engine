// SPDX-License-Identifier: LicenseRef-Tlaxtrelom-Proprietary
// Copyright (c) 2026 Tlaxtrelom Engine. All rights reserved.
// CONFIDENTIAL — internal use only.
/**
 * Forma canónica y validación del Tlaxtrelom HIR (TypeScript).
 *
 * Este archivo debe producir **exactamente los mismos bytes** que
 * `python/tlaxtrelom_tools/ir.py`:
 *
 * - JSON con claves ordenadas por punto de código y sin espacios.
 * - Números enteros normalizados (1.0 y 1 son el mismo byte).
 * - Floats formateados como `repr()` de CPython, no como `JSON.stringify`.
 * - BLAKE2b-512 truncado a 128 bits, prefijo `blake2b512-128:`.
 *
 * La compatibilidad no es opcional: el editor (TS) y la CLI de cocina
 * (Python) firman el mismo proyecto con el mismo ID.
 */

import { createHash } from "node:crypto";

export const IR_SCHEMA = "tlaxtrelom.ir/1";
export const HASH_PREFIX = "blake2b512-128:";

export const ALLOWED_EFFECTS = new Set(["pure", "write", "neural", "io", "nondet"]);
export const NON_DETERMINISTIC_EFFECTS = new Set(["neural", "io", "nondet"]);

const MODULE_RE = /^[A-Za-z_][A-Za-z0-9_]*$/;

export type IrValue = Record<string, unknown>;
export type IrOp = Record<string, unknown>;
export type IrOutput = Record<string, unknown>;
export type Hir = Record<string, unknown>;

/** El HIR no cumple el schema o su hash no corresponde. */
export class IrError extends Error {
  override readonly name = "IrError";
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/**
 * Compara strings por punto de código, igual que `sorted()` de Python.
 * El comparador por defecto de JS usa unidades UTF-16, que discrepa con
 * pares subrogados.
 */
function compareCodePoints(a: string, b: string): number {
  const left = Array.from(a);
  const right = Array.from(b);
  const shared = Math.min(left.length, right.length);
  for (let i = 0; i < shared; i += 1) {
    const diff = (left[i].codePointAt(0) ?? 0) - (right[i].codePointAt(0) ?? 0);
    if (diff !== 0) return diff;
  }
  return left.length - right.length;
}

/**
 * Formatea un float como `repr()` de CPython.
 *
 * `Number.prototype.toString` y `JSON.stringify` usan umbrales distintos para
 * notación científica (1e-6 / 1e21) que CPython (1e-4 / 1e16). Sin esto, un
 * HIR con `0.00001` tendría hashes distintos en el editor y en la CLI.
 */
function formatFloat(value: number): string {
  if (!Number.isFinite(value)) {
    throw new IrError(`constante no finita en HIR: ${value}`);
  }
  if (Number.isInteger(value)) {
    // -0 se normaliza a 0, igual que `_normalize` en Python.
    if (Object.is(value, -0)) return "0";
    // Para |x| >= 1e21, toString usa notación científica; BigInt da los
    // dígitos exactos del double, que es lo que produce int(float) en Python.
    if (Math.abs(value) >= 1e21) return BigInt(value).toString();
    return value.toString();
  }

  const negative = value < 0;
  const absolute = Math.abs(value);
  const [mantissa, exponentText] = absolute.toExponential().split("e");
  const exponent = Number(exponentText);
  const digits = mantissa.replace(".", "");

  let body: string;
  if (exponent >= -4 && exponent < 16) {
    if (exponent >= 0) {
      const integer = digits.slice(0, exponent + 1).padEnd(exponent + 1, "0");
      const fraction = digits.slice(exponent + 1);
      body = fraction.length ? `${integer}.${fraction}` : `${integer}.0`;
    } else {
      body = `0.${"0".repeat(-exponent - 1)}${digits}`;
    }
  } else {
    const sign = exponent < 0 ? "-" : "+";
    const magnitude = Math.abs(exponent).toString().padStart(2, "0");
    body =
      digits.length > 1
        ? `${digits[0]}.${digits.slice(1)}e${sign}${magnitude}`
        : `${digits}e${sign}${magnitude}`;
  }
  return negative ? `-${body}` : body;
}

/** Serializa un valor con las mismas convenciones que `json.dumps`. */
function serialize(value: unknown): string {
  if (value === null) return "null";
  switch (typeof value) {
    case "boolean":
      return value ? "true" : "false";
    case "number":
      return formatFloat(value);
    case "string":
      return JSON.stringify(value);
    case "object": {
      if (Array.isArray(value)) {
        return `[${value.map(serialize).join(",")}]`;
      }
      if (!isRecord(value)) {
        throw new IrError(`valor no serializable en HIR: ${String(value)}`);
      }
      const keys = Object.keys(value).sort(compareCodePoints);
      const parts: string[] = [];
      for (const key of keys) {
        const entry = value[key];
        if (entry === undefined) {
          throw new IrError(`clave ${key} con valor undefined en HIR`);
        }
        parts.push(`${JSON.stringify(key)}:${serialize(entry)}`);
      }
      return `{${parts.join(",")}}`;
    }
    default:
      throw new IrError(`tipo no soportado en HIR: ${typeof value}`);
  }
}

/** Serialización canónica **sin** el campo `hash` (entrada del hash). */
export function canonicalBytes(hir: Hir): Uint8Array {
  if (!isRecord(hir)) {
    throw new IrError("el HIR debe ser un objeto");
  }
  const payload: Hir = {};
  for (const [key, value] of Object.entries(hir)) {
    if (key !== "hash") payload[key] = value;
  }
  return new TextEncoder().encode(serialize(payload));
}

export function canonicalText(hir: Hir): string {
  return new TextDecoder().decode(canonicalBytes(hir));
}

/** Hash estable del HIR: BLAKE2b-512 truncado a 128 bits. */
export function hashHir(hir: Hir): string {
  const digest = createHash("blake2b512").update(canonicalBytes(hir)).digest();
  return HASH_PREFIX + digest.subarray(0, 16).toString("hex");
}

/** Copia el HIR con el hash recalculado y validado. */
export function seal(hir: Hir): Hir {
  const payload: Hir = { ...hir };
  delete payload.hash;
  validate(payload, { checkHash: false });
  payload.hash = hashHir(payload);
  return payload;
}

/** Valida schema y hash. */
export function verify(hir: Hir): void {
  validate(hir, { checkHash: true });
}

export interface ValidateOptions {
  checkHash?: boolean;
}

export function validate(hir: Hir, options: ValidateOptions = {}): void {
  const checkHash = options.checkHash ?? true;
  if (!isRecord(hir)) {
    throw new IrError("el HIR debe ser un objeto");
  }
  if (hir.schema !== IR_SCHEMA) {
    throw new IrError(`schema inesperado: ${String(hir.schema)}`);
  }

  const moduleName = hir.module;
  if (typeof moduleName !== "string" || !MODULE_RE.test(moduleName)) {
    throw new IrError(`nombre de módulo inválido: ${String(moduleName)}`);
  }

  const values = hir.values;
  const ops = hir.ops;
  const outputs = hir.outputs;
  if (!Array.isArray(values) || !Array.isArray(ops)) {
    throw new IrError("`values` y `ops` deben ser listas");
  }
  if (!Array.isArray(outputs) || outputs.length === 0) {
    throw new IrError("el HIR necesita al menos un output");
  }

  const byId = new Map<number, IrValue>();
  for (let index = 0; index < values.length; index += 1) {
    const value = values[index];
    if (!isRecord(value)) {
      throw new IrError(`value[${index}] no es un objeto`);
    }
    const id = value.id;
    if (typeof id !== "number" || !Number.isInteger(id) || id < 0) {
      throw new IrError(`value[${index}] tiene id inválido: ${String(id)}`);
    }
    if (byId.has(id)) {
      throw new IrError(`id duplicado: ${id}`);
    }
    if (typeof value.type !== "string") {
      throw new IrError(`value ${id} sin tipo`);
    }
    if (value.kind !== "param" && value.kind !== "const" && value.kind !== "op") {
      throw new IrError(`value ${id} con kind inválido: ${String(value.kind)}`);
    }
    byId.set(id, value);
  }

  const sortedIds = [...byId.keys()].sort((a, b) => a - b);
  const expectedIds = Array.from({ length: values.length }, (_, index) => index);
  if (sortedIds.length !== expectedIds.length || sortedIds.some((id, index) => id !== expectedIds[index])) {
    throw new IrError("los ids de values deben ser contiguos desde 0");
  }

  const deterministic = hir.deterministic !== false;
  for (let index = 0; index < ops.length; index += 1) {
    const op = ops[index];
    if (!isRecord(op)) {
      throw new IrError(`op[${index}] no es un objeto`);
    }
    const id = op.id;
    if (typeof id !== "number" || !Number.isInteger(id) || !byId.has(id)) {
      throw new IrError(`op[${index}] tiene id desconocido: ${String(id)}`);
    }
    const value = byId.get(id)!;
    if (value.kind !== "op") {
      throw new IrError(`op ${id} no referencia un value kind=op`);
    }
    if (value.type !== op.type) {
      throw new IrError(`op ${id}: tipo del value y de la op no coinciden`);
    }
    if (typeof op.op !== "string" || op.op.length === 0) {
      throw new IrError(`op ${id} sin nombre`);
    }
    const args = op.args;
    if (!Array.isArray(args)) {
      throw new IrError(`op ${id}: \`args\` debe ser lista`);
    }
    for (const arg of args) {
      if (typeof arg !== "number" || !Number.isInteger(arg) || !byId.has(arg)) {
        throw new IrError(`op ${id}: argumento inexistente ${String(arg)}`);
      }
      if (arg >= id) {
        throw new IrError(`op ${id}: argumento ${arg} no es SSA (debe ser anterior)`);
      }
    }
    const effects = op.effects;
    if (!Array.isArray(effects) || effects.some((effect) => typeof effect !== "string" || !ALLOWED_EFFECTS.has(effect))) {
      throw new IrError(`op ${id}: efectos inválidos ${JSON.stringify(effects)}`);
    }
    if (deterministic && effects.some((effect) => NON_DETERMINISTIC_EFFECTS.has(effect as string))) {
      throw new IrError(`op ${id} (${String(op.op)}): efectos no deterministas [${[...effects].sort().join(", ")}]`);
    }
  }

  const seenOutputs = new Set<string>();
  for (const output of outputs) {
    if (!isRecord(output)) {
      throw new IrError("output inválido");
    }
    const name = output.name;
    if (typeof name !== "string" || !MODULE_RE.test(name)) {
      throw new IrError(`nombre de output inválido: ${String(name)}`);
    }
    if (seenOutputs.has(name)) {
      throw new IrError(`output duplicado: ${name}`);
    }
    seenOutputs.add(name);
    const valueId = output.value;
    if (typeof valueId !== "number" || !byId.has(valueId)) {
      throw new IrError(`output ${name}: value inexistente ${String(valueId)}`);
    }
    if (output.type !== byId.get(valueId)!.type) {
      throw new IrError(`output ${name}: tipo inconsistente`);
    }
  }

  if (checkHash) {
    const declared = hir.hash;
    const expected = hashHir(hir);
    if (typeof declared !== "string" || declared !== expected) {
      throw new IrError(`hash inválido: declarado=${String(declared)} esperado=${expected}`);
    }
  }
}

/** Serializa el HIR completo (incluido el hash). */
export function toBytes(hir: Hir, options: { sealIt?: boolean } = {}): Uint8Array {
  const sealIt = options.sealIt ?? true;
  const payload = sealIt ? seal(hir) : { ...hir };
  return new TextEncoder().encode(serialize(payload));
}

export function fromBytes(blob: Uint8Array, options: { verifyIt?: boolean } = {}): Hir {
  const verifyIt = options.verifyIt ?? true;
  let parsed: unknown;
  try {
    parsed = JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(blob));
  } catch (error) {
    throw new IrError(`HIR no es JSON UTF-8 válido: ${String(error)}`);
  }
  if (!isRecord(parsed)) {
    throw new IrError("el HIR raíz debe ser un objeto");
  }
  if (verifyIt) verify(parsed);
  return parsed;
}
