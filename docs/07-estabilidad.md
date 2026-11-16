<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 2026 angiraulene1-create -->

# Tlaxtrelom Engine — Estabilidad

> Versión 0.2 · Alcance: invariantes de runtime, catálogo de fallos, límites de
> recursos, determinismo y estrategia de pruebas.

---

## 0. Qué significa "estable" aquí

No significa "nunca falla". Significa:

1. **Falla temprano y con contexto** en vez de corromper estado tres horas
   después.
2. **Un subsistema no tumba a los demás**: el render se recupera de una
   superficie perdida; un script no bloquea el host; un frame BCI inválido se
   neutraliza.
3. **El comportamiento es reproducible**: mismo input, misma versión y mismo
   hardware ⇒ mismo hash de simulación.
4. **Los límites son explícitos**: memoria, instrucciones, colas, pasos por
   frame y presupuesto de latencia.

---

## 1. Invariantes del runtime

| Invariante | Verificación |
|---|---|
| Toda configuración se valida antes de crear GPU/ventana | `Config::validate` |
| La simulación avanza a paso fijo y con tope por frame | `max_sim_steps`, tests de pacer |
| Un `NaN`/`Inf` no entra en matrices ni ejes | validación en XR, gate y Lua |
| El gate neutraliza sin frame nuevo | watchdog de 50 ms, tests |
| La isla C++ rechaza ABI incompatible | `GEO_ABI_VERSION = 2`, smoke test |
| Un script no puede escapar del sandbox | suite Lua (14 tests) |
| El hash HIR es idéntico en Python y TypeScript | fixture compartido |
| El frame loop no asigna memoria en el hilo RT | contrato de `FullDive` |
| Un error de superficie no aborta si es recuperable | `SurfaceError::Lost/Outdated` |
| Coordenadas en dos capas: el anclaje `f64` (`WorldOrigin`) nunca entra en un shader; solo viaja el offset `f32` (`LocalPose`) | `coords.rs`, tests de rebase |
| El rebase (umbral de 1024 m) pliega X/Z al anclaje y no toca el eje Y por debajo del umbral | `REBASE_THRESHOLD`, `rebase()` |
| La integridad de la escena depende del anclaje | `World::digest()` hashea el anclaje |
| Integridad del pipeline HDR: MSAA se negocia contra el target HDR, no contra el swapchain; tonemap ACES solo en el pase de presentación | `renderer.rs`, `shaders/present.wgsl` |
| Degradación de formato documentada: `Rgba16Float` → `Rgba32Float` → 8 bits si el adaptador no acepta lo anterior | `renderer.rs` |
| Frustum culling con vista neutral (perspectiva de 90°) | `View::neutral()` (antes `View::identity()`) |
| La isla rechaza assets cocinados `TLXG` v1; el formato vigente es v2 (origen y LOD en `double`) | `ERR_FORMAT`, `cook_asset.cpp` |
| `GeoView.origin` es `double` a través de la ABI | cabecera de geometría |

---

## 2. Catálogo de fallos y respuesta

### 2.1 GPU / ventana

| Fallo | Respuesta |
|---|---|
| `SurfaceError::Lost` / `Outdated` | reconfigurar superficie y continuar |
| `SurfaceError::Timeout` | reintentar; contar error |
| `SurfaceError::OutOfMemory` | abortar limpio con diagnóstico |
| Adaptador ausente | `--probe` explica qué backend falta |
| Shader inválido | falla el build (test de parseo WGSL) |

Nunca se avanza la simulación más de `max_sim_steps` tras un stall. Un
breakpoint o una suspensión del portátil no produce una cascada de ticks.

### 2.2 XR

| Fallo | Respuesta |
|---|---|
| Pose no disponible | reutilizar la última pose válida, marcar el frame |
| IPD/FOV fuera de rango | rechazar en arranque |
| Frames tardíos consecutivos | bajar `render_scale` con histéresis |
| Matriz no finita | rechazar el frame, no enviarlo a GPU |

### 2.3 FullDive

| Fallo | Respuesta |
|---|---|
| Sin frames nuevos | watchdog ⇒ neutral + `GATED` |
| Confianza baja | neutral |
| Artefacto / electrodo caído | `ARTIFACT`, nunca intención válida |
| Replay inexistente | error de configuración al arrancar |
| Hardware ausente con `mock=false` | error accionable; no se inventan señales |
| Modo deshabilitado | intención neutral, sin hilo ni transporte |

### 2.4 Scripting Lua

| Fallo | Respuesta |
|---|---|
| Error de sintaxis | se reporta, el host sigue vivo |
| Bucle infinito | presupuesto de instrucciones lo corta |
| Allocación desmedida | presupuesto de memoria lo corta |
| `pcall` intenta ocultar el corte | el error de presupuesto se re-lanza |
| Cola de eventos llena | backpressure explícita |
| Acceso a `os`/`io`/`debug` | error de global no permitido |

### 2.5 Geometría C++

| Fallo | Respuesta |
|---|---|
| Asset con magic/versión inválidos (p. ej. `TLXG` v1) | `ERR_FORMAT`; no se registra |
| Archivo inexistente | `ERR_IO` |
| Doble `ack_upload` | `ERR_STATE` |
| ABI distinta | rechazo en carga |
| Página corrupta | se descarta; el contador de fallos sube |

---

## 3. Límites de recursos

| Recurso | Límite | Dónde |
|---|---|---|
| Pasos de simulación por frame | 8 (configurable) | `Config::max_sim_steps` |
| `dt` máximo tras stall | 100 ms | `Config::max_frame_dt` |
| Canales BCI | 64 | `MAX_CHANNELS` |
| Sample rate BCI | 250–16 000 Hz | `Config::validate` |
| Cola de eventos Lua | 4096 | `script/tlaxtrelom/init.lua` |
| Logs Lua | 256 (ring) | `script/tlaxtrelom/init.lua` |
| Presupuesto Lua por defecto | 50 000 instrucciones / 16 MiB | `Runtime.new` |
| Acciones discretas | 16 bits | `gate::MAX_ACTIONS` |
| Páginas listas de geometría | configurable | `TlaxtrelomGeoConfig` |

Los límites son parte del contrato: subirlos exige revisar el peor caso de
latencia, no solo el caso feliz.

---

## 4. Determinismo

- **FP estricto** en simulación: sin `fast-math` ni reasociación.
- **Paso fijo**: el acumulador de simulación no depende del frame rate.
- **RNG explícito**: el RNG de Lua es un LCG sembrado por el host, no
  `math.random` global.
- **Hash de simulación**: `--selftest` compara el estado tras N ticks.
- **Hash de HIR**: forma canónica compartida entre Python y TypeScript.

Un test de determinismo que solo compara "no explotó" no sirve. Los tests
comparan hashes y fallan si cambian sin una actualización deliberada del
fixture.

---

## 5. Seguridad de FFI

- Handles opacos y versión de ABI al crear la isla.
- Sin excepciones C++ cruzando la ABI.
- Sin `panic!`/`unwrap` cruzando la ABI.
- Ownership documentado: quién asigna, quién libera y hasta cuándo es válido un
  puntero.
- `unsafe` acotado y documentado; Clippy lo vigila.
- Los tests de ABI son en C y C++ reales, no solo en Rust.

---

## 6. Estrategia de pruebas

| Capa | Estado actual | Comando |
|---|---|---|
| Rust (`tlaxtrelom-core`, `--all-features`) | 126 pasan, 3 ignoradas | `cargo test -p tlaxtrelom-core --all-features` |
| Rust (sin features) | 125 pasan | `cargo test -p tlaxtrelom-core` |
| Rust (`xtask`, empaquetado y validación de `packaging/`) | 18 pasan | `cargo test -p xtask` |
| GPU offscreen (ignoradas, requieren hardware local) | 3 | `cargo test -- --ignored` |
| Python | 40 pasan | `cd python && python3 -m unittest discover -s tests` |
| Lua | 14 pasan | `lua script/tests/run.lua` |
| TypeScript | 24 pasan | `cd tools/tlaxtrelom-ts && node --test tests` |
| C++ geometría (ABI + TLXG v2) | smoke test en verde | `make -C native/tlaxtrelom-geometry test` |
| C spatial (cinemática SIMD; `spatial.zig` no se compila) | smoke test en verde | `make -C native/tlaxtrelom-spatial test` |
| HarmonyOS (puente NDK) | smoke test en verde | `make -C native/tlaxtrelom-harmonyos test` |
| Shaders GLSL→SPIR-V | `glslc` + `spirv-val` | `cargo xtask shaders` |
| Selftest | headless determinista con `--json` | `cargo run -- --selftest --json` |
| Docs | enlaces locales | `cargo xtask docs` |
| Packaging | definiciones de `packaging/`, `.nsi`, JSON5 y guiones, sin compilar | `cargo xtask package-check` |

### 6.1 La matriz real de CI (`cargo xtask ci`)

Ejecuta, en este orden y sobre un único runner `ubuntu-latest`:

1. `cargo fmt --all -- --check`
2. `cargo clippy --workspace --all-targets --all-features -- -D warnings`
3. Geometría C++: `make -C native/tlaxtrelom-geometry all` + `test`
4. Spatial: `make -C native/tlaxtrelom-spatial test`
5. HarmonyOS: `make -C native/tlaxtrelom-harmonyos test`
6. Shaders GLSL→SPIR-V con `glslc` y validación `spirv-val`
7. `cargo test --workspace`
8. `cargo test --workspace --features native-geometry`
9. C#: `python3 dotnet/test_csharp.py`
10. Relay Go: `python3 services/tlaxtrelom-relay/test_relay.py`
11. Python, Lua y Node
12. Selftest headless con `--json`
13. Validación de enlaces de documentación

No hay matriz multi-OS ni GPU en CI: los 3 tests ignorados de render HDR/present
solo se corren en hardware local con `cargo test -- --ignored`.

`cargo xtask ci` es la única fuente de verdad. Si un comando funciona en
local pero no en CI, es un bug de la matriz, no una excusa.

---

## 7. Compatibilidad de plataforma

- **Linux x86_64**: plataforma de referencia (Vulkan, HAL C11, `SCHED_FIFO`).
- **Windows/macOS**: el runtime Rust compila; la HAL C11 usa rutas portables o
  stub. Verificar `--probe` y el backend `wgpu` antes de prometer soporte.
- **Sin GPU**: `--selftest` y `--headless` no requieren adaptador.
- **Sin C**: `--no-default-features` produce un binario Rust-only para entornos
  restringidos.

Una plataforma solo se declara soportada si pasa la matriz y el selftest en
hardware real.

---

## 8. Qué haría fallar una release

- `cargo xtask ci` en rojo.
- Cambio de hash de simulación o de HIR sin actualizar el fixture.
- Aumento de dependencias fuera de la allowlist de `deny.toml`.
- Cualquier ruta que permita a un script Lua tocar filesystem/red.
- Cualquier actuador FullDive sin watchdog o parada física documentada.
- Documentación que prometa una capacidad no implementada.

---

## 9. Deuda conocida

- CI sin GPU: los pases HDR y de presentación (`present.wgsl`, tonemap ACES,
  MSAA contra target HDR) solo se prueban en hardware local, bajo
  `cargo test -- --ignored`.
- Backend OpenXR real: contrato definido, integración pendiente.
- Firmware BCI: no existe; los decoders EEG/EMG de pruebas y replay son
  surrogados deterministas, no hardware.
- El relay Go y el SDK C# no enlazan contra el runtime: sus pruebas en CI son
  standalone y no cubren la integración end-to-end. Además el runner no instala
  `go` ni `dotnet`, así que lo que se valida es el contrato de bytes (TLX1 +
  CRC32) reimplementado en Python, no el compilado de aquellos.
- `services/tlaxtrelom-relay/relay.go:99`: `main()` sólo imprime una línea. El
  `RelayServer` con `ListenUDP` existe y su encuadernado está probado, pero el
  demonio nunca abre socket.
- `native/tlaxtrelom-spatial/spatial.zig` comparte la ABI del kernel C y no la
  compila ninguna ruta de build ni de CI: la contrapartida en Zig está sin
  verificar.
- El viñeteado de confort está a medias: `ComfortVignette` (`xr.rs`) calcula la
  apertura en CPU, pero ningún uniform del frame WGSL la consume. El shader que
  sí recibe `comfortVignette` es `shaders/glsl/stereo_multiview.frag`, un
  contrato compilado en CI que el runtime no carga. En pantalla hoy no hay
  confort dinámico.
- Sin fuzzing de la ruta de render en CI (WGSL/SPIR-V/assets cocinados).
- Firmado de artefactos y SBOM: política documentada; falta automatizar en CI.
- El conteo de instrucciones de Lua es una **cota inferior** muestreada por
  hooks; no es un contador exacto de instrucciones VM.
