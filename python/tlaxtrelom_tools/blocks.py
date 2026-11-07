# SPDX-License-Identifier: LicenseRef-Tlaxtrelom-Proprietary
# Copyright (c) 2026 Tlaxtrelom Engine. All rights reserved.
# CONFIDENTIAL — internal use only.
"""Sistema de bloques apilables (estilo Scratch) → grafo → HIR.

Los bloques **no tienen semántica propia**: son azúcar sobre el grafo de nodos.
Eso garantiza que un proyecto pueda migrar de bloques a nodos sin reescribir y
que exista un solo backend.

Estrategia de lowering:
- La secuencia se modela con `flow.seq` (efecto `write`).
- Los bucles con `times` constante se **desenrollan** en SSA (rápido y
  predecible; ideal para prototipado por IA).
- Los condicionales con efectos requieren el pase de *state machine* del
  compilador; aquí solo se expone `if_` para valores puros (`math.select`).
- Variables: SSA en Python; el editor las muestra como "variables" con nombre.

Limitación deliberada: este frontend prioriza claridad y velocidad de
iteración, no expresividad total. Para lógica compleja, el usuario migra a
nodos o a texto.
"""

from __future__ import annotations

from typing import Any, Callable
import json

from .graph import Graph, T, Value, node, Port, select


@node(
    "flow.seq",
    inputs=(Port("a", T.ANY), Port("b", T.ANY)),
    outputs=(Port("out", T.ANY),),
    effects=("write",),
    doc="Ejecuta a y luego b; devuelve b. El orden es explícito.",
)
def seq(a: Any, b: Any) -> Any:
    return b


@node(
    "flow.emit",
    inputs=(Port("code", T.I32),),
    outputs=(Port("out", T.I32),),
    effects=("write",),
    doc="Emite un evento al runtime (código entero estable).",
)
def emit(code: int) -> int:
    return code


class Script:
    """Constructor imperativo de bloques.

    >>> s = Script("turret")
    >>> s.emit(1)
    >>> s.repeat(3, lambda: s.emit(2))
    >>> hir = s.finish()
    """

    def __init__(self, name: str) -> None:
        self.graph = Graph(name, deterministic=False, effects=("write",))
        self._vars: dict[str, Value] = {}
        self._last: Value | None = None

    # -- variables -----------------------------------------------------------

    def var(self, name: str, initial: Any) -> Value:
        if name in self._vars:
            raise ValueError(f"variable duplicada: {name}")
        value = self.graph.const(initial)
        self._vars[name] = value
        return value

    def get(self, name: str) -> Value:
        try:
            return self._vars[name]
        except KeyError:
            raise KeyError(f"variable no definida: {name}") from None

    def set(self, name: str, value: Value) -> None:
        if name not in self._vars:
            raise KeyError(f"variable no definida: {name}")
        self._vars[name] = value
        self._append(value)

    # -- bloques -------------------------------------------------------------

    def emit(self, code: int) -> None:
        self._append(self.graph.call(emit, code=self.graph.const(code)))

    def repeat(self, times: int, body: Callable[[], None]) -> None:
        if times < 0:
            raise ValueError("repeat requiere times >= 0")
        if times > 64:
            raise ValueError(
                "repeat > 64 se desenrollaría demasiado; usá un nodo de loop"
            )
        for _ in range(times):
            body()

    def if_(self, cond: Value, then_fn: Callable[[], Value],
            else_fn: Callable[[], Value]) -> Value:
        """Condicional puro. Los efectos en ramas van por state machine."""
        a = then_fn()
        b = else_fn()
        return self.graph.call(select, cond=cond, a=a, b=b)

    # -- cierre --------------------------------------------------------------

    def finish(self) -> dict[str, Any]:
        if self._last is not None:
            self.graph.output("flow", self._last)
        return self.graph.lower()

    def _append(self, value: Value) -> None:
        if self._last is None:
            self._last = value
        else:
            self._last = self.graph.call(seq, a=self._last, b=value)


if __name__ == "__main__":
    script = Script("turret")
    script.emit(1)
    script.repeat(3, lambda: script.emit(2))
    print(json.dumps(script.finish(), indent=2))
