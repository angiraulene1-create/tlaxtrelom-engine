# Registro de cambios — Tlaxtrelom Engine

Formato basado en [Keep a Changelog](https://keepachangelog.com/es-ES/1.1.0/).
El proyecto es propietario; no hay releases públicas ni paquetes en registries.

## [0.2.0] — 2026-11-06

### Añadido

- Runtime Rust con host `winit` + `wgpu`, bucle a paso fijo, métricas de frame,
  recuperación de superficie y telemetría agregada.
- Sesión XR con composición estéreo, presupuesto de frame, escala adaptativa,
  validación de IPD/FOV y preview `--stereo`.
- FullDive: `IntentFrame` POD, decoder de referencia, transporte mock/replay,
  gate de seguridad, watchdog y presupuesto RT.
- Isla C++20 de geometría virtual con formato cocinado `TLXG` v1, ABI
  `GEO_ABI_VERSION = 1`, IO + zstd y smoke test de ABI en C++.
- HAL C11 (`native/tlaxtrelom-hal`) con capacidades, afinidad, `mlockall`,
  reloj y hashing.
- HIR canónico en Python y TypeScript: JSON ordenado sin espacios, normalización
  de enteros, floats compatibles con CPython y hash
  `blake2b512-128:`.
- CLI Python (`selftest`, `demo`, `check`, `hash`) y CLI TypeScript equivalente.
- Runtime Lua sandbox con callbacks, ejes, RNG determinista, presupuestos de
  instrucciones/memoria, ejemplo `turret` y 14 tests.
- Matriz `cargo xtask` (`fmt`, `clippy`, `test`, `native`, `python`, `lua`,
  `node`, `docs`, `ci`) y workflow de GitHub Actions que invoca la misma matriz.
- Documentación 05–08 y README reescritos; enlaces locales verificados en CI.
- Metadatos de repositorio: `.editorconfig`, `rustfmt.toml`, `clippy.toml`,
  `SECURITY.md`, `CONTRIBUTING.md` y `Makefile` raíz.

### Cambiado

- Renombrado integral del motor a **Tlaxtrelom Engine**.
- La HAL C11 es feature por defecto; `--no-default-features` produce un binario
  Rust-only con stub portable.
- El hash HIR usa BLAKE2b-512 truncado a 128 bits (antes `blake2b` de 16 bytes
  sin prefijo) y es compartido por Python y TypeScript.
- El formato de geometría usa magic `0x544C5847` (`"TLXG"`).
- El percentil de métricas usa *nearest-rank* (p50 de 1..100 = 50).
- La isla C++ se construye con `Makefile` para entornos sin CMake; el
  `CMakeLists.txt` se conserva como alternativa.

### Corregido

- `CPU_SET`/`sched_setaffinity` con `-std=c11` estricto (`_GNU_SOURCE`).
- Instrumentación de Lua: `Runtime.new` es accesible y `stats().instructions`
  refleja el consumo del hook.
- `pcall`/`xpcall` del sandbox no pueden ocultar el agotamiento de presupuesto.
- Recuperación de superficie y neutralización del gate ante frames tardíos.

### Seguridad

- FullDive deshabilitado por defecto en modo seguro (`enabled = false`); no se
  inventan señales cuando `mock = false`.
- Sandbox Lua sin `os`, `io`, `package`, `require`, `debug`, `dofile`,
  `loadfile` ni bytecode.
- Rechazo de islas C++ con ABI incompatible y de assets con formato inválido.

## [0.1.0] — 2026-01-15

### Añadido

- Documento de arquitectura inicial, especificación FullDive y esquemas de
  referencia en Rust, C++ y Python.
- Política de propiedad intelectual y allowlist de licencias.
