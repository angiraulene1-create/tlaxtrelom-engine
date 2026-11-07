# SPDX-License-Identifier: LicenseRef-Tlaxtrelom-Proprietary
# Copyright (c) 2026 Tlaxtrelom Engine. All rights reserved.
# CONFIDENTIAL — internal use only.
"""Frontend de nodos visuales → Tlaxtrelom HIR.

Un grafo es un **dataflow DAG** con puertos tipados. Cada nodo es un *intrinsic*
registrado; el grafo no tiene semántica propia: se baja a HIR y el compilador
de Rust hace el resto (typeck, efectos, determinismo, SPIR-V/LLVM).

Reglas:
- Sin ciclos: el feedback se expresa con nodos `state`/`delay` (no incluidos en
  esta demo) que el lowering convierte en memoria explícita.
- Los tipos se verifican en tiempo de construcción, con mensajes que incluyen
  el nodo y el puerto.
- Los efectos se declaran por nodo; un grafo determinista no puede contener
  `neural`, `io` o `nondet`.

Este módulo es Python puro: se puede usar en el editor, en CI y en tests sin
cargar el runtime nativo.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any, Callable, Iterable
import json

from .ir import hash_hir


class T:
    """Tipos canónicos del IR. En producción son un enum + schema FlatBuffers."""

    BOOL = "bool"
    I32 = "i32"
    F32 = "f32"
    VEC2 = "vec2<f32>"
    VEC3 = "vec3<f32>"
    VEC4 = "vec4<f32>"
    MAT4 = "mat4<f32>"
    ENTITY = "entity"
    ANY = "any"


@dataclass(frozen=True, slots=True)
class Port:
    name: str
    type: str
    default: Any = None


@dataclass(frozen=True, slots=True)
class NodeDef:
    op: str
    inputs: tuple[Port, ...]
    outputs: tuple[Port, ...]
    effects: frozenset[str] = frozenset({"pure"})
    doc: str = ""


_NODES: dict[str, NodeDef] = {}

# Promociones numéricas permitidas. Se materializan como un nodo `cast` para
# que el HIR sea explícito (nada de coerciones invisibles en el backend).
_PROMOTE: dict[tuple[str, str], str] = {
    (T.I32, T.F32): T.F32,
    (T.F32, "f64"): "f64",
}


def node(
    op: str,
    *,
    inputs: Iterable[Port],
    outputs: Iterable[Port],
    effects: Iterable[str] = ("pure",),
    doc: str = "",
) -> Callable[[Callable[..., Any]], Callable[..., Any]]:
    """Registra una función Python como nodo del editor.

    La función sigue siendo ejecutable (útil para el intérprete del editor),
    pero el grafo solo usa sus metadatos. El backend real es el HIR.
    """

    def register(fn: Callable[..., Any]) -> Callable[..., Any]:
        definition = NodeDef(op, tuple(inputs), tuple(outputs), frozenset(effects), doc)
        existing = _NODES.get(op)
        if existing is not None:
            # Reimportar un módulo (o ejecutarlo con `python -m`) no debe
            # romper el registro si la definición es idéntica.
            if existing != definition:
                raise ValueError(f"nodo duplicado con definición distinta: {op}")
        else:
            _NODES[op] = definition
        setattr(fn, "__tlaxtrelom_op__", op)
        setattr(fn, "__tlaxtrelom_def__", definition)
        return fn

    return register


# ─────────────────────────────────────────────────────────────────────────────
# Librería de nodos de referencia
# ─────────────────────────────────────────────────────────────────────────────

@node(
    "math.add",
    inputs=(Port("a", T.F32), Port("b", T.F32)),
    outputs=(Port("out", T.F32),),
)
def add(a: float, b: float) -> float:
    return a + b


@node(
    "math.mul",
    inputs=(Port("a", T.F32), Port("b", T.F32)),
    outputs=(Port("out", T.F32),),
)
def mul(a: float, b: float) -> float:
    return a * b


@node(
    "math.lerp",
    inputs=(Port("a", T.F32), Port("b", T.F32), Port("t", T.F32)),
    outputs=(Port("out", T.F32),),
)
def lerp(a: float, b: float, t: float) -> float:
    return a + (b - a) * t


@node(
    "math.clamp",
    inputs=(Port("x", T.F32), Port("lo", T.F32), Port("hi", T.F32)),
    outputs=(Port("out", T.F32),),
)
def clamp(x: float, lo: float, hi: float) -> float:
    return max(lo, min(hi, x))


@node(
    "vec3.compose",
    inputs=(Port("x", T.F32), Port("y", T.F32), Port("z", T.F32)),
    outputs=(Port("out", T.VEC3),),
)
def vec3_compose(x: float, y: float, z: float) -> tuple[float, float, float]:
    return (x, y, z)


@node(
    "vec3.scale",
    inputs=(Port("v", T.VEC3), Port("s", T.F32)),
    outputs=(Port("out", T.VEC3),),
)
def vec3_scale(v: tuple[float, float, float], s: float) -> tuple[float, float, float]:
    return (v[0] * s, v[1] * s, v[2] * s)


@node(
    "vec3.add",
    inputs=(Port("a", T.VEC3), Port("b", T.VEC3)),
    outputs=(Port("out", T.VEC3),),
)
def vec3_add(
    a: tuple[float, float, float], b: tuple[float, float, float]
) -> tuple[float, float, float]:
    return (a[0] + b[0], a[1] + b[1], a[2] + b[2])


@node(
    "math.select",
    inputs=(Port("cond", T.BOOL), Port("a", T.ANY), Port("b", T.ANY)),
    outputs=(Port("out", T.ANY),),
)
def select(cond: bool, a: Any, b: Any) -> Any:
    return a if cond else b


@node(
    "neural.read",
    inputs=(Port("channel", T.I32),),
    outputs=(Port("out", T.F32),),
    effects=("neural",),
    doc="Lee un eje del IntentFrame. Prohibido en sistemas deterministas.",
)
def neural_read(channel: int) -> float:
    raise RuntimeError("neural.read solo se ejecuta en el runtime nativo")


# ─────────────────────────────────────────────────────────────────────────────
# Grafo
# ─────────────────────────────────────────────────────────────────────────────

@dataclass(frozen=True, slots=True)
class Value:
    id: int
    type: str
    name: str | None = None


class Graph:
    """Constructor de grafos. Usar como context manager:

    >>> with Graph("demo") as g:
    ...     x = g.input("x", T.F32)
    ...     g.output("y", g.call(add, a=x, b=g.const(1.0)))
    """

    def __init__(
        self,
        name: str,
        *,
        deterministic: bool = True,
        effects: Iterable[str] = ("pure",),
    ) -> None:
        if not name.isidentifier():
            raise ValueError(f"nombre de módulo inválido: {name!r}")
        self.name = name
        self.deterministic = deterministic
        self.effects = frozenset(effects)
        self._values: list[dict[str, Any]] = []
        self._ops: list[dict[str, Any]] = []
        self._outputs: list[dict[str, Any]] = []
        self._next = 0
        self._closed = False

    # -- ciclo de vida -------------------------------------------------------

    def __enter__(self) -> "Graph":
        return self

    def __exit__(self, exc_type: Any, exc: Any, tb: Any) -> bool:
        if exc_type is None:
            self._validate()
            self._closed = True
        return False

    # -- construcción --------------------------------------------------------

    def input(self, name: str, type: str) -> Value:
        if not name.isidentifier():
            raise ValueError(f"nombre de input inválido: {name!r}")
        return self._new_value("param", type, name=name)

    def const(self, value: Any, type: str | None = None) -> Value:
        if type is None:
            type = _infer_type(value)
        return self._new_value("const", type, value=value)

    def call(self, fn: Any, **kwargs: Any) -> Value:
        op = getattr(fn, "__tlaxtrelom_op__", fn)
        definition = _NODES.get(op)
        if definition is None:
            raise KeyError(f"nodo no registrado: {op!r}")

        args: list[int] = []
        for port in definition.inputs:
            if port.name in kwargs:
                value = kwargs.pop(port.name)
            elif port.default is not None:
                value = self.const(port.default, port.type)
            else:
                raise TypeError(f"{op}: falta el puerto de entrada '{port.name}'")

            if not isinstance(value, Value):
                raise TypeError(
                    f"{op}.{port.name}: se esperaba un Value, llegó {type(value).__name__}"
                )
            value = self._coerce(value, port.type, op, port.name)
            args.append(value.id)

        if kwargs:
            raise TypeError(f"{op}: puertos desconocidos {sorted(kwargs)}")

        out_type = definition.outputs[0].type if definition.outputs else "void"
        if out_type == T.ANY:
            # Un `select`/`any` hereda el tipo de su primer argumento real.
            out_type = self._values[args[1]]["type"] if len(args) > 1 else T.ANY

        return self._emit(op, out_type, args, definition.effects)

    def output(self, name: str, value: Value) -> None:
        if not isinstance(value, Value):
            raise TypeError("output requiere un Value")
        if any(o["name"] == name for o in self._outputs):
            raise ValueError(f"output duplicado: {name}")
        self._outputs.append({"name": name, "value": value.id, "type": value.type})

    # -- lowering ------------------------------------------------------------

    def lower(self) -> dict[str, Any]:
        if not self._closed:
            self._validate()
            self._closed = True

        payload: dict[str, Any] = {
            "schema": "tlaxtrelom.ir/1",
            "module": self.name,
            "entry": "main",
            "deterministic": self.deterministic,
            "values": self._values,
            "ops": self._ops,
            "outputs": self._outputs,
        }
        payload["hash"] = hash_hir(payload)
        return payload

    # -- internos ------------------------------------------------------------

    def _new_value(
        self,
        kind: str,
        type: str,
        *,
        name: str | None = None,
        value: Any = None,
    ) -> Value:
        vid = self._next
        self._next += 1
        record: dict[str, Any] = {"id": vid, "type": type, "kind": kind}
        if name is not None:
            record["name"] = name
        if value is not None:
            record["value"] = value
        self._values.append(record)
        return Value(vid, type, name)

    def _emit(
        self, op: str, out_type: str, args: list[int], effects: frozenset[str]
    ) -> Value:
        vid = self._next
        self._next += 1
        self._values.append({"id": vid, "type": out_type, "kind": "op"})
        self._ops.append(
            {
                "id": vid,
                "op": op,
                "args": args,
                "type": out_type,
                "effects": sorted(effects),
            }
        )
        return Value(vid, out_type)

    def _coerce(self, value: Value, target: str, op: str, port: str) -> Value:
        if target == T.ANY or value.type == target:
            return value
        if (value.type, target) in _PROMOTE:
            return self._emit("cast", target, [value.id], frozenset({"pure"}))
        raise TypeError(
            f"{op}.{port}: tipo incompatible {value.type!r} → {target!r}"
        )

    def _validate(self) -> None:
        if not self._outputs:
            raise ValueError(f"grafo {self.name!r} sin outputs")
        if self.deterministic:
            for op in self._ops:
                bad = set(op["effects"]) & {"neural", "nondet", "io"}
                if bad:
                    raise ValueError(
                        f"grafo determinista {self.name!r} contiene {op['op']} "
                        f"con efectos {sorted(bad)}; usá un gate explícito"
                    )

    def __repr__(self) -> str:
        return f"Graph({self.name!r}, values={len(self._values)}, ops={len(self._ops)})"


def _infer_type(value: Any) -> str:
    if isinstance(value, bool):
        return T.BOOL
    if isinstance(value, int):
        return T.I32
    if isinstance(value, float):
        return T.F32
    if isinstance(value, (tuple, list)) and len(value) == 3:
        return T.VEC3
    raise TypeError(f"no se puede inferir el tipo de {value!r}")


# ─────────────────────────────────────────────────────────────────────────────
# Demo ejecutable sin runtime nativo
# ─────────────────────────────────────────────────────────────────────────────

if __name__ == "__main__":
    with Graph("player_locomotion") as g:
        speed = g.input("speed", T.F32)
        forward = g.call(
            vec3_compose, x=g.const(0.0), y=g.const(0.0), z=g.const(1.0)
        )
        velocity = g.call(vec3_scale, v=forward, s=speed)
        g.output("velocity", velocity)

    print(json.dumps(g.lower(), indent=2))
