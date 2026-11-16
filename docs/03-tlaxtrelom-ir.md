<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 2026 angiraulene1-create -->

# Tlaxtrelom IR — Un AST para tres paradigmas de autoría

> Versión 0.2 · Alcance: tipos, efectos, HIR/MIR, frontends (texto, nodos,
> bloques), backends, passes y determinismo.

**Cómo leer este documento.** Cada sección separa explícitamente:

- **Implementado**: código que existe hoy en el repositorio. El layer canónico
  HIR vive en `python/tlaxtrelom_tools/ir.py` (validador, canonicalización y
  hash) y su gemelo byte-exacto en `tools/tlaxtrelom-ts/src/ir.ts`; los
  frontends de nodos y bloques están en `python/tlaxtrelom_tools/graph.py` y
  `python/tlaxtrelom_tools/blocks.py`; el builder de TS en
  `tools/tlaxtrelom-ts/src/builder.ts`; las CLI equivalentes en
  `python/tlaxtrelom_tools/cli.py` y `tools/tlaxtrelom-ts/src/cli.ts`.
- **Destino**: contrato de diseño sin código aún. No existen en este repo el
  MIR, el lowering HIR→MIR→SPIR-V/LLVM, ningún crate `tlaxtrelom-ir` o
  `tlaxtrelom-bindings`, proc-macros de kernel, plugin de Clang ni backend de
  shaders generado desde el IR (los `.spv` actuales se cocinan con `glslc`
  desde GLSL escrito a mano vía `cargo xtask shaders`).

---

## 0. La verdad incómoda

"Que Rust, C++ y Python compilen al mismo AST" suena a que se puede tomar
cualquier archivo `.cpp`, `.rs` o `.py` y bajarlo a un IR común. **No se puede**
sin reimplementar un compilador de C++ (plantillas, UB, sobrecarga, macros) y la
semántica dinámica de Python (introspección, `eval`, monkey patching). Cualquier
proyecto que prometa eso está vendiendo humo o redefiniendo "C++" y "Python".

Lo que **sí** se puede —y es lo que hace Tlaxtrelom— es definir un **subconjunto de
kernel**:

- tipado estáticamente,
- determinista,
- sin punteros crudos ni *aliasing*,
- sin excepciones ni *unwind*,
- sin introspección ni *dynamic dispatch*,
- con efectos declarados (`pure`, `write`, `io`, `neural`, `nondet`).

Cada lenguaje expresa ese subconjunto con anotaciones:

```rust
#[tlaxtrelom::kernel]
fn fresnel(cos_theta: f32, ior: f32) -> f32 { ... }
```

```cpp
[[tlaxtrelom::kernel]] float fresnel(float cos_theta, float ior) { ... }
```

```python
@tlaxtrelom.kernel
def fresnel(cos_theta: f32, ior: f32) -> f32: ...
```

El código **host** (lo que no es kernel) sigue siendo nativo y llama al runtime
por ABI. Así se obtiene el 95% del valor (shaders, sistemas de gameplay,
simulación) sin prometer magia. Los kernels son el 5–20% del código y el 80%
del coste de rendimiento: ahí es donde el IR compartido paga.

**Estado**: el subconjunto de kernel descrito es *destino* como compilador: no
hay proc-macro `#[tlaxtrelom::kernel]`, ni plugin de Clang, ni decorador
Python que emita HIR desde código fuente. Lo que **sí existe hoy** es la forma
canónica del HIR y su validador (Python y TypeScript), los frontends de nodos
y bloques que lo construyen, mientras que las anotaciones de ejemplo de arriba
son el contrato de cómo se verá el frontend de texto.

---

## 1. Objetivos de diseño

1. **Un solo contrato semántico** para los tres frontends.
2. **Tipos explícitos** y verificados antes de generar código.
3. **Efectos** para que el compilador impida, por ejemplo, un `neural` read
   dentro de un sistema determinista de red.
4. **Determinismo opt-in** verificable (sin FP fast-math, sin orden no
   determinista de reducciones).
5. **Bajar a GPU y CPU** sin duplicar la lógica de negocio.
6. **Versionado estable** para cachear shaders y assets entre versiones.
7. **Mensajes de error con origen**: cada nodo del grafo conserva su `span`
   (archivo/línea o coordenadas del canvas).

**Estado**: los puntos 1–4 y 6 son **implementados** a nivel de HIR: el mismo
validador corre en Python y TypeScript, los efectos y la bandera
`deterministic` se verifican contra el set canónico, y el schema
`tlaxtrelom.ir/1` más el hash BLAKE2b dan versionado y cacheabilidad por
contenido. Los puntos 5 (lowering a GPU/CPU) y 7 (spans por nodo) son
**destino**: el HIR actual no tiene campo `span` y ningún backend lee MIR.

---

## 2. Sistema de tipos

```text
escalares:   bool, i8, i16, i32, i64, u8..u64, f16, f32, f64
vectores:    vec2<T>, vec3<T>, vec4<T>
matrices:    mat2<T>..mat4<T>, quat<T>
texturas:    tex2d<T>, tex3d<T>, texCube<T>, sampler
buffers:     buffer<T, space>, storage<T>
tensores:    tensor<T, shape>          // ML y simulación
dominio:     entity, page, cluster, signal<T, N>
compuestos:  option<T>, result<T, E>, array<T, N>, struct { ... }
```

Reglas:

- **Sin conversiones implícitas con pérdida.** `i32 → f32` requiere `cast`.
- **Sin `null`.** `option<T>` explícito; en GPU se baja a un valor centinela
  documentado.
- **Alineación explícita** en structs que cruzan a GPU (`#[repr(C, align(16))]`).
- **Genéricos monomorfizados** en el backend; nada de *vtables* en kernels.

**Implementado**: el validador canónico trata `type` como **string opaco** y
solo exige coherencia (el tipo de una `op` coincide con su `value`, el tipo de
un `output` coincide con el `value` referenciado). Los tipos concretos que los
frontends producen hoy son los de `tlaxtrelom_tools.graph.T` (Python: `bool`,
`i32`, `f32`, `vec2<f32>`, `vec3<f32>`, `vec4<f32>`, `mat4<f32>`, `entity`,
`any`) y `builder.ts` (TS: `bool`, `i32`, `i64`, `f32`, `f64`, `vec3<f32>`).
La regla "sin conversiones implícitas con pérdida" está implementada como
tabla de promociones (`_PROMOTE`: `i32→f32`, `f32→f64`) que materializa un
nodo `cast` explícito en el HIR.

**Destino**: texturas, buffers, tensores, `option`/`result`, alineación
`repr(C)` y monomorfización; ninguno tiene constructor ni checker aún, y no
hay una enum de tipos compartida entre lenguajes (solo literals de string).

---

## 3. Efectos

```text
pure      no lee ni escribe estado; puede plegarse/duplicarse
write     escribe un recurso declarado
io        filesystem/red; prohibido en GPU y en sim determinista
neural    lee IntentFrame; prohibido en sistemas de red autoritativos
nondet    reloj/RNG sin semilla; prohibido en replay determinista
```

La lectura de buffers/texturas no se modela hoy como efecto separado: el
validador de HIR solo acepta los cinco efectos anteriores. Añadir `read` o
`gpu` exige subir la versión del schema y actualizar el set en Python y
TypeScript en el mismo cambio (no existe validador de efectos en Rust: el
crate `tlaxtrelom-ir` está en el diagrama de `docs/01-architecture.md` como
destino, no en `crates/`).

El *effect checker* es lo que permite que un mismo grafo se ejecute en el editor
(interpreter), en GPU (SPIR-V) y en el servidor (LLVM) con garantías distintas.
Ejemplo: un nodo `neural.read` no puede entrar en un `system` marcado
`deterministic = true` sin pasar por un `gate` que lo convierta en input
explícito.

**Implementado**: el control de efectos hoy es el validador de HIR
(`ALLOWED_EFFECTS` / `NON_DETERMINISTIC_EFFECTS` en `ir.py` y `ir.ts`) más el
chequeo en construcción del grafo (`Graph._validate` en Python, y en TS
`builder.ts` → `HirBuilder.build` → `seal` → `validate`). Concretamente: cada
`op` declara su lista de efectos; cualquier efecto fuera del set de cinco se
rechaza en Python (`IrError`) y en TypeScript (`throw new IrError(...)`); y un
módulo `deterministic` (por defecto `true`) rechaza `neural`, `io` y `nondet`.
**Destino**: el checker semántico profundo (pureza por aliasing, recursos de
GPU, gates automáticos) y su ejecución en interpreter/SPIR-V/LLVM, que no
existen todavía.

---

## 4. HIR (High-level IR)

**Implementado** (Python `python/tlaxtrelom_tools/ir.py`, TypeScript
`tools/tlaxtrelom-ts/src/ir.ts`, gemelos byte-exactos):

- **SSA** (static single assignment), tipado, con efectos por operación.
- Estructura de nivel superior (todas las claves son parte del contrato):

| Campo | Tipo | Regla |
|---|---|---|
| `schema` | string | debe ser exactamente `"tlaxtrelom.ir/1"`; el validador rechaza lo demás |
| `module` | string | identificador `^[A-Za-z_][A-Za-z0-9_]*$` |
| `entry` | string | hoy siempre `"main"` (los frontends lo fijan) |
| `deterministic` | bool | default `true` si falta; habilita el gate de efectos |
| `values` | lista | SSA: `{id, type, kind}` con `kind ∈ {"param","const","op"}`, `name` opcional en params, `value` opcional en consts |
| `ops` | lista | `{id, op, args, type, effects}`; `id` referencia un `value` de `kind="op"`; `args` son ids estrictamente anteriores; `effects` es lista de strings del set canónico (Python la emite ordenada con `sorted()`) |
| `outputs` | lista | `{name, value, type}`, al menos uno, nombres únicos y tipos coherentes con el `value` referenciado |
| `hash` | string | `blake2b512-128:` + hex, calculado sobre el HIR **sin** este campo |

- Serialización canónica: **JSON UTF-8 con claves ordenadas por punto de
  código y sin espacios** (`sort_keys=True, separators=(",", ":")` en Python;
  orden por code point explícito en TS). No se usa `pickle` ni un formato
  binario dependiente de una versión de librería: el contrato son los bytes.
- Normalizaciones para que Python y TS producan los mismos bytes: enteros de
  punto flotante normalizados (`1.0` y `1` dan el mismo byte), `-0 → 0`,
  floats no finitos rechazados, y en TS los floats se formatean con las
  reglas de `repr()` de CPython (umbrales de notación científica `1e-4`/`1e16`,
  no los de `JSON.stringify`), con dígitos exactos vía `BigInt` para `|x| ≥ 1e21`.
- Hash: BLAKE2b-**512** sobre los bytes canónicos sin `hash`, **truncado a los
  primeros 128 bits** (16 bytes), prefijo `blake2b512-128:`. En Python vía
  `hashlib.blake2b(digest_size=64)`; en TS vía `node:crypto`
  `createHash("blake2b512")`.
- API pública en ambos lenguajes: `canonical_bytes`/`canonicalBytes`,
  `hash_hir`/`hashHir`, `seal`, `verify`/`validate`,
  `to_bytes`/`from_bytes` (`to_bytes` sella; `from_bytes` verifica).

### 4.1 Ejemplo

El grafo de referencia `player_locomotion` (construido por `demo_graph()` en
`cli.py` y `demoHir()` en `builder.ts`):

```json
{"deterministic":true,"entry":"main","module":"player_locomotion","ops":[{"args":[1,2,3],"effects":["pure"],"id":4,"op":"vec3.compose","type":"vec3<f32>"},{"args":[4,0],"effects":["pure"],"id":5,"op":"vec3.scale","type":"vec3<f32>"}],"outputs":[{"name":"velocity","type":"vec3<f32>","value":5}],"schema":"tlaxtrelom.ir/1","values":[{"id":0,"kind":"param","name":"speed","type":"f32"},{"id":1,"kind":"const","type":"f32","value":0},{"id":2,"kind":"const","type":"f32","value":0},{"id":3,"kind":"const","type":"f32","value":1},{"id":4,"kind":"op","type":"vec3<f32>"},{"id":5,"kind":"op","type":"vec3<f32>"}]}
```

El hash de ese HIR (sin el campo `hash`) es
`blake2b512-128:b2861452eb9b8267652b896c81d00774`, idéntico en Python y
TypeScript; es el fixture `DEMO_HASH` de `tools/tlaxtrelom-ts/src/builder.ts`
y lo cruza el selftest de la CLI TS (`python_parity`).

### 4.2 Invariantes

Verificadas hoy por `validate()` en ambos lenguajes:

- Los ids de `values` son contiguos desde 0 y sin duplicados; cada `value`
  tiene `type` string y `kind` válido.
- Cada `op` referencia un `value` de `kind="op"` con el mismo tipo; sus
  `args` existen y son estrictamente anteriores (SSA sin ciclos).
- Toda operación declara sus efectos dentro del set de cinco; un módulo
  `deterministic` no admite `neural`, `io` ni `nondet`.
- Hay al menos un output, sin nombres duplicados, con tipo coherente.
- El hash cubre el HIR completo salvo el propio campo `hash`; cualquier cambio
  de contenido cambia el ID (`verify` lo rechaza; el selftest TS lo prueba con
  `tamper_detection`).

**Destino**: "todo valor tiene un tipo concreto tras la inferencia" (hoy el
tipo se declara al construir, no se infiere), los nodos `state`/`delay` para
feedback bajados a memoria explícita, y la declaración de orden en
reducciones: ninguno de esos conceptos existe en el schema `tlaxtrelom.ir/1`.

---

## 5. MIR (Mid-level IR)

- Bloques básicos, control flow explícito, memoria (load/store), llamadas.
- **Sin azúcar**: nada de `for`, `match`, `async`; todo es `br`, `switch`, `phi`.
- Optimizaciones: inline, DCE, LICM, vectorización, *kernel fusion*, eliminación
  de bounds checks demostrables.
- Backends leen MIR, no HIR.

```text
fn main(speed: f32, velocity: vec3) -> vec3 {
entry:
  %0 = vec3(0, 0, 1)
  %1 = fmul %0, speed
  %2 = fadd %1, velocity
  ret %2
}
```

**Toda esta sección es destino**: no existe codificador ni parser MIR en el
repo (ni en Rust, ni en Python, ni en TS), ni un pase HIR→MIR. El bloque de
arriba es un ejemplo ilustrativo de la forma esperada, no un formato emitido
por ninguna herramienta.

---

## 6. Los tres frontends

El docstring de `ir.py` menciona un cuarto frontend (Lua) en la lista de
productores del HIR: es también **destino** — `script/` contiene un runtime de
scripting que no emite HIR.

### 6.1 Texto (Rust / C++ / Python anotados)

| Lenguaje | Mecanismo | Subconjunto | Limitaciones |
|---|---|---|---|
| Rust | proc-macro `#[tlaxtrelom::kernel]` + `syn` | funciones puras, structs POD, `match`, loops acotados | sin traits dinámicos, sin `unsafe`, sin closures que capturen |
| C++ | plugin de Clang (`LibTooling`) sobre `[[tlaxtrelom::kernel]]` | funciones, templates monomorfizados, `constexpr` | sin excepciones, sin RTTI, sin punteros crudos, sin macros complejas |
| Python | `ast` + `@tlaxtrelom.kernel` | expresiones, `if`, `for` acotado, numpy-like | sin `eval`, sin introspección, sin clases dinámicas |

El frontend **no reemplaza** al compilador nativo: extrae el kernel, lo valida
contra el subconjunto y emite HIR. El resto del archivo se compila normalmente y
llama al kernel por ABI.

**Estado**: este frontend es **destino en su totalidad**. No hay crate de
proc-macros (`hir-macros` no existe), ni plugin LibTooling, ni decorador
`@tlaxtrelom.kernel`; la tabla describe el diseño acordado, no código.

### 6.2 Nodos (equivalente a Blueprints)

- Grafo **dataflow** dirigido, con puertos tipados.
- Nodos = intrinsics registrados (`math.lerp`, `noise.simplex`, `neural.read`).
- Estado explícito: `state.get`, `state.set`, `delay` (para feedback).
- Validación: tipos, efectos, ciclos, recursos, límites de bucles.
- Lowering: cada nodo → una o más ops de HIR; las aristas → SSA values.

```text
[Input speed:f32] ──┐
                    ├──> [math.mul] ──> [vec3.compose] ──> [Output velocity]
[Const (0,0,1)] ────┘                          ▲
[Input velocity:vec3] ─────────────────────────┘
```

**Implementado** en `python/tlaxtrelom_tools/graph.py`: `Graph` (context
manager), `Port`/`NodeDef` tipados, registro de intrinsics vía el decorador
`@node`, verificación de tipos al construir con promociones explícitas a nodo
`cast`, gate de determinismo en `_validate()` y lowering a HIR sellado en
`lower()`. Intrinsics registrados hoy: `math.add`, `math.mul`, `math.lerp`,
`math.clamp`, `math.select`, `vec3.compose`, `vec3.scale`, `vec3.add`,
`neural.read`. En TypeScript, `builder.ts` expone el mismo contrato como
primitiva de bajo nivel (`HirBuilder`: `param`/`const`/`op`/`output`/`build`)
sin registro de nodos ni funciones ejecutables.

**Destino**: `noise.simplex`, los nodos de estado (`state.get`, `state.set`,
`delay` — citados en el docstring de `graph.py` como "no incluidos en esta
demo"), el chequeo de recursos/límites de bucles, y un parser del formato de
guardado del editor (aún no hay canvas que serializar).

### 6.3 Bloques apilables (estilo Scratch)

- **Secuencia** + **eventos** + **condicionales** + **bucles acotados**.
- Pensado para prototipado rápido por IA o principiantes.
- Lowering en dos pasos:
  1. Árbol de bloques → grafo de nodos (desazucarado).
  2. Grafo → HIR.
- Los bloques no tienen semántica propia: son *sugar* del grafo. Esto evita
  tres backends distintos y garantiza que un proyecto pueda migrar de bloques a
  nodos sin reescribir.

```text
when start:
  repeat 10:
    set speed to 5
    wait 0.1
  if speed > 3:
    emit "fast"
```

↓

```text
event.start ──> seq ──> for(0..10) ──> state.set(speed, 5) ──> wait(0.1)
                                                      └──> if(gt(speed,3)) ──> emit("fast")
```

**Implementado** en `python/tlaxtrelom_tools/blocks.py`: `Script` construye
sobre `Graph` con `deterministic=False` y efecto `write`; `emit` →
`flow.emit`, secuencias → `flow.seq`, `repeat` con `times ≤ 64` se **desenrolla**
a SSA, `if_` solo admite ramas puras vía `math.select`, y las variables son
renombrado SSA (`var`/`get`/`set`). El selftest de la CLI valida este camino
(`blocks_lower`).

**Destino**: el diagrama con `event.start`, `wait` y `state.set` aún no es
emitible — no hay nodos de evento, espera ni estado en el registro; el pase de
*state machine* para condicionales con efectos tampoco existe.

---

## 7. Backends

| Backend | Entrada | Salida | Uso |
|---|---|---|---|
| SPIR-V | MIR | `.spv` | Shaders, compute, culling |
| WGSL | MIR | `.wgsl` | Debug/portabilidad vía `naga` |
| LLVM | MIR | `.o`/JIT | CPU kernels, DSP, ML |
| Cranelift | MIR | JIT | Editor en caliente, iteración rápida |
| Rust codegen | HIR/MIR | `.rs` | Sistemas ECS monomorfizados |
| C++ codegen | HIR/MIR | `.h/.cpp` | Stubs de plugins, ABI |
| Interpreter | HIR | bytecode | Preview del editor, bloques |

**Regla**: el editor usa Cranelift/interpreter para iterar en < 100 ms; el build
de shipping usa LLVM/SPIR-V con optimización completa y cache en disco.

**Estado**: **todos los backends de la tabla son destino**. No hay código que
produzca `.spv`, `.wgsl`, `.o`, JIT ni codegen Rust/C++ desde HIR/MIR; tampoco
intérprete del HIR. La única ruta real a SPIR-V hoy compila GLSL escrito a
mano (`shaders/glsl/*.vert|frag|comp`) con `glslc` + `spirv-val` vía
`cargo xtask shaders`, sin pasar por el IR. El puente Python
`bindings.compile_hir()` existe como API, pero lanza `NativeUnavailable`
porque el crate `tlaxtrelom-bindings` y el módulo `tlaxtrelom._native` no
están escritos.

---

## 8. Passes

1. **Resolve** (imports, símbolos, intrinsics).
2. **Typecheck** (unificación, casts explícitos).
3. **Effectcheck** (pureza, recursos, determinismo).
4. **Borrow/alias check** (kernels sin aliasing; en GPU, *bind groups* únicos).
5. **Const fold / DCE / CSE**.
6. **Loop bounds** (GPU: bucles acotados; CPU: permitidos con *trip count*).
7. **Vectorize** (SIMD en CPU; `vec4` en GPU).
8. **Kernel fusion** (fusionar nodos adyacentes para reducir dispatches).
9. **Autodiff** (gradientes para ML/física; modo reverso).
10. **Determinism check** (orden de reducciones, FP estricto, RNG sembrado).
11. **Lower a MIR**.
12. **Backend** (SPIR-V/LLVM/Rust/C++).

**Estado**: de la lista, hoy corren **Resolve** (en Python: registro de nodos,
puertos y kwargs validados en `graph.py`; en TS el builder exige tipos y ids
explícitos, sin símbolos que resolver), **Typecheck** en su forma mínima
(coincidencia de tipos declarados, promociones limitadas a `_PROMOTE` emitidas
como `cast`; no hay unificación), **Effectcheck** (set canónico + gate de
`deterministic` en `validate()`/`_validate()` de ambos lenguajes) y la parte
estructural del **Determinism check** (rechazo de no-determinismo en el IR y
hash byte-a-byte estable entre lenguajes). **Destino**: borrow/alias check,
const fold/DCE/CSE, loop bounds, vectorize, kernel fusion, autodiff, lowering
a MIR y los backends (pasos 4–12 salvo lo mencionado).

---

## 9. Determinismo

- **FP estricto** por defecto en simulación: sin `fast-math`, sin FMA
  reasociado, sin `-ffast-math`.
- Reducciones con orden fijo (árbol balanceado) o acumuladores Kahan.
- RNG con semilla explícita y *stream* por sistema.
- Prohibido `nondet` en sistemas autoritativos de red.
- Tests de *golden hash*: mismo input + misma versión de HIR → mismo hash de
  estado tras N ticks.

**Implementado**: determinismo **de representación**: canonicalización con
hash estable entre Python y TS (fixture cruzado `DEMO_HASH` + check
`python_parity` del selftest TS), lowerings idempotentes verificados en CI por
`cargo xtask protocol` (checks `graph_deterministic`, `canonical_bytes`,
`explicit_cast` del lado Python) y `cargo xtask node` (checks
`builder_deterministic`, `float_parity`, `tamper_detection` del lado TS), más
las suites `python/tests/test_ir.py` y `tools/tlaxtrelom-ts/tests/ir.test.ts`.

**Destino**: FP estricto en código compilado, reducciones con orden fijo, RNG
sembrado por sistema, gate de `nondet` en tiempo de ejecución y los golden
hashes de estado tras N ticks: requieren el compilador, el runtime de sim y
los backends que aún no existen.

---

## 10. Versionado y cache

**Implementado**:

- `schema` versionado (`tlaxtrelom.ir/1`); el **validador** (no un runtime de
  juego) rechaza versiones desconocidas, en Python y en TS.
- Hash del HIR canónico (sin el campo `hash`) como identificador por
  contenido, con el prefijo `blake2b512-128:` + BLAKE2b-512 truncado a 128
  bits; es lo que `check`/`hash` de las CLI reportan para CI.
- Herramientas reales: `python -m tlaxtrelom_tools.cli selftest|demo|check|hash`
  y su espejo `node src/cli.ts`; `cargo xtask protocol` corre el selftest de
  Python desde `python/`, `cargo xtask python` corre `python/tests` y
  `cargo xtask node` corre `tools/tlaxtrelom-ts/tests`.

**Destino**: la cache en disco `~/.tlaxtrelom/cache/<engine>/<hash>.spv` no
la escribe ningún código del repo — depende del compilador SPIR-V que aún no
existe; con ella llegarán las reglas de invalidación por versión de backend,
flags de optimización y driver.

---

## 11. Ejemplo end-to-end

El tramo **Python → HIR** funciona hoy tal cual (los intrinsics del ejemplo
son de la biblioteca registrada; `math.pow` habría que registrarlo con el
mismo patrón `@node`):

```python
# Python: frontend de nodos (implementado: graph.py)
with Graph("fresnel") as g:
    cos_t = g.input("cos_theta", T.f32)
    ior = g.input("ior", T.f32)
    r0 = g.call("math.pow", base=(1.0 - ior) / (1.0 + ior), exp=2.0)
    out = g.call("math.lerp", a=r0, b=1.0, t=g.call("math.pow", base=1.0 - cos_t, exp=5.0))
    g.output("reflectance", out)

hir = g.lower()                      # dict canónico con `hash` (implementado)
spv = tlaxtrelom.compile(hir, target="spirv")   # ← destino: no existe
```

```rust
// Rust: el mismo kernel, anotado — destino: no existe el proc-macro
#[tlaxtrelom::kernel(effects = "pure")]
fn fresnel(cos_theta: f32, ior: f32) -> f32 {
    let r0 = ((1.0 - ior) / (1.0 + ior)).powi(2);
    r0 + (1.0 - r0) * (1.0 - cos_theta).powi(5)
}
```

Hoy el contrato verificable es más débil pero real: ambos frontends que
existen (nodos Python y builder TS) producen el **mismo HIR canónico y el
mismo hash** para el mismo grafo. Que ambos produzcan el mismo SPIR-V
cacheado es la meta, y requiere los backends de la sección 7.
