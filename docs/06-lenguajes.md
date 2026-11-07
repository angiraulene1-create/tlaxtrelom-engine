# Tlaxtrelom Engine — Lenguajes y fronteras

> Versión 0.2 · Alcance: por qué cada lenguaje existe, qué posee y cuál es su
> contrato con el resto del motor.

---

## 0. Regla de oro

**Cada dato tiene un dueño y cada frontera es explícita.** Un lenguaje no
"llama" libremente a otro: cruza una ABI versionada, por lotes, con ownership
documentado. El frame loop de un juego publicado no ejecuta Python, Lua ni
TypeScript.

```text
┌──────────────────────────────────────────────────────────────┐
│                        Rust (runtime)                        │
│  frame loop · ECS · scheduler · wgpu · XR · FullDive · IR     │
└───────┬──────────────────┬──────────────────┬─────────────────┘
        │ ABI C v1         │ ABI C HAL        │ ABI C Spatial v1
        ▼                  ▼                  ▼
   C++20 geometry      C11 platform       C/Zig spatial
   (IO + zstd)         (caps/RT/hash)     (6DOF dual-quat/BVH)

   C# .NET SDK (Tlaxtrelom.Engine)        Go Relay (telemetría de red)
   (gameplay VR / seguridad BCI)          (UDP/WebTransport / E-Stop)

   Herramientas fuera del frame loop:
   Python (pipeline/editor) · TypeScript (editor/CLI) · Lua (sandbox)
   WGSL & GLSL/SPIR-V (shaders multiview estéreo y compute)
```

---

## 1. Rust — runtime y camino caliente

**Posee:** bucle principal, simulación a paso fijo, ECS, scheduler, render
`wgpu`, sesión XR, hilo FullDive, `IntentGate`, IR y seguridad de memoria.

**Por qué:** memoria sin GC, concurrencia verificada, FFI predecible y
`no_std` para firmware futuro.

**Contrato:** todo lo que cruza a otro lenguaje pasa por una ABI C plana o por
buffers POD. Un `panic!` no cruza la frontera: se convierte en código de error.

**No hace:** no reemplaza el ecosistema de middleware gráfico/DCC de C++ ni
interpreta scripts en el camino caliente.

---

## 2. C++20 — isla de geometría virtual

**Posee:** streaming de geometría de alta densidad: mallas → clusters de 128
triángulos → DAG de LOD → páginas comprimidas con zstd.

**Por qué:** control fino de IO (`pread`), compresión, hilos de afinidad y
código de terceros ya existente. El DAG y el recorrido son lógica pura sin
`wgpu`.

**ABI:** `native/tlaxtrelom-geometry/include/tlaxtrelom_geometry.h`.

- `GEO_ABI_VERSION = 1`. El runtime rechaza una isla con versión distinta.
- Handles opacos; el llamador nunca libera memoria de una página: lo hace
  `tlaxtrelom_geo_ack_upload`.
- Formato cocinado: magic `0x544C5847` (`"TLXG"`), versión 1.
- Contrato de hilos: creación en el hilo principal; `begin_frame`, `collect`,
  `drain_ready`, `ack_upload` y `get_stats` siempre en el hilo de render.

**No hace:** no toca GPU, no conoce entidades/materiales/shaders y no libera
memoria de GPU.

**Build:** `make -C native/tlaxtrelom-geometry all`. No requiere CMake; el
`CMakeLists.txt` se conserva para entornos que lo prefieran.

---

## 3. C11 — HAL de plataforma

**Posee:** detección de capacidades (cores, memoria, páginas, SIMD, RT),
afinidad de hilos, `mlockall`, reloj monotónico y hashing.

**Por qué:** es la frontera mínima y estable con el sistema operativo. C11 sin
dependencias mantiene la HAL portable y testeable.

**Contrato:** `native/tlaxtrelom-hal/include/tlaxtrelom_hal.h`. La feature
`native-hal` está activa por defecto; `--no-default-features` produce un
binario Rust-only con un stub portable.

**No hace:** no contiene lógica de gameplay ni estado del mundo.

---

## 4. Python — pipeline, editor y autoría

**Posee:** HIR canónico, frontend de nodos, bloques apilables, CLI, validador
y puente opcional PyO3.

**Por qué:** productividad, ecosistema DCC/ML e iteración sin recompilar. Es el
lenguaje del editor, no del runtime de shipping.

**Contrato:**

- `python/tlaxtrelom_tools/ir.py` define la forma canónica y el hash.
- El HIR viaja como JSON UTF-8 canónico; nunca `pickle`.
- El módulo nativo (`tlaxtrelom._native`) es opcional: si no está, la CLI
  informa cómo compilarlo en vez de fallar de forma opaca.

**No hace:** no se enlaza en el binario de un juego publicado.

---

## 5. Lua — scripting sandbox

**Posee:** eventos de gameplay, ejes, RNG determinista, logs y callbacks
`on_start` / `on_tick` / `on_stop`.

**Por qué:** scripting embebible, rápido de cargar y fácil de aislar. El host
consume eventos por lote; el intérprete no está en el render.

**Sandbox:**

- Sin `os`, `io`, `package`, `require`, `debug`, `dofile`, `loadfile`.
- Sin carga de bytecode (`load` solo acepta texto).
- Presupuesto de instrucciones y memoria con hooks.
- `pcall`/`xpcall` no pueden ocultar el agotamiento de presupuesto.
- Cola de eventos y logs acotadas (backpressure explícita).

**Contrato:** `script/tlaxtrelom/init.lua`; ejemplo en
`script/examples/turret.lua`; suite en `script/tests/run.lua`.

**No hace:** no accede al filesystem, a la red ni a la GPU.

---

## 6. TypeScript — editor y validación

**Posee:** validador HIR, builder SSA y CLI de autoría, ejecutables con Node
nativo (sin `tsc`).

**Por qué:** el editor web y las herramientas comparten tipos y validación con
la CLI de Python. Node 22.6+ ejecuta TypeScript directamente; el proyecto no
depende de una toolchain de build.

**Contrato:** `tools/tlaxtrelom-ts/`.

- `src/ir.ts`: forma canónica, hash y validación.
- `src/builder.ts`: builder de HIR y grafo de referencia.
- `src/cli.ts`: `selftest`, `demo`, `check`, `hash`.
- Debe producir **los mismos bytes** que Python. Los tests comparan el hash del
  demo (`blake2b512-128:b2861452eb9b8267652b896c81d00774`) y fixtures de
  formato de floats.

**No hace:** no es el runtime del juego ni reemplaza al compilador de Rust.

---

## 7. WGSL y GLSL/SPIR-V — shaders

**Posee:** shader estéreo de referencia (`shaders/stereo.wgsl`), shaders multiview
Vulkan (`shaders/glsl/stereo_multiview.vert/.frag`) y compute shader de foveated
rendering (`shaders/glsl/fulldive_foveated.comp`).

**Por qué:** WGSL proporciona interoperabilidad unificada sobre `wgpu`, mientras
que GLSL 450 compilado a SPIR-V con `glslc` y validado con `spirv-val` cubre el
camino nativo de render de alto rendimiento en Vulkan/OpenXR.

**Contrato:** los shaders se compilan y validan formalmente en la suite de CI
con `spirv-val`.

**No hace:** no contiene lógica de gameplay ni estado persistente.

---

## 8. C / Zig — cinemática espacial SIMD y cuaterniones duales

**Posee:** transformaciones 6DOF en cuaterniones duales para evitar el colapso
de volumen en avatares VR, e intersección de rayos con cajas AABB para BVH y
foveated culling (`native/tlaxtrelom-spatial/`).

**Por qué:** matemática pura sin asignaciones dinámicas, compilación directa
con cero overhead y verificación en tiempo de compilación.

**Contrato:** ABI C plana `include/tlaxtrelom_spatial.h` versionada (`ABI = 1`).
Posee implementación nativa dual en Zig (`spatial.zig`) y C11 portable.

---

## 9. C# (.NET SDK) — autoría de gameplay VR y scripts de seguridad

**Posee:** SDK de desarrollo de alto nivel `dotnet/Tlaxtrelom.Engine/` con wrappers
P/Invoke de bajo costo para el runtime nativo, filtro 1€ en C#, monitor de
seguridad biométrica y procesamiento de audio espacial HRTF.

**Por qué:** estándar indiscutible en la industria de VR para desarrollo de
escenas interactivas y herramientas de autoría rápida (Unity, Godot, Stride).

**Contrato:** structs idénticos en memoria con `StructLayout(Sequential)`
validando el tamaño exacto de 56 bytes de `IntentFrame`.

---

## 10. Go (Golang) — telemetría distribuida y relay de red

**Posee:** servidor de red y sincronización de sesiones VR multijugador y
telemetría FullDive vía UDP/WebTransport (`services/tlaxtrelom-relay/`).

**Por qué:** alta concurrencia nativa, multiplexación con goroutines y latencia
sub-milisegundo para propagación de estados neuronales y E-Stop distribuido.

**Contrato:** protocolo binario con magic `0x544C5831` ("TLX1"), checksum CRC32
y watchdog de desconexión por inactividad.

---

## 11. Matriz de build políglota

| Componente | Herramienta | Comando |
|---|---|---|
| Rust (Core) | Cargo | `cargo test --workspace` |
| C++20 (Geometría) | Make + g++ | `make -C native/tlaxtrelom-geometry test` |
| C11 (HAL) | `cc` vía `build.rs` | `cargo check` (feature `native-hal`) |
| C / Zig (Spatial) | Make / cc / zig | `make -C native/tlaxtrelom-spatial test` |
| GLSL / SPIR-V | glslc + spirv-val | `cargo xtask shaders` |
| C# (.NET SDK) | Python / dotnet | `python3 dotnet/test_csharp.py` |
| Go (Relay) | Python / go | `python3 services/tlaxtrelom-relay/test_relay.py` |
| Python (Tools) | Python 3.11+ | `python3 -m unittest discover -s tests` |
| Lua (Scripting) | Lua 5.4+ | `lua script/tests/run.lua` |
| TypeScript (HIR) | Node 22.6+ | `node --test tests` |
| Docs | `xtask` | `cargo xtask docs` |

La matriz completa se ejecuta con `cargo xtask ci`; el workflow de GitHub
Actions invoca exactamente el mismo comando.

---

## 9. Añadir un lenguaje nuevo

Un lenguaje nuevo solo entra si cumple **todas** estas condiciones:

1. Tiene un rol que ningún lenguaje actual cubre mejor.
2. Su frontera es por lotes, no por objeto.
3. No introduce un GC en el frame loop.
4. Puede fallar sin tumbar el runtime (código de error, no `panic`/`throw`
   cruzando la ABI).
5. Tiene tests en CI y un contrato documentado.
6. Si produce HIR, respeta la forma canónica y el hash compartido.

Un lenguaje de autoría que no cumpla esto puede vivir como herramienta externa,
pero no como parte del runtime.
