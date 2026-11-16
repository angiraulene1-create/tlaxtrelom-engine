<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 2026 angiraulene1-create -->

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
│  frame loop · mundo (world.rs) · coords · dimension · wgpu   │
│  XR · FullDive · metrics · headless                          │
└───────┬──────────────────┬──────────────────┬────────────────┘
        │ ABI C GEO v2     │ ABI C HAL v1     │ ABI C Spatial v1
        ▼                  ▼                  ▼
   C++20 geometry      C11 platform       C11 spatial (Zig = destino)
   (IO + zstd, TLXG v2) (caps/RT/hash)    (6DOF dual-quat/BVH)

   C# .NET SDK (Tlaxtrelom.Engine)        Go Relay (telemetría de red)
   (espejo ABI managed; VR / seguridad)   (UDP TLX1; WebTransport = contrato)

   Herramientas fuera del frame loop:
   Python (pipeline/editor, HIR JSON) · TypeScript (editor/CLI) · Lua (sandbox)
   WGSL (pases reales: scene3d, stereo, present) · GLSL/SPIR-V (validados, no cargados)
   HarmonyOS (C++20/XComponent, contrato probado por smoke)
```

El puente Rust↔C++ **no usa `cxx`**: son funciones `extern "C"` planas sobre
structs POD verificados con `bytemuck` (`crates/tlaxtrelom-core/src/geometry.rs`).

---

## 1. Rust — runtime y camino caliente

**Posee:** bucle principal, simulación a paso fijo, el mundo del runtime
(`world.rs`: entidades, origen global, poses), coordenadas de dos capas
(`coords.rs`), modos dimensionales (`dimension.rs`), render `wgpu` con pipeline
HDR→ACES (`renderer.rs`), sesión XR, hilo FullDive, `IntentGate`, métricas
(`metrics.rs`) y modo headless/selftest (`headless.rs`), además de la
seguridad de memoria.

**Por qué:** memoria sin GC, concurrencia verificada, FFI predecible y
`no_std` para firmware futuro.

**Contrato:** todo lo que cruza a otro lenguaje pasa por una ABI C plana o por
buffers POD (`extern "C"` + `bytemuck`; sin `cxx`, sin PyO3 en runtime). Un
`panic!` no cruza la frontera: se convierte en código de error.

**No hace:** no tiene ECS ni scheduler ni IR propios (el mundo vive en
`world.rs`; el IR canónico vive en Python/TypeScript) y no interpreta scripts
en el camino caliente.

---

## 2. C++20 — isla de geometría virtual

**Posee:** streaming de geometría de alta densidad: mallas → clusters de 128
triángulos → DAG de LOD → páginas comprimidas con zstd.

**Por qué:** control fino de IO (`pread`), compresión, hilos de afinidad y
código de terceros ya existente. El DAG y el recorrido son lógica pura sin
`wgpu`.

**ABI:** `native/tlaxtrelom-geometry/include/tlaxtrelom_geometry.h`.

- `GEO_ABI_VERSION = 2`. El runtime rechaza una isla con versión distinta. La 2
  añadió `TlaxtrelomGeoView::origin` (origen flotante, `double[3]`) y el origen
  de sector de la cabecera; con ella el LOD se mide en `double` y la isla recorta
  por frustum.
- Handles opacos; el llamador nunca libera memoria de una página: lo hace
  `tlaxtrelom_geo_ack_upload`.
- Formato cocinado: magic `0x544C5847` (`"TLXG"`), versión 2. Los `center` de los
  clusters son `float` **relativos** al `origin` del asset (metros, `double`):
  así un sector puede vivir a 1e11 m sin que la geometría pierda resolución. Un
  asset v1 se rechaza con `TLAXTRELOM_GEO_ERR_FORMAT` y hay que recocinarlo
  (`make -C native/tlaxtrelom-geometry cook`).
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

**Contrato:** `native/tlaxtrelom-hal/include/tlaxtrelom_hal.h`.
`tlaxtrelom_hal_abi_version()` devuelve la entera `1`
(`TLAXTRELOM_HAL_ABI_VERSION 1u`) y el runtime la compara contra su propia
constante (`HAL_ABI_VERSION: u32 = 1` en `crates/tlaxtrelom-core/src/hal.rs`);
no existe ninguna versión string tipo «1.1.0». La feature `native-hal` está
activa por defecto; `--no-default-features` produce un binario Rust-only con un
stub portable.

**No hace:** no contiene lógica de gameplay ni estado del mundo.

---

## 4. Python — pipeline, editor y autoría

**Posee:** HIR canónico, frontend de nodos, bloques apilables, CLI y validador.

**Por qué:** productividad, ecosistema DCC/ML e iteración sin recompilar. Es el
lenguaje del editor, no del runtime de shipping.

**Contrato:**

- `python/tlaxtrelom_tools/ir.py` define la forma canónica y el hash.
- El HIR viaja como JSON UTF-8 canónico; nunca `pickle`.
- **No hay enlace Rust↔Python en runtime.** `bindings.py` referencia un módulo
  opcional `tlaxtrelom._native` (PyO3) que no se compila en este repo: no
  existe el crate `tlaxtrelom-bindings`. El intercambio real con el motor se
  hace por artefactos en disco: el HIR JSON canónico y los bins cocinados del
  pipeline (p. ej. `TLXG` v2 de geometría). Si el módulo opcional falta, la CLI
  lo dice en vez de fallar de forma opaca.
- `numpy` es **opcional** (solo lo requieren las rutas de señales neuronales).

**No hace:** no se enlaza en el binario de un juego publicado, y hoy no llama
al motor en tiempo de ejecución.

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

**Posee:** los tres pases WGSL que sí se cargan en runtime: `shaders/scene3d.wgsl`
(geometría sobre el target HDR), `shaders/stereo.wgsl` (preview estéreo/mono y
modos dimensionales) y `shaders/present.wgsl` (tonemap ACES, dither y viñeteado
de confort). Además, los shaders GLSL multiview Vulkan
(`shaders/glsl/stereo_multiview.vert/.frag`) y el compute de foveated rendering
(`shaders/glsl/fulldive_foveated.comp`).

**Por qué:** WGSL proporciona interoperabilidad unificada sobre `wgpu`, mientras
que GLSL 450 compilado a SPIR-V con `glslc` y validado con `spirv-val` cubre el
futuro camino nativo de render de alto rendimiento en Vulkan/OpenXR.

**Contrato:** los WGSL se parsean y validan con `naga` en tests y los GLSL se
compilan y validan con `spirv-val` en CI. **Honestidad: los `.spv` generados de
GLSL no se cargan en tiempo de ejecución** — son artefactos de contrato, no
parte del frame loop. El runtime dibuja el estéreo en un único pase de escena
WGSL (un draw recortado por ojo) más un pase de presentación, sin multiview.

**No hace:** no contiene lógica de gameplay ni estado persistente.

---

## 8. C / Zig — cinemática espacial SIMD y cuaterniones duales

**Posee:** transformaciones 6DOF en cuaterniones duales para evitar el colapso
de volumen en avatares VR, e intersección de rayos con cajas AABB para BVH y
foveated culling (`native/tlaxtrelom-spatial/`).

**Por qué:** matemática pura sin asignaciones dinámicas, compilación directa
con cero overhead y verificación en tiempo de compilación.

**Contrato:** ABI C plana `include/tlaxtrelom_spatial.h` versionada
(`TLAXTRELOM_SPATIAL_ABI_VERSION = 1u`). Existen dos implementaciones que
exportan los mismos símbolos: C11 portable (`src/tlaxtrelom_spatial.c`) y Zig
(`spatial.zig`, sintaxis 0.14). **El binario final enlaza la versión C.**
**Honestidad:** ningún paso del `Makefile` ni de `cargo xtask ci` compila o
prueba `spatial.zig` (no hay build de Zig integrado ni `zig test` en la suite);
la reimplementación en Zig es destino verificado, no cobertura actual.

---

## 9. C# (.NET SDK) — autoría de gameplay VR y scripts de seguridad

**Posee:** SDK de desarrollo `dotnet/Tlaxtrelom.Engine/` (5 archivos:
`NativeTypes.cs`, `VrTrackingFilter.cs`, `SpatialAudio.cs`,
`FullDiveSafetyMonitor.cs` y `Tests/TlaxtrelomTests.cs`) con el espejo managed
del ABI nativo, filtro 1€ en C#, monitor de seguridad biométrica y
procesamiento de audio espacial HRTF.

**Por qué:** estándar indiscutible en la industria de VR para desarrollo de
escenas interactivas y herramientas de autoría rápida (Unity, Godot, Stride).

**Contrato:** structs idénticos en memoria con `StructLayout(Sequential)`
validando el tamaño exacto de 56 bytes de `IntentFrame`
(8+4+4+4+32+2+2) y bits de flags coincidentes con
`crates/tlaxtrelom-core/src/fulldive.rs`. **Honestidad:** hoy el SDK es 100 %
managed y no declara ningún `DllImport`; el acceso P/Invoke contra la
biblioteca compartida es contrato de destino, no estado actual.

**Validación:** `dotnet/test_csharp.py` **sí corre en CI** dentro de
`cargo xtask ci`, pero como validador Python puro (parsea los `.cs` y comprueba
layouts y bits contra el Rust). Solo si el SDK `dotnet` está disponible además
compila `Tlaxtrelom.Engine.csproj`; en las máquinas de CI actuales (sin SDK)
esa compilación no se ejecuta. Lo verificado en CI es la consistencia de ABI,
no un binario C#.

---

## 10. Go (Golang) — telemetría distribuida y relay de red

**Posee:** servidor de retransmisión de telemetría y pánico distribuido
(E-Stop) en `services/tlaxtrelom-relay/`. El transporte real es **UDP con
paquetes binarios**: cabecera con magic `0x544C5831` ("TLX1"), secuencia,
`UserID`, checksum CRC32 sobre cabecera+payload, y poda de sesiones por
inactividad (`PruneStale`).

**Por qué:** alta concurrencia nativa y multiplexación con goroutines para
propagación de estados y E-Stop.

**Honestidad:** **WebTransport no está implementado** — no hay servidor QUIC/HTTP/3
ni ruta TCP en el repo; es contrato. El binario `main()` del relay aún no abre
el socket en producción (la lógica de sesiones y paquetes sí está testada).

---

## 11. Matriz de build políglota

| Componente | Herramienta | Comando |
|---|---|---|
| Rust (Core) | Cargo | `cargo test -p tlaxtrelom-core --all-features` (123 tests; 3 GPU offscreen con `-- --ignored`) |
| C++20 (Geometría) | Make + g++ | `make -C native/tlaxtrelom-geometry test` |
| C11 (HAL, `tlaxtrelom_hal_abi_version()` = 1) | `cc` vía `build.rs` | `cargo check` (feature `native-hal`) |
| C (Spatial, ruta de shipping) | Make / cc | `make -C native/tlaxtrelom-spatial test` |
| Zig (Spatial, kernel paralelo) | — | `spatial.zig` existe; ningún paso de build o CI lo compila hoy (destino) |
| C++ (HarmonyOS, XComponent NDK) | Make + `cc` | `make -C native/tlaxtrelom-harmonyos test` (smoke; `ABI_VERSION = 1`, sin host OHOS real) |
| GLSL / SPIR-V | glslc + spirv-val | `cargo xtask shaders` (valida; no carga en runtime) |
| C# (.NET SDK) | Python / dotnet | `python3 dotnet/test_csharp.py` (corre vía `cargo xtask ci`; compila con `dotnet` solo si el SDK está presente) |
| Go (Relay) | Python / go | `python3 services/tlaxtrelom-relay/test_relay.py` |
| Python (Tools) | Python 3.11+ | `python3 -m unittest discover -s tests` (40 tests) |
| Lua (Scripting) | Lua 5.4+ | `cargo xtask lua` (= `lua script/tests/run.lua`; 14 tests) |
| TypeScript (HIR) | Node 22.6+ | `node --test tests` (24 tests) |
| Docs | `xtask` | `cargo xtask docs` |

La matriz completa se ejecuta con `cargo xtask ci`; el workflow de GitHub
Actions invoca exactamente el mismo comando, en **un único runner Linux** (las
pruebas locales de C++ y HarmonyOS son smoke tests verde sobre Linux; no
equivalen a verificación en Windows, macOS o un dispositivo OHOS real).

---

## 12. Añadir un lenguaje nuevo

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
