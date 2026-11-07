# SPDX-License-Identifier: LicenseRef-Tlaxtrelom-Proprietary
# Copyright (c) 2026 Tlaxtrelom Engine. All rights reserved.
# CONFIDENTIAL — internal use only.
"""Forma canónica y validación del Tlaxtrelom HIR.

El HIR es el contrato entre los frontends (texto, nodos, bloques, Lua) y los
backends. Este módulo define su representación *canónica* en Python:

- JSON con claves ordenadas y sin espacios, UTF-8.
- Números enteros normalizados (``1.0`` y ``1`` producen el mismo byte).
- Hash BLAKE2b-512 truncado a 128 bits, con prefijo ``blake2b512-128:``.

El mismo algoritmo está implementado en ``tools/tlaxtrelom-ts`` para que un
proyecto editable en el editor y validado por la CLI tenga el mismo ID.

Este módulo es puro: no importa el runtime nativo.
"""

from __future__ import annotations

import copy
import hashlib
import json
import re
from typing import Any, Mapping

IR_SCHEMA = "tlaxtrelom.ir/1"
HASH_PREFIX = "blake2b512-128:"
ALLOWED_EFFECTS = frozenset({"pure", "write", "neural", "io", "nondet"})
NON_DETERMINISTIC_EFFECTS = frozenset({"neural", "io", "nondet"})
_MODULE_RE = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*$")


class IrError(ValueError):
    """El HIR no cumple el schema o su hash no corresponde."""


def _normalize(value: Any) -> Any:
    """Normaliza para que Python y JavaScript produzcan el mismo JSON."""
    if isinstance(value, float):
        if value != value or value in (float("inf"), float("-inf")):
            raise IrError(f"constante no finita en HIR: {value!r}")
        if value.is_integer():
            return int(value)
        return value
    if isinstance(value, Mapping):
        return {str(k): _normalize(v) for k, v in value.items()}
    if isinstance(value, (list, tuple)):
        return [_normalize(v) for v in value]
    return value


def canonical_bytes(hir: Mapping[str, Any]) -> bytes:
    """Serialización canónica sin el campo ``hash`` (entrada del hash)."""
    payload = {k: v for k, v in hir.items() if k != "hash"}
    return _dumps(payload)


def _dumps(value: Any) -> bytes:
    canonical = json.dumps(
        _normalize(value),
        sort_keys=True,
        separators=(",", ":"),
        ensure_ascii=False,
        allow_nan=False,
    )
    return canonical.encode("utf-8")


def hash_hir(hir: Mapping[str, Any]) -> str:
    """Hash estable del HIR (BLAKE2b-512 truncado a 128 bits)."""
    digest = hashlib.blake2b(canonical_bytes(hir), digest_size=64).digest()[:16]
    return HASH_PREFIX + digest.hex()


def seal(hir: Mapping[str, Any]) -> dict[str, Any]:
    """Devuelve una copia del HIR con el hash recalculado y validado."""
    payload = copy.deepcopy(dict(hir))
    payload.pop("hash", None)
    validate(payload, check_hash=False)
    payload["hash"] = hash_hir(payload)
    return payload


def verify(hir: Mapping[str, Any]) -> None:
    """Valida schema *y* hash. Lanza :class:`IrError` si algo no cuadra."""
    validate(hir, check_hash=True)


def validate(hir: Mapping[str, Any], *, check_hash: bool = True) -> None:
    if not isinstance(hir, Mapping):
        raise IrError("el HIR debe ser un objeto (dict)")
    if hir.get("schema") != IR_SCHEMA:
        raise IrError(f"schema inesperado: {hir.get('schema')!r}")

    module = hir.get("module")
    if not isinstance(module, str) or not _MODULE_RE.match(module):
        raise IrError(f"nombre de módulo inválido: {module!r}")

    values = hir.get("values")
    ops = hir.get("ops")
    outputs = hir.get("outputs")
    if not isinstance(values, list) or not isinstance(ops, list):
        raise IrError("`values` y `ops` deben ser listas")
    if not isinstance(outputs, list) or not outputs:
        raise IrError("el HIR necesita al menos un output")

    by_id: dict[int, Mapping[str, Any]] = {}
    for index, value in enumerate(values):
        if not isinstance(value, Mapping):
            raise IrError(f"value[{index}] no es un objeto")
        vid = value.get("id")
        if not isinstance(vid, int) or isinstance(vid, bool) or vid < 0:
            raise IrError(f"value[{index}] tiene id inválido: {vid!r}")
        if vid in by_id:
            raise IrError(f"id duplicado: {vid}")
        if not isinstance(value.get("type"), str):
            raise IrError(f"value {vid} sin tipo")
        if value.get("kind") not in {"param", "const", "op"}:
            raise IrError(f"value {vid} con kind inválido: {value.get('kind')!r}")
        by_id[vid] = value

    if sorted(by_id) != list(range(len(values))):
        raise IrError("los ids de values deben ser contiguos desde 0")

    deterministic = bool(hir.get("deterministic", True))
    op_ids: set[int] = set()
    for index, op in enumerate(ops):
        if not isinstance(op, Mapping):
            raise IrError(f"op[{index}] no es un objeto")
        oid = op.get("id")
        if not isinstance(oid, int) or isinstance(oid, bool) or oid not in by_id:
            raise IrError(f"op[{index}] tiene id desconocido: {oid!r}")
        value = by_id[oid]
        if value.get("kind") != "op":
            raise IrError(f"op {oid} no referencia un value kind=op")
        if value.get("type") != op.get("type"):
            raise IrError(f"op {oid}: tipo del value y de la op no coinciden")
        if not isinstance(op.get("op"), str) or not op["op"]:
            raise IrError(f"op {oid} sin nombre")
        args = op.get("args")
        if not isinstance(args, list):
            raise IrError(f"op {oid}: `args` debe ser lista")
        for arg in args:
            if not isinstance(arg, int) or isinstance(arg, bool) or arg not in by_id:
                raise IrError(f"op {oid}: argumento inexistente {arg!r}")
            if arg >= oid:
                raise IrError(f"op {oid}: argumento {arg} no es SSA (debe ser anterior)")
        effects = op.get("effects")
        if not isinstance(effects, list) or any(e not in ALLOWED_EFFECTS for e in effects):
            raise IrError(f"op {oid}: efectos inválidos {effects!r}")
        if deterministic and NON_DETERMINISTIC_EFFECTS.intersection(effects):
            raise IrError(
                f"op {oid} ({op['op']}): efectos no deterministas {sorted(effects)}"
            )
        op_ids.add(oid)

    seen_outputs: set[str] = set()
    for output in outputs:
        if not isinstance(output, Mapping):
            raise IrError("output inválido")
        name = output.get("name")
        if not isinstance(name, str) or not _MODULE_RE.match(name):
            raise IrError(f"nombre de output inválido: {name!r}")
        if name in seen_outputs:
            raise IrError(f"output duplicado: {name}")
        seen_outputs.add(name)
        value_id = output.get("value")
        if value_id not in by_id:
            raise IrError(f"output {name}: value inexistente {value_id!r}")
        if output.get("type") != by_id[value_id].get("type"):
            raise IrError(f"output {name}: tipo inconsistente")

    if check_hash:
        declared = hir.get("hash")
        expected = hash_hir(hir)
        if not isinstance(declared, str) or declared != expected:
            raise IrError(f"hash inválido: declarado={declared!r} esperado={expected!r}")


def to_bytes(hir: Mapping[str, Any], *, seal_it: bool = True) -> bytes:
    """Serializa el HIR completo (incluido el hash)."""
    payload = seal(hir) if seal_it else dict(hir)
    return _dumps(payload)


def from_bytes(blob: bytes, *, verify_it: bool = True) -> dict[str, Any]:
    try:
        payload = json.loads(blob.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise IrError(f"HIR no es JSON UTF-8 válido: {exc}") from exc
    if not isinstance(payload, dict):
        raise IrError("el HIR raíz debe ser un objeto")
    if verify_it:
        verify(payload)
    return payload
