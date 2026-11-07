# Tlaxtrelom IR — Un AST para tres paradigmas de autoría

> Versión 0.1 · Alcance: tipos, efectos, HIR/MIR, frontends (texto, nodos,
> bloques), backends, passes y determinismo.

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
`gpu` exige subir la versión del schema y actualizar Python, TypeScript y Rust
en el mismo cambio.

El *effect checker* es lo que permite que un mismo grafo se ejecute en el editor
(interpreter), en GPU (SPIR-V) y en el servidor (LLVM) con garantías distintas.
Ejemplo: un nodo `neural.read` no puede entrar en un `system` marcado
`deterministic = true` sin pasar por un `gate` que lo convierta en input
explícito.

---

## 4. HIR (High-level IR)

- **SSA** (static single assignment), tipado, con efectos por operación.
- `values` contiene parámetros, constantes y resultados de operaciones; cada
  operación referencia únicamente valores anteriores (sin ciclos).
- Metadatos: `module`, `entry`, `deterministic`, `hash`.
- Serialización canónica: **JSON UTF-8 con claves ordenadas y sin espacios**.
  No se usa `pickle` ni un formato binario dependiente de una versión de
  librería: el contrato son los bytes.

### 4.1 Ejemplo

```json
{"deterministic":true,"entry":"main","module":"player_locomotion","ops":[{"args":[1,2,3],"effects":["pure"],"id":4,"op":"vec3.compose","type":"vec3<f32>"},{"args":[4,0],"effects":["pure"],"id":5,"op":"vec3.scale","type":"vec3<f32>"}],"outputs":[{"name":"velocity","type":"vec3<f32>","value":5}],"schema":"tlaxtrelom.ir/1","values":[{"id":0,"kind":"param","name":"speed","type":"f32"},{"id":1,"kind":"const","type":"f32","value":0},{"id":2,"kind":"const","type":"f32","value":0},{"id":3,"kind":"const","type":"f32","value":1},{"id":4,"kind":"op","type":"vec3<f32>"},{"id":5,"kind":"op","type":"vec3<f32>"}]}
```

El hash de ese HIR (sin el campo `hash`) es
`blake2b512-128:b2861452eb9b8267652b896c81d00774`, idéntico en Python y
TypeScript.

### 4.2 Invariantes

- Todo valor tiene un tipo concreto tras la inferencia.
- Toda operación declara sus efectos.
- Los ids de `values` son contiguos desde 0 y los argumentos son SSA.
- No hay ciclos en el grafo de datos salvo a través de `state`/`delay`, que se
  bajan a memoria explícita.
- Las reducciones declaran su orden (para determinismo).
- El hash cubre el HIR completo salvo el propio campo `hash`; cualquier cambio
  de contenido cambia el ID.

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

---

## 6. Los tres frontends

### 6.1 Texto (Rust / C++ / Python anotados)

| Lenguaje | Mecanismo | Subconjunto | Limitaciones |
|---|---|---|---|
| Rust | proc-macro `#[tlaxtrelom::kernel]` + `syn` | funciones puras, structs POD, `match`, loops acotados | sin traits dinámicos, sin `unsafe`, sin closures que capturen |
| C++ | plugin de Clang (`LibTooling`) sobre `[[tlaxtrelom::kernel]]` | funciones, templates monomorfizados, `constexpr` | sin excepciones, sin RTTI, sin punteros crudos, sin macros complejas |
| Python | `ast` + `@tlaxtrelom.kernel` | expresiones, `if`, `for` acotado, numpy-like | sin `eval`, sin introspección, sin clases dinámicas |

El frontend **no reemplaza** al compilador nativo: extrae el kernel, lo valida
contra el subconjunto y emite HIR. El resto del archivo se compila normalmente y
llama al kernel por ABI.

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

---

## 9. Determinismo

- **FP estricto** por defecto en simulación: sin `fast-math`, sin FMA
  reasociado, sin `-ffast-math`.
- Reducciones con orden fijo (árbol balanceado) o acumuladores Kahan.
- RNG con semilla explícita y *stream* por sistema.
- Prohibido `nondet` en sistemas autoritativos de red.
- Tests de *golden hash*: mismo input + misma versión de HIR → mismo hash de
  estado tras N ticks.

---

## 10. Versionado y cache

- `schema` versionado (`tlaxtrelom.ir/1`); el runtime rechaza versiones
  desconocidas.
- Hash de HIR canónico (sin el campo `hash`) para cachear shaders y kernels:
  `blake2b512-128:` + BLAKE2b-512 truncado a 128 bits.
- Cache en disco: `~/.tlaxtrelom/cache/<engine>/<hash>.spv`.
- Invalidación por: versión de backend, flags de optimización, driver (solo si
  el driver tiene bugs conocidos).

---

## 11. Ejemplo end-to-end

```python
# Python: frontend de nodos
with Graph("fresnel") as g:
    cos_t = g.input("cos_theta", T.f32)
    ior = g.input("ior", T.f32)
    r0 = g.call("math.pow", base=(1.0 - ior) / (1.0 + ior), exp=2.0)
    out = g.call("math.lerp", a=r0, b=1.0, t=g.call("math.pow", base=1.0 - cos_t, exp=5.0))
    g.output("reflectance", out)

hir = g.lower()                      # dict / FlatBuffers
spv = tlaxtrelom.compile(hir, target="spirv")
```

```rust
// Rust: el mismo kernel, anotado
#[tlaxtrelom::kernel(effects = "pure")]
fn fresnel(cos_theta: f32, ior: f32) -> f32 {
    let r0 = ((1.0 - ior) / (1.0 + ior)).powi(2);
    r0 + (1.0 - r0) * (1.0 - cos_theta).powi(5)
}
```

Ambos producen el **mismo HIR canónico** (módulo de nombres) y, por tanto, el
mismo SPIR-V cacheado. Ese es el contrato.
