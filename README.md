<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 2026 angiraulene1-create -->

# Tlaxtrelom Engine

> [!WARNING]
> **Versión alfa (`0.4.0-alpha.1`) — proyecto experimental y conceptual.** El
> motor está en etapa temprana: puede fallar al arrancar en equipos o GPUs sin
> cubrir, cambiar de formato de archivos o de ABI sin aviso, y los instaladores
> no están firmados ni probados en todas las plataformas. Las etiquetas de
> GitHub se publican como *pre-releases* a propósito. Úsalo como laboratorio,
> no como producto.

> Licenciado bajo **[MIT License](LICENSE)**. © 2026 angiraulene1-create.
> Motor gráfico modular, determinista y de ultra-alta fidelidad (2D, 2.5D, 3D, 3.5D, 4D, 4.5D, 5D) con arquitectura de próxima generación (estándar Unreal Engine 5/6) y complemento experimental FullDive en segundo plano (opt-in, desactivado por defecto).
> Ver documentación en [arquitectura](docs/01-architecture.md) y [licencias y dependencias](docs/04-licencias.md).

Motor gráfico políglota y determinista concebido para rendering de última generación y simulación espacial hiperdimensional (**2D, 2.5D, 3D, 3.5D, 4D, 4.5D y 5D**). Rust posee el runtime y el frame loop; C++20 aporta la isla de geometría virtual (streaming de clusters DAG y jerarquía de LOD inspirada en Nanite); C11 implementa la HAL de plataforma; C acelera la cinemática espacial SIMD 6DOF; C# (.NET) ofrece autoría de gameplay y herramientas de monitoreo; Go gestiona la telemetría distribuida; Python, Lua y TypeScript cubren autoría, scripting y tooling; WGSL y GLSL/SPIR-V comandan shaders estéreo, compute y presentación HDR ACES.

**FullDive en segundo plano**: El subsistema neural/BCI es un **prototipo experimental opt-in** que permanece completamente en segundo plano y viene **desactivado por defecto**. El motor arranca de fábrica como un motor gráfico normal, de altísimo rendimiento y estabilidad. Sólo cuando se requiere investigar interfaces neuronales se activa con `--fulldive` o `--replay`, levantando su hilo en segundo plano con *gate* de seguridad, watchdog biométrico y desconexión gradual.

Este repositorio **ya compila y ejecuta su matriz de pruebas local**: no es solo un documento de arquitectura. El host abre una ventana `winit` + `wgpu`, corre simulación a paso fijo, compone estéreo con viñeteado de confort, filtra jitter con filtro 1€, predice poses a tiempo de display MTP, procesa proyecciones multi-dimensionales continuas y, de forma optativa, consume intención FullDive a través de un gate de seguridad con parada física y suave.

## La tesis en una frase

**Rust posee el camino caliente; C++ las islas de alto rendimiento; C11 la
frontera de plataforma; C la cinemática SIMD; C# y Go la autoría VR y red;
y los lenguajes de scripting viven fuera del frame loop.
La frontera es una ABI C versionada y por lotes, nunca un RPC por objeto.**

## Lenguajes y responsabilidades

| Lenguaje | Rol | Qué **no** hace |
|---|---|---|
| **Rust** | Runtime, ECS, scheduler, `wgpu`, XR, FullDive, seguridad, IR | Interoperar con SDKs gráficos propietarios sin FFI |
| **C++20** | Streaming de geometría virtual (clusters, LOD, IO + zstd) | Tocar GPU, entidades, materiales o shaders |
| **C11** | HAL: capacidades, afinidad, `mlockall`, hashing, reloj | Lógica de gameplay |
| **C** (con port a Zig sin compilar) | Cinemática espacial 6DOF, cuaterniones duales, BVH AABB | Manejo de recursos o I/O |
| **C# (.NET)** | SDK de gameplay VR, monitor biométrico, audio 3D HRTF | Frame loop interno del motor |
| **Go** | Relay de red, sincronización UDP/WebTransport, pánico E-Stop | Simulación física local |
| **Python** | Pipeline de assets, editor, grafo/bloques, CLI, bindings | Frame loop de shipping |
| **Lua 5.4+** | Scripting sandbox de gameplay y eventos | Acceso a `os`, `io`, `package`, `require` o bytecode |
| **TypeScript** | Editor, validador HIR y CLI (Node nativo, sin `tsc`) | Runtime de juego |
| **WGSL / GLSL**| Shaders del runtime wgpu: escena, estéreo y presentación HDR (WGSL). Los contratos GLSL de estéreo multiview y foveación compute se compilan y validan a SPIR-V en CI, pero el runtime aún no los consume | Autoría de gameplay |
| **Zig / HarmonyOS** | Contrapartida `spatial.zig` del kernel spatial y puente ArkUI | Runtime del frame loop |

El detalle de cada frontera está en [lenguajes](docs/06-lenguajes.md).

## Garantías de estabilidad

- **Nada entra al frame loop sin validar**: `sim_hz`, IPD, resolución, budgets y
  confianza se comprueban al arrancar con errores accionables.
- **Simulación a paso fijo** con tope de pasos por frame (sin *spiral of death*).
- **Determinismo verificable**: mismo input ⇒ mismo hash (`--selftest`).
- **XR seguro y confortable**: poses filtradas con 1€ filter, predictor a display time MTP,
  viñeteado dinámico anti-cinetosis (VR sickness), IPD en rango humano y `render_scale` adaptativo.
- **FullDive con *fail-safe* y salud biológica**: watchdog biométrico (BPM, SpO2, temperatura),
  desconexión gradual en 300 ms (E-Stop suave sin choque sensorial), limitador de estimulación a 50 µA
  y supresión de resonancia cortical.
- **ABI versionada**: una isla C++ o Spatial incompatible se rechaza en vez de corromper
  memoria.
- **Presupuestos de scripting**: Lua corta bucles infinitos y allocaciones
  desmedidas incluso si el script captura errores con `pcall`.

Ver [estabilidad](docs/07-estabilidad.md) y [VR/FullDive](docs/05-vr-fulldive.md).

## Estructura

```text
tlaxtrelom-engine/
├── Cargo.toml                  # workspace Rust; publish = false
├── crates/
│   ├── tlaxtrelom-core/        # runtime, host, XR, FullDive, HAL, geometría
│   └── xtask/                  # orquestador de build, test y CI
├── native/
│   ├── tlaxtrelom-geometry/    # isla C++ (Makefile + smoke test de ABI)
│   ├── tlaxtrelom-spatial/     # cinemática espacial SIMD en C y Zig
│   ├── tlaxtrelom-hal/         # HAL C11 (enlazada vía build.rs; sin Makefile propio)
│   └── tlaxtrelom-harmonyos/   # puente ArkUI/HarmonyOS en C++20 (Makefile + smoke)
├── dotnet/Tlaxtrelom.Engine/   # SDK C# (.NET): gameplay VR, biometría, audio HRTF
├── services/tlaxtrelom-relay/  # Relay de red y telemetría en Go
├── python/tlaxtrelom_tools/    # HIR, grafo, bloques, CLI, bindings
├── tools/tlaxtrelom-ts/        # validador HIR, builder y CLI en TypeScript
├── script/                     # runtime Lua sandbox + ejemplo + tests
├── shaders/                    # WGSL del runtime + GLSL/SPIR-V (multiview + foveación)
├── assets/                     # ejemplo cocinado `demo_cluster.tlxg`
├── docs/                       # arquitectura, IR, FullDive, operaciones
└── .github/workflows/ci.yml    # misma matriz que `cargo xtask ci`
```

## Quickstart

Requiere Rust 1.83+ (la MSRV real del código), `g++` con C++20, `zstd`,
Python 3.11+, Lua 5.4+ y Node 22.6+ (probado con Node 26, que ejecuta
TypeScript nativo).

```bash
# 1. Selftest determinista sin GPU ni ventana
cargo run -p tlaxtrelom-core --bin tlaxtrelom -- --selftest

# 2. Host de escritorio (120 Hz, mono)
cargo run -p tlaxtrelom-core --bin tlaxtrelom

# 3. Preview estéreo side-by-side (VR sin compositor XR)
cargo run -p tlaxtrelom-core --bin tlaxtrelom -- --stereo

# 4. Islas nativas y tests de sus ABI (el puente Rust las enlaza con el feature)
make -C native/tlaxtrelom-geometry test
make -C native/tlaxtrelom-spatial test
make -C native/tlaxtrelom-harmonyos test
cargo test -p tlaxtrelom-core --all-features   # 128 pruebas, 4 de GPU en --ignored

# 5. Herramientas de autoría
(cd python && python3 -m tlaxtrelom_tools.cli selftest)
(cd tools/tlaxtrelom-ts && node src/cli.ts selftest)
lua script/tests/run.lua

# 6. Matriz completa (fmt, clippy, C++/C/HarmonyOS, shaders SPIR-V, Rust,
#    C#, Go, Python, Lua, Node, selftest, packaging y enlaces de docs)
cargo xtask ci

# 7. Paquetes portables e instaladores → dist/
cargo xtask package --platform linux   # .tar.gz con layout FHS + .deb + install.sh
cargo xtask package-check              # valida packaging/ sin compilar
```

El binario sirve también para diagnosticar hardware:

```bash
tlaxtrelom --probe          # enumera adaptadores wgpu
tlaxtrelom --help           # flags, rangos y valores por defecto
tlaxtrelom --selftest --json
tlaxtrelom --stereo --msaa 4 --sector 40000 --dimension 4d
```

### Flags reales del host

Lista completa tal y como la parsea `crates/tlaxtrelom-core/src/config.rs`:
cualquier otro flag es un error duro, nunca un *silent ignore*.

| Flag | Qué hace |
|---|---|
| `--windowed` / `--stereo` / `--headless` / `--selftest` | modo de ejecución (ventana, estéreo side-by-side, sin GPU) |
| `--sim-hz <Hz>` | frecuencia de simulación [30, 480] (120) |
| `--fps <Hz>` | tasa objetivo de presentación (120) |
| `--ipd-mm <mm>` | distancia interpupilar (63) |
| `--render-scale <f>` | escala de render [0.5, 2.0] (1.0) |
| `--sector <m>` | alcance del sector en metros [1, 1e8] (10000) |
| `--ticks <n>` | pasos de simulación en headless (600) |
| `--telemetry-every <n>` | frames entre resúmenes de telemetría (120) |
| `--replay <archivo>` | reproducir un log `.tlxneuro` determinista |
| `--dimension <modo>` | `2d`, `2.5d`, `3d` (defecto), `3.5d`, `4d`, `4.5d`, `5d` |
| `--dimension-blend <f>` | dónde se está del espectro [0, 1] (0.5): mezcla la cámara ortográfica del `2.5d` con la perspectiva del `3d` en `3.5d`, y la proyección 3D con la 4D en `4.5d` |
| `--dimension-focal <f>` | distancia focal N-D [0.1, 1000] (6.0) |
| `--msaa <n>` | muestras MSAA {1, 2, 4} (1) |
| `--fulldive` | activa la pila FullDive (viene desactivada: es un complemento experimental) |
| `--no-fulldive` | apaga FullDive aunque esté el mock o un replay cargado |
| `--secure` | exige confianza ≥ 0.75 para acciones discretas |
| `--json` | selftest en JSON (para CI) |
| `-h`/`--help`, `-V`/`--version`, `--probe` | ayuda, versión, enumeración de adaptadores |

## HIR canónico

Los frontends (Python, TypeScript y los futuros frontends de texto) producen el
mismo HIR:

- JSON UTF-8 con claves ordenadas y sin espacios;
- enteros normalizados (`1.0` y `1` son el mismo byte);
- floats formateados como `repr()` de CPython;
- hash `blake2b512-128:` (BLAKE2b-512 truncado a 128 bits).

El grafo de referencia tiene hash
`blake2b512-128:b2861452eb9b8267652b896c81d00774` en **ambas** implementaciones.
Ver [Tlaxtrelom IR](docs/03-tlaxtrelom-ir.md).

## Estado

- Runtime Rust con ventana, estéreo, recuperación de superficie, telemetría y
  selftest determinista.
- Render en dos pases: escena a un target HDR lineal (`Rgba16Float`, con caída a
  `Rgba32Float` y a 8 bits) y presentación con exposición, tonemap ACES y
  dither (`shaders/present.wgsl`). La **viñeta de confort** vive en el pase de
  escena: `XrSession::begin_frame` la calcula con `ComfortVignette` (velocidad
  angular desenvuelta), la viaja en `StereoFrame::comfort_aperture` y
  `scene3d.wgsl` la aplica por ojo desde el uniform `scene.comfort.x` — 1.0 en
  reposo, 0.4 en el peor latigazo, esquina a 1/4 de brillo.
  El MSAA vive en el target HDR; `present.wgsl` es un triángulo fullscreen sin
  geometría. El estéreo es **un pase de escena con un draw recortado por ojo**
  (side-by-side), no multiview (`multiview: None`).
- Proyección dimensional N-D (`--dimension {2d,2.5d,3d,3.5d,4d,4.5d,5d}`) con blend
  de ejes extra en el vertex stage. `3.5d` es una **mezcla ortográfica↔de
  perspectiva** en la cámara XR (profundidad parcial sin punto de fuga fuerte),
  no un eje adicional: `--dimension-blend` controla cosas distintas en `3.5d` y
  en `4.5d`.
- Geometría virtual: `GEO_ABI_VERSION = 2` con formato cocinado `TLXG` v2
  (magic `0x544C5847`, `kAssetVersion = 2`); clusters de 128 triángulos, DAG
  jerárquico, páginas indexadas por `page_id`, origen de sector en `double`,
  frustum culling por esferas y distancias de LOD en `double`.
- HAL C11 con **ABI 1** (`TLAXTRELOM_HAL_ABI_VERSION 1u`), enlazada por defecto
  vía `build.rs` (feature `native-hal`); cinemática spatial **ABI 1** y puente
  HarmonyOS **ABI 1**, cada uno con su smoke test.
  `native/tlaxtrelom-spatial/spatial.zig` es la contrapartida en Zig: existe y
  comparte la ABI, pero la matriz local no la compila (necesita `zig` en el
  PATH). El puente C# ↔ native y el relay Go tienen sus propios smoke tests.
- Coordenadas de dos capas (`coords.rs`): anclaje global `WorldOrigin` en `f64`
  + offset de render en `f32`, con rebase a 1024 m y `--sector` hasta 1e8 m.
  Sin límite de arena ni colapso de estéreo por alejamiento del origen.
- Pipeline Python (40 tests), runtime Lua (14 tests), tooling TypeScript
  (24 tests) y los smoke de ABI de geometría C++, spatial C y HarmonyOS verdes.
  En Rust: 132 `#[test]` en `tlaxtrelom-core` (128 corren con `--all-features`; 4 de
  GPU tras `--ignored`) y 18 en `xtask`.
- FullDive: contrato de decoder, transporte *mock*/replay, gate, watchdog y
  presupuesto RT. Es un **complemento experimental opt-in**: `Config::default()`
  lo deja apagado (ni hilo de tiempo real ni transporte) y se enciende con
  `--fulldive` o con `--replay`. El backend OpenXR y el transporte BCI real son
  contratos explícitos, no promesas disfrazadas.

La hoja de ruta y los criterios de salida están en
[arquitectura](docs/01-architecture.md) y [operaciones](docs/08-operaciones.md).

## Documentación

| Documento | Contenido |
|---|---|
| [01 · Arquitectura](docs/01-architecture.md) | capas, FFI, hilos, build, ADRs |
| [02 · FullDive](docs/02-fulldive.md) | hardware BCI, protocolo, latencia, seguridad |
| [03 · Tlaxtrelom IR](docs/03-tlaxtrelom-ir.md) | tipos, efectos, HIR/MIR, frontends, backends |
| [04 · Licencias](docs/04-licencias.md) | MIT, SPDX, dependencias, SBOM, firma |
| [05 · VR y FullDive](docs/05-vr-fulldive.md) | pipeline XR, confort, gate, modos seguros |
| [06 · Lenguajes](docs/06-lenguajes.md) | fronteras y contrato de cada lenguaje |
| [07 · Estabilidad](docs/07-estabilidad.md) | invariantes, fallos, determinismo, pruebas |
| [08 · Operaciones](docs/08-operaciones.md) | build, CI, release, diagnóstico |
| [09 · Multiplataforma](docs/09-multiplataforma.md) | Linux, Windows, macOS, HarmonyOS |
| [Índice](docs/README.md) | mapa de la documentación |

## Principios de diseño

1. **Un solo dueño por dato.** Cada buffer tiene un único owner en un único
   lenguaje; los demás reciben préstamos con *lifetime* documentado.
2. **Lotes, no llamadas.** Cruzar la FFI cuesta; se cruza una vez por frame o
   por página, nunca por entidad.
3. **Fallar en el borde.** `panic!` y `throw` no cruzan la ABI: se convierten en
   códigos de error y se registran.
4. **Medir antes de optimizar.** Métricas de frame, histogramas de latencia y
   telemetría agregada desde el primer commit.
5. **Determinismo donde importa.** Simulación a paso fijo; los surrogados
   neuronales tienen *fallback* determinista.
6. **La seguridad del usuario es un requisito, no una feature.** Un casco BCI
   nunca acciona un actuador sin *gate*, *watchdog* y parada física.
7. **Las licencias y las dependencias se verifican en el proceso, no al final.** Cabeceras
   SPDX MIT, allowlist de licencias permisivas, SBOM y *provenance* desde el primer commit.

## Licencia

Este proyecto está licenciado bajo los términos de la **[Licencia MIT](LICENSE)**. © 2026 angiraulene1-create.
Eres libre de utilizar, modificar, bifurcar y distribuir este motor de acuerdo con los términos de dicha licencia.
Cada archivo fuente lleva `SPDX-License-Identifier: MIT`. Ver también la
[política de licencias, dependencias y cadena de suministro](docs/04-licencias.md).
