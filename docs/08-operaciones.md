<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 2026 angiraulene1-create -->

# Tlaxtrelom Engine — Operaciones

> Versión 0.2 · Alcance: requisitos, build, pruebas, CI, diagnóstico, release y
> gestión de artefactos.

---

## 0. Requisitos

| Herramienta | Versión mínima | Uso |
|---|---|---|
| Rust estable | 1.83 (MSRV en `Cargo.toml` y `clippy.toml`) | runtime, `xtask` |
| `g++` | C++20 | isla de geometría y HarmonyOS |
| `make` | cualquiera POSIX | build C++/C sin CMake |
| `zstd` (dev) | 1.4 | compresión de páginas |
| Zig | 0.14 | contrapartida `spatial.zig` de la cinemática SIMD (**no** la invoca CI: no hay build de Zig) |
| `glslc` + `spirv-tools` | vigentes | shaders GLSL→SPIR-V (CI) |
| Python | 3.11 | pipeline y CLI |
| Lua | 5.4 | tests del sandbox |
| Node | 22.6 | TypeScript nativo (sin `tsc`) |
| Go | 1.22 | relay FullDive (opcional: `test_relay.py` corre `go test` sólo si `go` está en `PATH`) |
| .NET SDK | 8 (sin workload nativo) | SDK C# (opcional: `dotnet/test_csharp.py` valida el contrato sin compilador si `dotnet` falta) |
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
cargo xtask test          # Rust (--workspace)
cargo xtask test-all      # Rust + isla C++
cargo test -- --ignored   # pruebas de GPU offscreen (requiere hardware local)
cargo xtask native-test   # smoke test de la ABI C++ (geometría)
cargo xtask spatial       # smoke test C de la cinemática espacial (Zig no se compila)
cargo xtask harmonyos     # smoke test del puente HarmonyOS
cargo xtask shaders       # GLSL→SPIR-V con glslc + spirv-val
cargo xtask python        # 40 tests Python
cargo xtask lua           # 14 tests Lua
cargo xtask node          # 24 tests TypeScript
cargo xtask dotnet        # tests del SDK C#
cargo xtask relay         # tests del relay Go
cargo xtask docs          # enlaces locales de la documentación
```

Referencias verificadas: `cargo test -p tlaxtrelom-core --all-features` deja
126 pruebas pasando y 3 ignoradas (GPU); sin features, 125. En `xtask`, otras
18 sobre el empaquetado.

La matriz de CI (`cargo xtask ci`) ejecuta, en este orden:

1. `cargo fmt --all -- --check`
2. `cargo clippy --workspace --all-targets --all-features -- -D warnings`
3. geometría C++: `make -C native/tlaxtrelom-geometry all` + `test`
4. spatial: `make -C native/tlaxtrelom-spatial test`
5. HarmonyOS: `make -C native/tlaxtrelom-harmonyos test`
6. shaders GLSL→SPIR-V con `glslc` + `spirv-val`
7. `cargo test --workspace`
8. `cargo test --workspace --features native-geometry`
9. C#: `python3 dotnet/test_csharp.py`
10. relay Go: `python3 services/tlaxtrelom-relay/test_relay.py`
11. Python, Lua y Node
12. selftest headless con `--json`
13. validación de enlaces de documentación

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
| `--sector M` | alcance del sector en metros (1–1e8, defecto 10 000): mueve el `near` en 3D y fija el alcance del streamer, el `far` de la proyección y la niebla |
| `--msaa N` | antialiasing: 1, 2 o 4 muestras (se negocia contra el target HDR) |
| `--dimension MODO` | 2d, 2.5d, 3d (defecto), 3.5d, 4d, 4.5d o 5d |
| `--dimension-blend F` | 0–1: orto↔perspectiva en 3.5d, morph 3D↔4D en 4.5d |
| `--ticks N` | ticks del selftest/headless |
| `--telemetry-every N` | frames entre resúmenes de telemetría |
| `--replay ARCHIVO` | reproduce un log `.tlxneuro` |
| `--fulldive` | enciende la pila FullDive (experimental, apagada por defecto) |
| `--no-fulldive` | modo seguro: intención neutral |
| `--secure` | confianza discreta alta, sin mock |
| `--json` | salida JSON del selftest |

Un flag desconocido es un error, no un *silent ignore*.

---

## 4. Diagnóstico

### 4.1 Selftest

Salida humana:

```text
tlaxtrelom-engine 0.3.2 — selftest OK
  ticks           : 600
  sim hash        : 0x728977e58f08e538
  determinismo    : sí
  pasos válidos   : 599
  pasos gated     : 1
  frames acción   : 17
  XR frames       : 240 (late 0, scale 1.00)
  páginas geom.   : 0
  HAL             : linux x86_64 · 16 cores · ...
  tiempo          : 34.223 ms
```

`--json` expone los mismos campos para automatización. `ok` es vinculante: CI
falla si es `false`. Las `páginas geom.` son `0` con las features por defecto:
el streamer TLXG sólo enlaza cuando se construye con `native-geometry` y el
asset `assets/demo_cluster.tlxg` está presente.

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

El workflow usa un único runner `ubuntu-latest` con `build-essential`,
`pkg-config`, `libzstd-dev`, `glslang-tools` (`glslc` + `spirv-val`), Lua 5.4,
las cabeceras X11/Wayland que pide `winit`, Rust stable con `rustfmt` y
`clippy`, Python 3.12 y Node 24. **No instala Zig, Go ni el SDK de .NET**: esos
tres pasos se ejecutan sobre el contrato validado por Python, y sólo invocan el
toolchain nativo si ya está en `PATH`. No hay matriz multi-OS ni GPU en CI: los
3 tests Rust de render HDR/present están marcados `#[ignore]` y solo corren en
hardware local con `cargo test -- --ignored`.

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
6. Binario firmado; hash publicado junto con la release.
7. Publicación abierta: tag, notas de release y artefactos firmados en el
   repositorio público.

El modelo de distribución es publicación abierta con binarios firmados. El
código fuente, la documentación y los artefactos cocinados se publican bajo
licencia MIT (ver `docs/04-licencias.md`); cada binario va firmado y con su
hash verificable.

### 6.1 Empaquetado (`cargo xtask package`)

`crates/xtask/src/package.rs` compila, ordena y archiva los paquetes en `dist/`
a partir de las definiciones versionadas de `packaging/`:

```bash
cargo xtask package                     # la plataforma del host
cargo xtask package --platform linux    # .tar.gz (layout FHS) + .deb + install.sh
cargo xtask package --all               # linux, windows y harmonyos
cargo xtask package --platform windows --cross       # exige la tripleta instalada
cargo xtask package --skip-build --out /tmp/prueba   # empaqueta el binario que ya está
cargo xtask package-check               # valida packaging/ sin compilar (corre en ci)
```

| Plataforma | Portable | Instalable | Qué hace falta además |
|---|---|---|---|
| linux | `tlaxtrelom-<v>-x86-64-unknown-linux-gnu.tar.gz` | `tlaxtrelom_<v>_amd64.deb` e `install.sh` (`PREFIX`, por defecto `$HOME/.local`) | `tar`; `dpkg-deb` o `ar` para el `.deb` |
| windows | `.zip` con `tlaxtrelom.exe`, `tlaxtrelom.cmd` y `tlaxtrelom.nsi` | `tlaxtrelom-setup-<v>.exe` | `makensis` (NSIS) y la tripleta `x86_64-pc-windows-msvc` |
| harmonyos | `.zip` con `bin/`, `assets/`, `shaders/` y el árbol `hap/` (`module.json5`, `EntryAbility.ets`, `entry/libs/<abi>/`) | `install.sh`, que vuelca el árbol o instala el HAP por `hdc` | `hdc` en el PATH y el NDK (`OHOS_NDK_HOME`) para la `.so` de la ABI del paquete |

Reglas que sostienen el empaquetado:

- **Un solo layout en Linux.** El árbol portable ya es FHS (`bin/` +
  `share/tlaxtrelom/{assets,shaders}` + `applications`/`icons`/`metainfo`), así
  que `install.sh` y el `.deb` copian en vez de reorganizar. Como el motor busca
  `demo_cluster.tlxg` relativo al ejecutable (`bin/../share/tlaxtrelom/assets`,
  entre otras candidatas), el paquete funciona lanzado desde cualquier
  directorio: no hay que `cd` antes de arrancar.
- **Degradar, no fingir.** Falta una herramienta ⇒ se omite *ese* artefacto
  concreto y el aviso dice cómo conseguirlo; ningún archivo se escribe
  presentándolo como construido cuando no lo está. `--skip-build` no compila
  nada y `--debug`/`--release` eligen el perfil.
- **La ABI manda antes que el paquete.** Una `libtlaxtrelom_harmonyos.so`
  compilada en el host (x86-64) no entra en `entry/libs/arm64-v8a/`: se descarta
  con un aviso, porque un HAP con la ABI equivocada falla en el dispositivo y no
  en la máquina que empaqueta.
- **Windows y HarmonyOS no están verificados en CI.** La matriz corre en un
  runner Linux con una sola tripleta instalada; lo que se garantiza acá es el
  árbol, los guiones y las definiciones (`package-check`), no el binario
  cruzado. Ver `docs/09-multiplataforma.md`.

`package-check` —que `cargo xtask ci` ejecuta en cada commit— comprueba sin
compilar que las rutas que declara `tlaxtrelom.nsi` son rutas que el empaquetado
realmente entrega, que los JSON5 de HarmonyOS están balanceados y referencian
recursos que existen en el árbol, que `control` mantiene las claves obligatorias
y que los guiones `sh` no llevan expansiones escapadas (`\${...}`, que ningún
shell evalúa) y pasan `sh -n`.

---

## 7. Versionado

| Contrato | Valor actual |
|---|---|
| Versión del workspace | 0.3.2 |
| MSRV | Rust 1.83 (`rust-version`, `clippy.toml`) |
| ABI de geometría (`GEO_ABI_VERSION`) | 2 |
| Formato cocinado | `TLXG` v2 (origen de sector en `double`, LOD en `double`, frustum culling) |
| ABI spatial (C/Zig) | 1 |
| ABI HarmonyOS | 1 |
| ABI HAL C11 | 1 (`tlaxtrelom_hal_abi_version()`) |
| Schema HIR | `tlaxtrelom.ir/1` |

- `0.x`: contrato en evolución; los cambios incompatibles suben la versión
  menor y el schema/ABI correspondiente.
- `GEO_ABI_VERSION` y el magic `TLXG` son contratos independientes de la
  versión del motor: v1 se rechaza en carga (`ERR_FORMAT`).
- En el lado Rust, `View::identity()` pasó a `View::neutral()` (perspectiva
  de 90°); la isla usa esa vista para el frustum culling de `TLXG` v2.
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

---

## 9. Licencia y publicación

- El proyecto es **MIT**: todos los archivos y manifiestos llevan
  `SPDX-License-Identifier: MIT` (ver `docs/04-licencias.md`).
- `publish = false` en el workspace y los crates, `"private": true` en
  `tools/tlaxtrelom-ts/package.json` y el classifier `Private :: Do Not
  Upload` se mantienen a propósito: son guardias contra publicaciones
  accidentales en registries, no señales de código cerrado.
- Publicar en crates.io, PyPI, npm o NuGet es una decisión explícita, no un
  paso por defecto del release.
