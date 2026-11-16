<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 2026 angiraulene1-create -->

# Registro de cambios — Tlaxtrelom Engine

Formato basado en [Keep a Changelog](https://keepachangelog.com/es-ES/1.1.0/).
El proyecto está bajo licencia MIT; `publish = false` y `private = true` siguen
ahí a propósito, así que publicar en un registry sigue siendo una decisión
explícita y no un accidente.

## [Sin publicar]

- El workflow de release pasa `--prerelease` a `gh release create/edit` cuando
  la etiqueta lleva sufijo semver: `gh` **no** detecta el sufijo solo
  (`v0.4.0-alpha.1` se publicó como release estable y hubo que marcarlo a
  mano). Verificado el release ya corregido: pre-release, aviso en las notas,
  `alpha` en el nombre de los seis artefactos, glibc máxima exigida 2.34,
  AppImage y binario del `.deb` arrancando de verdad y `SHA256SUMS` 6/6.

## [0.4.0-alpha.1] — 2026-11-16

### Cambiado

- **El proyecto se marca alfa de forma explícita (experimental/conceptual):**
  la versión pasa a `0.4.0-alpha.1` en todos los manifiestos (workspace Rust +
  lock, TS, Python, .NET, Lua, empaquetados y `docs/08`), de modo que `alpha`
  aparece en el nombre de cada artefacto; GitHub reconoce la etiqueta como
  **pre-release** y las notas del release encabezan con un aviso de etapa
  temprana. El README lleva el mismo banderín. `versionCode` de HarmonyOS
  sube a 5 y el metainfo declara `type="development"`.
- El job Linux del workflow de release construye en `ubuntu-22.04`
  (glibc ≥ 2.35) en vez del runner más nuevo: los binarios de 0.3.x exigían
  glibc 2.43 y el AppImage no arrancaba en distros algo más viejas. Como en
  22.04 no existe ningún paquete `glslc`, `cargo xtask package` deja de
  recompilar ahí el SPIR-V de contrato (`make native shaders` pasa a
  `make native`): los `.spv` viajan desde el árbol, donde el paso `shaders`
  de ci.yml los valida contra sus fuentes GLSL en cada commit.

### Corregido

- Smoke test de los seis artefactos de `v0.3.2` (bajar y ejecutar de verdad):
  AppImage en ventana (~1100 fps en RADV Renoir), `tar.gz`, `.deb` dpkg
  extraído con `ar` y su binario lanzado, `.zip` con sus nueve entradas de
  shaders y el instalador NSIS verificado con `7z` (13 archivos, desinstalador
  incluido) responden bien; los `.exe` de Windows no se pudieron ejecutar sin
  wine, solo verificación estructural. El único defecto encontrado fue el de
  la glibc, corregido arriba; recordatorio para quien baja el AppImage: hay
  que darle permiso de ejecución (`chmod +x`) después de descargarlo.

## [0.3.2] — 2026-11-16

### Cambiado

- Versión 0.3.2 en todos los manifiestos (workspace Rust + lock, TS, Python,
  .NET, Lua, empaquetados y `docs/08`). El `versionCode` de HarmonyOS sube a 4.

### Corregido

- Dos fallos que la corrida de `v0.3.1` (la primera disparada por el push de
  una etiqueta) destapó en cuanto NSIS y dpkg llegaron a ejecutarse de
  verdad, ahora con avisos que los delatan en vez de degradar en silencio:
  `tlaxtrelom.nsi` contenía `UninstallButton true`, que no es un comando de
  NSIS, y `makensis` abortaba con exit 1 (línea 26) — eliminado; y la
  plantilla `packaging/linux/control` empezaba con comentarios `#` (SPDX),
  que el parser del fichero de control de dpkg rechaza
  (`parsing file ... near line 0`, exit 2) — `xtask package` filtra ahora
  las líneas de comentario al generar `DEBIAN/control`, y la validación
  local con un `dpkg-deb` de mentirijillas exige campos obligatorios sin
  comentarios.

## [0.3.1] — 2026-11-16

### Cambiado

- Versión 0.3.1 en todos los manifiestos (workspace Rust + lock, TS, Python,
  .NET, Lua, empaquetados y `docs/08`). El `versionCode` de HarmonyOS sube a 3
  para permitir la actualización inplace sobre el 0.3.0.

### Añadido

- `.github/workflows/release.yml`: al publicar una etiqueta `v*` (o por
  `workflow_dispatch`) construye en GitHub los instaladores de Linux y Windows
  con `cargo xtask package`, y los adjunta junto a `SHA256SUMS` al Release, con
  las notas recortadas del CHANGELOG. El instalador de HarmonyOS es opt-in
  mediante la variable de repositorio `OHOS_NDK_URL` y best-effort: su fallo no
  bloquea el release.

### Corregido

- `xtask package` en Linux: el `.deb` se armaba pero `dpkg-deb -b` fallaba en
  cualquier máquina con dpkg (CI de Ubuntu) porque el metadata se escribía en
  un directorio hermano y nunca se colocaba en `DEBIAN/` dentro del árbol; en
  local pasaba desapercibido porque sin dpkg se usa el contenedor ar+tar. Los
  `aviso:` del release workflow tampoco bastaban: choco instala NSIS en el PATH
  de la máquina y los pasos siguientes no lo heredan, así que `makensis` nunca
  se encontró y el instalador salía como respaldo (4,6 KB) incluso con NSIS
  instalado. `GITHUB_PATH` corrige lo segundo; el job Linux instala además
  `flatpak`/`ostree`, y `publicar` recalcula `SHA256SUMS` sobre todos los
  assets finales del release, no sólo los de la corrida.
- Tercer fallo destapado por la segunda corrida del release: `make_setup_exe`
  invocaba `makensis` sin fijar el directorio de trabajo, y como NSIS resuelve
  `OutFile` relativo al CWD, el instalador real caía fuera del árbol; xtask lo
  daba por fallido y el respaldo de 4,6 KB (`win_setup.c`) sobrescribía en
  `dist/` al `tlaxtrelom-setup-<versión>.exe` legítimo sin emitir un solo aviso.
  Ahora makensis corre con el árbol como CWD, y si se degrada al respaldo queda
  publicado en los avisos. Además, el paso de sumas ya no usa
  `gh release download --exclude` (flag inexistente en el gh de los runners):
  descarga todos los assets y descarta la suma vieja a mano.

## [0.3.0] — 2026-11-16

### Añadido

- Licencia MIT en todo el repositorio: `LICENSE` es ahora la fuente de verdad y
  cada archivo fuente declara `SPDX-License-Identifier: MIT` con
  `Copyright (c) 2026 angiraulene1-create`, en el estilo de comentario propio de
  cada lenguaje. Se abandonan la referencia `LicenseRef-Tlaxtrelom-Proprietary`
  y la marca `CONFIDENTIAL`.
- Los manifiestos declaran la licencia: `license = "MIT"` en el workspace Rust
  (heredado por ambos crates), `PackageLicenseExpression` MIT en el csproj,
  `license: MIT` en `package.json` y `license = { text = "MIT" }` con su
  classifier en `pyproject.toml`.
- `docs/04-licencias.md` sustituye a `docs/04-propiedad-intelectual.md`: régimen
  MIT, DCO en lugar de CLA, la misma allowlist de dependencias permisivas, política de
  marca (MIT no licencia el nombre) y las obligaciones de datos BCI, que no
  dependen de la licencia. `deny.toml` y la lista de documentos de `xtask` siguen
  el renombrado, y la validación de enlaces cubre ahora `docs/09`.
- Documentación alineada con el código: mapa real de crates y módulos, pipeline
  de dos pases con tonemap ACES, coordenadas de dos capas y alcance de sector,
  versions de ABI (geometría 2, spatial 1, HarmonyOS 1, HAL 1), la matriz de
  `cargo xtask ci` completa y los conteos de pruebas actuales. Lo que no está
  implementado se marca como contrato en lugar de describirse como existente.
- Se eliminan afirmaciones que el código nunca sostuvo: flags de CLI que no
  existen en `config.rs`, crates fuera de `tlaxtrelom-core` y `xtask`, un
  `HAL 1.1.0` (la ABI es el entero `1`), P/Invoke en el SDK C# (sólo hay structs
  espejo), `zig build` en CI (ningún paso compila `spatial.zig`) y el estéreo
  como «dos pases, uno por ojo» (es un pase de escena con un draw recortado por
  ojo más el pase de presentación). `docs/07-estabilidad.md` § 9 registra como
  deuda lo que falta: la apertura de `ComfortVignette` no llega a ningún uniform
  del frame WGSL y `main()` del relay Go no abre socket.

- Render a un target HDR lineal (`Rgba16Float`, con caída a `Rgba32Float` y a
  8 bits si el adaptador no los acepta) seguido de un pase de presentación con
  tonemap ACES y dither, en vez de sombrear y tonemapear en un único paso con
  salida directa al swapchain. `shaders/present.wgsl` es el nuevo paso;
  `scene3d.wgsl` ahora sólo entrega color lineal.
- El MSAA vive ahora en el target HDR, no en el swapchain: el tonemap es un
  triángulo fullscreen sin geometría que antialias. Se eliminaron el attachment
  y el pipeline multisample del swapchain, que estaban obsoletos desde que el
  HDR existe.
- Pruebas de GPU offscreen (`renderer::tests::gpu_frame_renders_and_presents_*`,
  `#[ignore]`) que renderizan scene → HDR → present a 64×64, leen los píxeles de
  vuelta y comprueban que el resultado no es negro, no está saturado y cae en un
  rango de luma media razonable. Cubren MSAA 1×, 2× y 4×; verificadas en HW
  real (RADV RENOIR).
- Coordenadas de dos capas (`crates/tlaxtrelom-core/src/coords.rs`): el mundo
  vive en un anclaje `f64` (`WorldOrigin`) y se dibuja contra un offset `f32`
  (`LocalPose`). Al superar 1024 m de offset, la parte entera se pliega al
  anclaje (`rebase`) y el `f32` recupera precisión completa. El rebase es
  determinista y mueve a la vez `player`, `previous` y `render`, así que la
  posición global y el frame interpolado no saltan. `World::digest()` hashea el
  anclaje, de modo que el selftest sigue midiendo determinismo.
- Alcance de sector configurable (`--sector <m>`, hasta 1e8 m): el `near` se
  deriva del alcance en vez de quedar fijo en 5 cm, y la niebla y el `far` de la
  proyección salen del mismo rango (`DepthRange`).
- Pruebas Rust de la nueva invariante: IPD distinguible con un anclaje a 1e11 m,
  continuidad de la posición global a través del rebase, determinismo del hash
  lejos del origen y altura de ojos que nunca se pliega al anclaje.
- La isla C++ de geometría cocina y consume assets `TLXG` **v2**: la cabecera
  guarda el origen del sector en `double origin[3]` y los `center` de los
  clusters son relativos a él. `TlaxtrelomGeoView` gana `origin[3]` (`double`),
  la distancia de LOD se calcula en `double` y el recorrido del DAG hace
  **frustum culling** real (esferas contra los seis planos derivados de
  `view_proj`, convención *left-handed* con profundidad 0..1).
  `GEO_ABI_VERSION` pasa de 1 a 2.
- Prueba de la cadena completa: `abi_smoke` cubre poda por frustum, prioridades
  de LOD distinguibles sobre un anclaje de 1e11 m y rechazo de assets v1; en
  Rust, `the_cooked_demo_asset_streams_a_page` une cocinador ↔ streamer ↔
  puente.
- El selftest registra `assets/demo_cluster.tlxg` si existe y reporta páginas
  realmente drenadas (antes la línea «páginas geom.» siempre decía 0).

### Cambiado

- Versión del workspace subida a `0.3.0` en todos los manifiestos (Cargo, npm,
  pyproject, csproj, `app.json5` de HarmonyOS, instaladores y empaquetado de
  Linux/Windows), incluido el `metainfo.xml` con su fecha de publicación.
- `View::identity()` pasa a ser `View::neutral()` y deja de ser la identidad:
  construye una perspectiva de 90°. Con frustum culling en la isla, una matriz
  identidad significaría «lo visible es el cubo NDC unitario» y podaría cada
  cluster cocinado en metros.
- Runtime verificado: `123` pruebas Rust verdes (`--all-features`, con la isla
  C++ enlazada) más `3` de GPU offscreen bajo `--ignored`; `make -C
  native/tlaxtrelom-geometry test` verde; `--selftest` determinista en los seis
  modos dimensionales; y `36` combinaciones live sin errores de validación
  (`--msaa {1,2,4} × --dimension {2d,2.5d,3d,4d,4.5d,5d} × {--windowed,--stereo}`
  con `--sector 40000`).

### Corregido

- El AppImage generado no arrancaba: `make_appimage` empaquetaba el árbol portable
  tal cual, que sólo trae `bin/tlaxtrelom.sh`, y el runtime ejecuta `AppRun` en la
  raíz del árbol extraído (`execv` fallaba con ENOENT). Ahora se inyecta un `AppRun`
  en el payload (y solo en él: el `.tar.gz` y el Flatpak comparten árbol y quedan
  limpios). Verificado: `tlaxtrelom-0.3.0-x86_64.AppImage --selftest` OK.
- CI de GitHub en verde por primera vez (run 43926561337) con cinco correcciones:
  `cargo fmt` sobre `xtask/package.rs`; la geometría C++ se construye **antes** de
  `clippy --all-features` (su `build.rs` enlaza `native/tlaxtrelom-geometry/build`
  cuando `native-geometry` está activa); `ci.yml` instala `glslc` + `spirv-tools`
  en lugar de `glslang-tools` (las herramientas que `check_shaders` invoca);
  `services/tlaxtrelom-relay/go.mod` para que `go test` funcione en runners con Go;
  y `node --test` recibe el glob explícito `tests/*.test.ts` porque Node 24 (CI) no
  descubre archivos `.ts` escaneando un directorio, a diferencia de Node 26.
- `--msaa 4` entraba en pánico en tiempo de ejecución: el pase de presentación
  usaba el attachment multisample heredado del swapchain con un pipeline de un
  solo sample ("Incompatible sample count"). El swapchain ya no participa en el
  MSAA, así que ese attachment se eliminó en lugar de parchearse.
- `--msaa 2` fallaba al crear la textura de profundidad: `Depth32Float` sólo
  garantiza 1× y 4× según la especificación WebGPU; 2× requiere el feature
  `TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES`, que ahora se solicita al device
  cuando el adaptador lo ofrece. Sin esto, la negociación de `samples` (que sí
  consultaba el soporte real por adaptador) y la creación de la textura (que no)
  quedaban en desacuerdo.
- La negociación de MSAA se hace contra el formato HDR, no contra el formato del
  swapchain, que es donde realmente se aplica el multisample desde el cambio de
  arriba.
- Errores de `Config::parse` visibles: `Display` de `anyhow` sólo imprime el
  mensaje raíz, así que un `--dimension-blend 2.0` malo terminaba en un críptico
  "configuración dimensional inválida". `Config` aplana ahora su cadena de
  causas (`configuración XR inválida: render_scale fuera de [0.5, 2.0]: 9`).
- Escena 2D contenida en NDC: las líneas de rejilla de los bordes sobresalían a
  ±1.00125 y se recortaban a lo largo por la mitad dentro del viewport de cada
  ojo en `--stereo`.
- Primitivas de malla (`push_quad_xy`, `push_disc_xy`, `push_box`, `push_tri` y
  la rejilla de `create_demo_scene`) ya no truncan `vertices.len()` a `u16` en
  silencio: se cortan al agotar el presupuesto de índices.
- `cargo fmt --check --all` y `cargo clippy --workspace --all-features -- -D
  warnings` vuelven a estar verdes (MSAA copiable, rangos `RangeInclusive`,
  bucles por iterador y firma de `Renderer::draw` agrupada en `RenderTargets`).
- `rust-version` elevado a 1.83, que es el mínimo real del código (`let else`).
- Con coordenadas globales `f32`, la geometría perdía el sub-milímetro más allá
  de ~16 km del origen (allí el ULP del `f32` supera el milímetro) y el estéreo
  colapsaba a mono cerca del millón de metros, donde sumar medio IPD ya no
  cambia el número. Ahora la IPD se suma al *offset* local y el anclaje `f64` no
  participa en el redondeo.
- Lejos del origen, la cámara isométrica de 2.5D dejaba al jugador fuera de
  cuadro: su ojo y su objetivo estaban fijos en el `(0,0,0)` global y su rango
  ortográfico no llegaba a un sector entero. Sigue el offset local.
- El LOD del streamer colapsaba al burdo en mundos grandes: la distancia se
  calculaba en `float` contra centros absolutos, así que clusters separados
  cientos de metros a 1e11 m daban exactamente la misma distancia (allí un ULP
  `f32` son 8,2 km).
- El streamer copiaba `view_proj` en su vista pero nunca la leía ni usaba el
  `radius` de los clusters: no había frustum culling y se pedían páginas de
  geometría a espalda de la cámara.
- `--selftest` no registraba ningún asset, así que su recuento de páginas de
  geometría no decía nada del camino real.

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
