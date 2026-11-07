# Tlaxtrelom Engine

> Licenciado bajo **[MIT License](LICENSE)**. © 2026 angiraulene1-create.
> Motor gráfico modular, determinista y orientado a experimentación XR y simulación espacial.
> Ver documentación en [arquitectura](docs/01-architecture.md) y [política de dependencias](docs/04-propiedad-intelectual.md).

Motor gráfico políglota, determinista y orientado a **VR / FullDive**. Rust
posee el runtime y el frame loop; C++ aporta la isla de geometría virtual; C11
implementa la HAL de plataforma; C/Zig acelera la cinemática espacial SIMD 6DOF;
C# (.NET) ofrece autoría de gameplay VR y monitores de seguridad; Go gestiona
la telemetría distribuida; Python, Lua y TypeScript cubren autoría, scripting
y tooling; WGSL y GLSL/SPIR-V comandan shaders estéreo y compute.

Este repositorio **ya compila y ejecuta su matriz de pruebas local**: no es solo
un documento de arquitectura. El host abre una ventana `winit` + `wgpu`, corre
simulación a paso fijo, compone estéreo con viñeteado de confort, filtra jitter con
filtro 1€, predice poses a tiempo de display MTP, monitorea signos vitales y
consume intención FullDive a través de un gate de seguridad con parada física y suave.

## La tesis en una frase

**Rust posee el camino caliente; C++ las islas de alto rendimiento; C11 la
frontera de plataforma; C/Zig la cinemática SIMD; C# y Go la autoría VR y red;
y los lenguajes de scripting viven fuera del frame loop.
La frontera es una ABI C versionada y por lotes, nunca un RPC por objeto.**

## Lenguajes y responsabilidades

| Lenguaje | Rol | Qué **no** hace |
|---|---|---|
| **Rust** | Runtime, ECS, scheduler, `wgpu`, XR, FullDive, seguridad, IR | Interoperar con SDKs gráficos propietarios sin FFI |
| **C++20** | Streaming de geometría virtual (clusters, LOD, IO + zstd) | Tocar GPU, entidades, materiales o shaders |
| **C11** | HAL: capacidades, afinidad, `mlockall`, hashing, reloj | Lógica de gameplay |
| **C / Zig** | Cinemática espacial 6DOF, cuaterniones duales, BVH AABB | Manejo de recursos o I/O |
| **C# (.NET)** | SDK de gameplay VR, monitor biométrico, audio 3D HRTF | Frame loop interno del motor |
| **Go** | Relay de red, sincronización UDP/WebTransport, pánico E-Stop | Simulación física local |
| **Python** | Pipeline de assets, editor, grafo/bloques, CLI, bindings | Frame loop de shipping |
| **Lua 5.4+** | Scripting sandbox de gameplay y eventos | Acceso a `os`, `io`, `package`, `require` o bytecode |
| **TypeScript** | Editor, validador HIR y CLI (Node nativo, sin `tsc`) | Runtime de juego |
| **WGSL / GLSL**| Shaders estéreo multiview, compute de foveación | Autoría de gameplay |

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
│   └── tlaxtrelom-hal/         # HAL C11
├── dotnet/Tlaxtrelom.Engine/   # SDK C# (.NET) para gameplay VR y seguridad
├── services/tlaxtrelom-relay/  # Relay de red y telemetría en Go
├── python/tlaxtrelom_tools/    # HIR, grafo, bloques, CLI, bindings
├── tools/tlaxtrelom-ts/        # validador HIR, builder y CLI en TypeScript
├── script/                     # runtime Lua sandbox + ejemplo + tests
├── shaders/                    # shaders WGSL y GLSL/SPIR-V (multiview + foveación)
├── docs/                       # arquitectura, IR, FullDive, operaciones
└── .github/workflows/ci.yml    # misma matriz que `cargo xtask ci`
```

## Quickstart

Requiere Rust estable, `g++` con C++20, `zstd`, Python 3.11+, Lua 5.4+ y Node
22.6+ (probado con Node 26, que ejecuta TypeScript nativo).

```bash
# 1. Selftest determinista sin GPU ni ventana
cargo run -p tlaxtrelom-core --bin tlaxtrelom -- --selftest

# 2. Host de escritorio (120 Hz, mono)
cargo run -p tlaxtrelom-core --bin tlaxtrelom

# 3. Preview estéreo side-by-side (VR sin compositor XR)
cargo run -p tlaxtrelom-core --bin tlaxtrelom -- --stereo

# 4. Isla C++ y tests de su ABI
make -C native/tlaxtrelom-geometry test

# 5. Herramientas de autoría
(cd python && python3 -m tlaxtrelom_tools.cli selftest)
(cd tools/tlaxtrelom-ts && node src/cli.ts selftest)
lua script/tests/run.lua

# 6. Matriz completa (fmt, clippy, C++, Rust, Python, Lua, Node, docs)
cargo xtask ci
```

El binario sirve también para diagnosticar hardware:

```bash
tlaxtrelom --probe          # enumera adaptadores wgpu
tlaxtrelom --help           # flags, rangos y valores por defecto
tlaxtrelom --selftest --json
```

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
- Isla C++ de geometría con formato cocinado `TLXG` v1 y ABI `GEO_ABI_VERSION=1`.
- HAL C11 con detección de SIMD, afinidad y hashing.
- Pipeline Python (39 tests), runtime Lua (14 tests) y tooling TypeScript
  (24 tests) verdes.
- FullDive: contrato de decoder, transporte *mock*/replay, gate, watchdog y
  presupuesto RT. El backend OpenXR y el transporte BCI real son contratos
  explícitos, no promesas disfrazadas.

La hoja de ruta y los criterios de salida están en
[arquitectura](docs/01-architecture.md) y [operaciones](docs/08-operaciones.md).

## Documentación

| Documento | Contenido |
|---|---|
| [01 · Arquitectura](docs/01-architecture.md) | capas, FFI, hilos, build, ADRs |
| [02 · FullDive](docs/02-fulldive.md) | hardware BCI, protocolo, latencia, seguridad |
| [03 · Tlaxtrelom IR](docs/03-tlaxtrelom-ir.md) | tipos, efectos, HIR/MIR, frontends, backends |
| [04 · Propiedad intelectual](docs/04-propiedad-intelectual.md) | licencias, secretos, SBOM, firma |
| [05 · VR y FullDive](docs/05-vr-fulldive.md) | pipeline XR, confort, gate, modos seguros |
| [06 · Lenguajes](docs/06-lenguajes.md) | fronteras y contrato de cada lenguaje |
| [07 · Estabilidad](docs/07-estabilidad.md) | invariantes, fallos, determinismo, pruebas |
| [08 · Operaciones](docs/08-operaciones.md) | build, CI, release, diagnóstico |
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
7. **La propiedad intelectual y dependencias se verifican en el proceso, no al final.** Cabeceras
   SPDX, allowlist de licencias permisivas, SBOM y *provenance* desde el primer commit.

## Licencia

Este proyecto está licenciado bajo los términos de la **[Licencia MIT](LICENSE)**. © 2026 angiraulene1-create.
Eres libre de utilizar, modificar, bifurcar y distribuir este motor de acuerdo con los términos de dicha licencia.
Ver también la [política de dependencias y licencias de terceros](docs/04-propiedad-intelectual.md).
