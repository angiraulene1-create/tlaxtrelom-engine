# Tlaxtrelom Engine — Operaciones

> Versión 0.2 · Alcance: requisitos, build, pruebas, CI, diagnóstico, release y
> gestión de artefactos.

---

## 0. Requisitos

| Herramienta | Versión mínima | Uso |
|---|---|---|
| Rust estable | 1.82 | runtime, `xtask` |
| `g++` | C++20 | isla de geometría |
| `make` | cualquiera POSIX | build C++ sin CMake |
| `zstd` (dev) | 1.4 | compresión de páginas |
| Python | 3.11 | pipeline y CLI |
| Lua | 5.4 | tests del sandbox |
| Node | 22.6 | TypeScript nativo (sin `tsc`) |
| Vulkan / DX12 / Metal | driver vigente | render |

No se requiere `cmake` ni `rustup`. El entorno de referencia usa el toolchain
del sistema y el `Makefile` de la isla C++.

---

## 1. Build

```bash
# Runtime completo (HAL C11 incluida por defecto)
cargo build --release

# Binario Rust-only (sin C)
cargo build --release --no-default-features

# Isla C++ de geometría
make -C native/tlaxtrelom-geometry all

# Runtime enlazado con la isla C++
make -C native/tlaxtrelom-geometry all
cargo build --release --features native-geometry
```

La isla C++ se enlaza desde
`native/tlaxtrelom-geometry/build/libtlaxtrelom_geometry.so`. El `build.rs`
verifica que exista y falla con un mensaje accionable si no.

### 1.1 Feature flags

| Feature | Efecto |
|---|---|
| `native-hal` (default) | compila y enlaza la HAL C11 |
| `native-geometry` | enlaza la isla C++ ya construida |
| *(ninguna)* | stubs portables, útil para CI sin C |

---

## 2. Pruebas

```bash
cargo xtask ci            # matriz completa
cargo xtask test          # Rust, features por defecto
cargo xtask test-all      # Rust + isla C++
cargo xtask native-test   # smoke test de la ABI C++
cargo xtask python        # 39 tests Python
cargo xtask lua           # 14 tests Lua
cargo xtask node          # 24 tests TypeScript
cargo xtask docs          # enlaces locales de la documentación
```

La matriz de CI ejecuta, en este orden:

1. `cargo fmt --all -- --check`
2. `cargo clippy --workspace --all-targets --all-features -- -D warnings`
3. build + smoke test de C++
4. `cargo test --workspace`
5. `cargo test --workspace --features native-geometry`
6. tests de Python, Lua y Node
7. selftest headless con `--json`
8. verificación de enlaces de documentación

`cargo xtask ci` es la única fuente de verdad. El workflow de GitHub Actions no
duplica comandos: invoca `cargo xtask ci`.

---

## 3. Ejecución

```bash
tlaxtrelom                    # ventana mono, 120 Hz
tlaxtrelom --stereo           # preview estéreo, 90 Hz
tlaxtrelom --headless         # sin GPU ni ventana
tlaxtrelom --selftest         # selftest determinista
tlaxtrelom --selftest --json  # reporte para CI
tlaxtrelom --probe            # adaptadores wgpu
tlaxtrelom --help             # flags y rangos
```

Flags relevantes:

| Flag | Efecto |
|---|---|
| `--sim-hz N` | frecuencia de simulación (30–480) |
| `--fps N` | tasa objetivo de presentación |
| `--ipd-mm N` | IPD en milímetros (50–80) |
| `--render-scale F` | escala por ojo (0.5–2.0) |
| `--ticks N` | ticks del selftest/headless |
| `--replay ARCHIVO` | reproduce un log `.tlxneuro` |
| `--no-fulldive` | modo seguro: intención neutral |
| `--secure` | confianza discreta alta, sin mock |
| `--json` | salida JSON del selftest |

Un flag desconocido es un error, no un *silent ignore*.

---

## 4. Diagnóstico

### 4.1 Selftest

Salida humana:

```text
tlaxtrelom-engine 0.2.0 — selftest OK
  ticks           : 600
  sim hash        : 0x098593b3972fbdd8
  determinismo    : sí
  pasos válidos   : 599
  pasos gated     : 1
  frames acción   : 17
  XR frames       : 240 (late 0, scale 1.00)
  páginas geom.   : 0
  HAL             : linux x86_64 · 16 cores · ...
  tiempo          : 0.397 ms
```

`--json` expone los mismos campos para automatización. `ok` es vinculante: CI
falla si es `false`.

### 4.2 Problemas típicos

| Síntoma | Causa probable | Acción |
|---|---|---|
| `wgpu no encontró ningún adaptador` | driver/entorno sin GPU | `--probe`; usar `--selftest` en CI |
| `no se pudo cargar tlaxtrelom-geometry` | `.so` no construida | `make -C native/tlaxtrelom-geometry all` |
| `ABI de geometría incompatible` | isla vieja | rebuild de la isla |
| `ipd_m fuera de [50, 80] mm` | flag corrupto | corregir `--ipd-mm` |
| `no existe el archivo de replay` | ruta incorrecta | revisar `--replay` |
| Lua: `presupuesto ... excedido` | script costoso | optimizar o subir budget conscientemente |
| Lua: `global no permitido` | acceso a `os`/`io`/`debug` | usar `engine.*` |
| Hash HIR distinto entre Python y TS | cambio de forma canónica | ejecutar ambos selftests y comparar fixtures |

### 4.3 Telemetría

En ejecución normal el host emite métricas agregadas cada
`--telemetry-every` frames. No se toman decisiones de calidad con un solo
frame; se usan ventanas con histéresis.

---

## 5. CI

`.github/workflows/ci.yml` instala toolchain y dependencias y ejecuta:

```bash
cargo xtask ci
```

No hay pasos "especiales" en CI que no existan en local. Un fallo que solo
aparece en CI es un fallo de reproducibilidad y se trata como bug.

### 5.1 Caché

- Caché de Cargo para `target/` y `~/.cargo`.
- Caché de `native/tlaxtrelom-geometry/build` no se versiona.
- Los artefactos de release se generan desde un commit limpio, no desde un
  workspace con cambios.

---

## 6. Release

Checklist:

1. `cargo xtask ci` en verde sobre el commit exacto.
2. `CHANGELOG.md` actualizado.
3. Versión coherente en `Cargo.toml`, `python/pyproject.toml` y
   `tools/tlaxtrelom-ts/package.json`.
4. `cargo build --release` y smoke test de la isla C++.
5. SBOM generado y archivado (pendiente de automatizar).
6. Binario firmado; hash publicado en el canal interno.
7. Distribución solo a equipos internos o socios bajo NDA.

El modelo de distribución es binario firmado. No se publica código fuente,
paquetes ni imágenes fuera de la organización.

---

## 7. Versionado

- `0.x`: contrato en evolución; los cambios incompatibles suben la versión
  menor y el schema/ABI correspondiente.
- `GEO_ABI_VERSION` y el magic `TLXG` son contratos independientes de la
  versión del motor.
- `tlaxtrelom.ir/1` es el schema actual del HIR. Cambiarlo exige actualizar
  Rust, Python y TypeScript en el mismo commit.
- Los hashes de simulación y HIR son fixtures de CI; un cambio deliberado
  actualiza el fixture y queda documentado en el changelog.

---

## 8. Limpieza

```bash
cargo xtask clean         # target/ + build C++
cargo clean               # solo Rust
make -C native/tlaxtrelom-geometry clean
```

La caché de shaders y artefactos cocinados vive fuera del repositorio, en el
directorio de usuario (`~/.tlaxtrelom/cache`). Nunca se versiona.
