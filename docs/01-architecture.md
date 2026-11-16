<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 2026 angiraulene1-create -->

# Tlaxtrelom Engine — Arquitectura del sistema

> Versión 0.2 · Documento vivo · Autor: Arquitectura Principal
> Alcance: runtime, fronteras FFI, modelo de hilos, build y ADRs.
> Este documento distingue en cada sección el **estado implementado** (lo que
> compila y corre hoy en el repo) del **destino** (contratos y diseño planeado).
> Donde no se diga lo contrario, el detalle descrito es destino.

---

## 0. Resumen ejecutivo

Tlaxtrelom es un motor **Rust-first** con islas nativas:

- **C++/C** para geometría virtual, HAL y puentes de plataforma. Es código que
  ya existe, que necesita control fino de memoria/SIMD o que envuelve SDKs.
- **Python y TypeScript** para el pipeline de contenido y la autoría del IR.
  Nunca para el frame loop de un juego publicado.

El motor es **software abierto bajo licencia MIT**: repositorio público, sin
CLAs ni NDA para usarlo. La política de licencias, dependencias y aviso de
copyright vive en `docs/04-licencias.md`; este documento asume esas reglas.
`publish = false` (Rust) y `"private": true` (Node) se conservan a propósito
como guardia contra publicaciones accidentales en registries: publicarlos es
una decisión explícita, no un descuido.

La frontera es una **ABI C plana** (`extern "C"`), versionada, con handles
opacos y contratos de ownership explícitos. Rust↔C++ cruza con `unsafe` +
`bytemuck` sobre structs POD; no se usa `cxx` ni generadores de binding.
Python y TypeScript no enlazan el runtime: solo hablan con el motor vía el
**HIR JSON canónico**. El objetivo no es "que los tres lenguajes se llamen
mucho", sino **que se llamen poco y bien**.

### Las cinco decisiones que definen el motor

1. **Rust posee el frame loop y la memoria del runtime.** No hay GC en el hilo
   de render; no hay `shared_ptr` atravesando la FFI.
2. **C++ es una isla, no un co-runtime.** Se invoca por lotes (comando de frame,
   páginas de geometría) y devuelve datos en buffers compartidos.
3. **Python es una herramienta, no un ciudadano del runtime.** Ni siquiera
   enlaza el motor hoy: solo intercambia HIR JSON. El editor lo usará embebido;
   el juego exportado nunca.
4. **Un solo IR para tres frontends.** Texto, nodos y bloques bajan a `Tlaxtrelom
   HIR`: hoy es **JSON ordenado con hash blake2b512-128**, implementado en Python
   y TypeScript. Los backends SPIR-V/LLVM/C++ codegen son contrato, no código.
   Ver `docs/03-tlaxtrelom-ir.md`.
5. **FullDive es un complemento experimental en segundo plano (opt-in, desactivado por defecto).**
   El motor funciona de fábrica como un motor gráfico normal. El subsistema BCI no
   consume ciclos ni abre transportes a menos que se invoque intencionalmente con
   `--fulldive` o `--replay`. Cuando se enciende, corre como servicio desacoplado
   en un hilo RT aislado vía triple buffer wait-free (`NeuralSource` → `IntentFrame`),
   protegido por un *IntentGate*, watchdog biométrico y desacoplamiento suave de 300 ms.

---

## 1. Objetivos y no-objetivos

### Objetivos

- **Arquitectura gráfica de próxima generación al nivel de Unreal Engine 5 y futuro UE6 o superior**:
  - Renderizado agnóstico Vulkan/DX12/Metal sobre `wgpu` con target HDR lineal (`Rgba16Float`),
    tonemapping ACES (RRT+ODT), shaders de foveación y estéreo multiview.
  - Geometría virtualizada con *cluster DAG*, streaming asíncrono I/O con compresión zstd,
    jerarquía de LOD adaptativa y frustum culling por esferas (arquitectura inspirada en Nanite).
  - Coordenadas espaciales masivas en dos capas (`WorldOrigin` en `f64` + `LocalPose` en `f32`)
    con rebase determinista a 1024 m y alcance `--sector` de hasta 100 000 km, evitando el colapso
    del ULP de precisión flotante a distancias astronómicas.
- **Soporte multidimensional nativo completo**:
  - Modos de proyección espacial **2D, 2.5D, 3D, 3.5D, 4D, 4.5D y 5D** de primera clase,
    con matrices de proyección continuas, morph de hipercubo N-D y blend ortográfico↔perspectiva.
- **Simulación determinista a paso fijo (120 Hz)** con prevención de espirales de la muerte
  (`max_sim_steps`) y verificación de integridad determinista por digest (`--selftest`).
- **Pipeline de autoría unificado**: bloques, nodos visuales y código fuente bajan a un mismo
  IR canónico normalizado (`Tlaxtrelom HIR` con hash BLAKE2b-512 truncado a 128 bits).
- **Complemento experimental FullDive en segundo plano**: stack BCI de baja latencia ($L_{proc} < 1\text{ ms}$)
  con límites de seguridad somatosensorial (50 µA, 50 ms, anti-resonancia) y watchdog biométrico.
- **Empaquetado multi-OS integrado**: generación automatizada de paquetes portables e instaladores
  para **Linux** (`.tar.gz` + `.deb`), **Windows** (`.zip` + `tlaxtrelom.exe` + `.nsi` + `install.ps1`)
  y **HarmonyOS for PC** (`.zip` + `.hap` + `install-harmonyos.sh`).

### No-objetivos (explícitos)

- **No** compilamos C++ arbitrario ni Python arbitrario al mismo AST. Eso exige
  reimplementar un compilador de C++ y la semántica dinámica de Python. Lo que
  hacemos es definir un **subconjunto de kernel** tipado y determinista que los
  tres lenguajes pueden expresar mediante anotaciones; el código *host* sigue
  siendo nativo. Hoy ese subconjunto es contrato: no hay compilador de kernels
  ni backend SPIR-V/LLVM en el repo. Ver `docs/03-tlaxtrelom-ir.md § 1`.
- **No** prometemos latencia end-to-end < 1 ms para intención cortical. La
  ventana fisiológica (50–300 ms) es irreducible. El < 1 ms es el presupuesto
  *última muestra → input aplicado*. Ver `docs/02-fulldive.md § 8`.
- **No** hay "física cuántica" en el sentido de un QPU en tiempo real. Hay
  solvers de tensor networks/campos cuánticos simulados en GPU, opcionales y
  fuera del camino determinista. Ver ADR-010.
- **No** imitamos la sobrecarga monolítica de motores tradicionales: la arquitectura
  apunta a competir y superar las capacidades clave de UE5 y futuro UE6 (geometría virtualizada
  en clusters DAG tipo Nanite, HDR lineal ACES, coordenadas masivas en 64-bit y proyección
  multidimensional 2D..5D) mediante un runtime modular, seguro y determinista sin deuda técnica.

---

## 2. Vista de capas

```text
┌──────────────────────────────────────────────────────────────────────────────┐
│                           EDITOR / HERRAMIENTAS                              │
│  UI (Rust + egui) · Grafo visual · Bloques · Inspector · Timeline · Debugger │
│  Python embebido (solo editor): importadores, validadores, automatización    │
├──────────────────────────────────────────────────────────────────────────────┤
│                       FRONTENDS DE AUTORÍA  (→ Tlaxtrelom HIR)                     │
│  Texto anotado (Rust/C++/Python)  ·  Nodos (dataflow)  ·  Bloques (secuencia)│
├──────────────────────────────────────────────────────────────────────────────┤
│                         TLAXTRELOM IR  (HIR → MIR → backends)                      │
│  typeck · effectck · determinismo · autodiff · vectorización · kernel fusion │
│  HIR hoy: JSON ordenado + hash blake2b512-128 (Python/TS). Backends         │
│  SPIR-V · LLVM/Cranelift · Rust codegen · C++ stubs: contrato, sin código   │
├──────────────────────────────────────────────────────────────────────────────┤
│                          RUNTIME RUST  (host / shipping)                     │
│  world · scheduler de frame · assets · input · XR · headless selftest       │
│  ┌──────────────┐ ┌──────────────┐ ┌──────────────┐ ┌─────────────────────┐ │
│  │ renderer     │ │ fulldive     │ │ world/sim    │ │ geometry bridge     │ │
│  │ wgpu HDR     │ │ gate+metrics │ │ 120 Hz fijo  │ │ unsafe + POD        │ │
│  └──────────────┘ └──────────────┘ └──────────────┘ └─────────────────────┘ │
├──────────────────────────────────────────────────────────────────────────────┤
│                        ISLAS NATIVAS  (C++ / C / firmware)                   │
│  tlaxtrelom-geometry (C++20, ABI 2) · tlaxtrelom-hal (C11, ABI 1)           │
│  tlaxtrelom-spatial (C, ABI 1)      · tlaxtrelom-harmonyos (C++, ABI 1)     │
│  firmware BCI (Rust no_std, Embassy): contrato, aún no en el repo           │
├──────────────────────────────────────────────────────────────────────────────┤
│                            HAL / PLATAFORMA                                  │
│  Vulkan · DX12 · Metal · OpenXR · USB HS / Ethernet · audio · filesystem     │
└──────────────────────────────────────────────────────────────────────────────┘
```

### 2.1 Mapa de crates y módulos

**Estado actual.** El workspace Rust tiene dos crates (`Cargo.toml:6`). Lo que
antes se dibujaba como crates separados vive hoy como módulos de
`tlaxtrelom-core`:

```text
crates/
  tlaxtrelom-core      # todo el runtime, en módulos:
    #  lib.rs main.rs config.rs        arranque y flags (--dimension, --sector, --probe)
    #  world.rs coords.rs dimension.rs simulación 120 Hz, anclaje f64+offset f32, modos dimensionales
    #  geometry.rs mesh.rs             puente FFI a la isla C++ (POD + unsafe), TLXG v2
    #  renderer.rs xr.rs hal.rs        wgpu HDR + present, OpenXR/XR, envoltura de la HAL
    #  fulldive.rs gate.rs metrics.rs  fuente neural, gate/watchdog, contadores
    #  headless.rs                     selftest sin ventana (--json)
  xtask                # orquestador de CI y builds (crates/xtask/src/main.rs)
native/
  tlaxtrelom-geometry   # C++20: cluster DAG + streamer + frustum culling; GEO_ABI_VERSION = 2
  tlaxtrelom-hal        # C11 ABI 1: compilado por build.rs con `cc`, enlazado vía feature native-hal
  tlaxtrelom-spatial    # C11: ABI 1 (`spatial.zig` es contrapartida, sin build en CI)
  tlaxtrelom-harmonyos  # C++ puente ArkUI: ABI 1
python/tlaxtrelom_tools/   # tools de contenido + HIR JSON
tools/tlaxtrelom-ts/       # TypeScript: HIR JSON y bindings de autoría
script/                    # Lua: scripting de contenido
dotnet/Tlaxtrelom.Engine/  # bindings .NET
services/tlaxtrelom-relay/ # relay en Go
```

**Destino.** Los siguientes crates aún **no existen**; son el desglose planeado
cuando los módulos de `tlaxtrelom-core` superen el umbral de separación:

```text
crates/ (destino, no existe)
  tlaxtrelom-render      # device wgpu, frame graph, passes, virtual geometry GPU
  tlaxtrelom-ir          # HIR/MIR, parser, passes, codegen
  tlaxtrelom-graph       # frontend de nodos (JSON → HIR)
  tlaxtrelom-blocks      # frontend de bloques (árbol → HIR)
  tlaxtrelom-fulldive    # transporte, DSP, decoder, gate, calibración
  tlaxtrelom-neuro-proto # protocolo binario compartido host↔firmware (no_std)
  tlaxtrelom-bindings    # exportación multilingüe del HIR JSON
  tlaxtrelom-cli         # CLI (hoy: main.rs dentro de core + xtask)
firmware/
  fulldive-mcu/    # Rust no_std: ADC, DMA, framing, CRC, watchdog (contrato)
```

No hay islas `tlaxtrelom-fluids` ni `tlaxtrelom-haptics`: se mencionaban como
destino y no tienen código ni carpeta.

### 2.2 Coordenadas: anclaje `f64` + offset de render

Un mundo de alcance interestelar no se puede representar con una coordenada
`f32` absoluta, y ese límite es del formato IEEE, no del motor. El `f32` guarda
24 bits de mantisa, así que la resolución absoluta es ~1/16.7 millones de la
magnitud:

- Más allá de 2^14 m (~16 km) el ULP supera el milímetro: la geometría tiembla y
  el depth buffer deja de repartir precisión donde hace falta.
- Más allá de 2^20 m (~1000 km) sumar medio IPD (3,15 cm) a la posición del ojo
  no cambia el número: el estéreo colapsa a mono sin ningún error visible.
- A 1e11 m un ULP son 8,2 km y a 1e12 m, 65 km: dos clusters separados cientos de
  metros dan exactamente la misma distancia y el LOD cae al burdo.

Ningún motor que pretenda escala grande dibuja en coordenadas absolutas.

El runtime separa dos capas (`crates/tlaxtrelom-core/src/coords.rs`):

| Capa | Tipo | Qué guarda |
| --- | --- | --- |
| Anclaje global | `WorldOrigin.anchor: [f64; 3]` | Dónde está el origen de render, en metros. Acumula el recorrido. |
| Offset de render | `LocalPose.offset: [f32; 3]` | Posición relativa a ese origen: siempre de cientos de metros. |

Todo lo que mira a la GPU —matrices de vista, IPD, posiciones de vértice, niebla,
distancias del streamer— se calcula contra el **offset**. El anclaje nunca entra
en un shader.

- **Rebase.** Al pasar de `REBASE_THRESHOLD` (1024 m) en un eje, la parte entera
  de ese eje se suma al anclaje y el offset queda cerca de cero. Es aritmética
  IEEE determinista (`f64 += f64`), se aplica a `player`, `previous` y `render` a
  la vez, y no pliega el eje Y por debajo del umbral para no corromper la altura
  de ojos. `World::digest()` hashea el anclaje, así que el selftest detecta
  cualquier deriva.
- **Sectores.** El flag `--sector <m>` desplaza el anclaje inicial hasta 1e8 m;
  el `DepthRange` se deriva del alcance del sector, no se fija a ciegas.
- **Vista hacia la isla.** `GeoView` (ABI v2) lleva `origin[3]` en `double` más
  la matriz de proyección; la isla deriva de ahí su frustum y poda los clusters
  fuera de vista. No existe `View::identity()`: el constructor neutro es
  `View::neutral()`, y una matriz inválida (ceros o NaN) se rechaza, no se
  tolera.
- **Assets.** Un `.tlxg` (TLXG v2) guarda su origen de sector en `double` y los
  centros de cluster en `float` relativos a él. La isla C++ suma
  `asset_origin + center` y resta `view.origin + camera` en `double` para la
  distancia de LOD.
- **Límite honesto.** `f64` mantiene resolución milimétrica hasta ~9e9 m del
  anclaje (unas pocas UA): sobra para un sistema planetario local, no para una
  galaxia entera de una pieza. El contenido galáctico se compone en *sectores*
  —cada sector rebasa el origen y el anclaje registra dónde cae—, que es el mismo
  modelo de Star Citizen o Elite Dangerous. Un motor que promete "años luz en
  coordenadas absolutas" miente o renderiza cerca del origen y finge el resto.

---

## 3. La frontera FFI

### 3.1 Matriz de comunicación

| Desde → Hacia | Mecanismo | Frecuencia | Datos | Estado |
|---|---|---|---|---|
| Rust → C++ (geometry) | `extern "C"` + `unsafe` + `bytemuck` POD | 1/frame + por página | `GeoView` v2, requests, stats | implementado |
| Rust → C (hal) | enlace estático vía `build.rs` con `cc`, feature `native-hal` | en build | funciones C puras | implementado |
| Rust ↔ Spatial / HarmonyOS | ABI C plana versionada (v1) | por llamada | POD, handles opacos | implementado |
| Python / TypeScript / Lua / .NET ↔ Motor | **sin enlace nativo**: HIR JSON canónico (bytes, hash blake2b512-128) y archivos `.tlxg` | por comando de tooling | JSON bin, páginas TLXG | implementado |
| C++ → Rust | structs POD devueltos por valor (`GeometryStats`, páginas listas) | 1/frame | stats, páginas | implementado |
| Firmware → Host | USB HS / Ethernet, protocolo binario | 1 kHz | muestras EEG/EMG | contrato |

No hay `cxx`, ni PyO3/maturin, ni manejo de GIL en el repo: la frontera
caliente es C plano y el cruce Rust↔C++ usa `unsafe` acotado con validación de
ABI (`GEO_ABI_VERSION = 2`). Python no enlaza el runtime ni siquiera en
tooling: solo intercambia HIR JSON y `.tlxg`.

### 3.2 Reglas no negociables

1. **ABI C plana en la frontera caliente.** Nada de `std::string`, `std::vector`,
   `Result<T,E>`, `Box<dyn Trait>` ni excepciones cruzando.
2. **Handles opacos.** `TlaxtrelomGeoStreamer*` es un puntero a un tipo incompleto; el
   consumidor no conoce su layout ni su tamaño.
3. **Ownership explícito.** "Quien asigna, libera". Los buffers que cruzan se
   documentan como *borrowed for the duration of the call* o *owned by the
   callee until ack*.
4. **Firewall de errores.** En Rust, `catch_unwind` en cada `extern "C"`; en
   C++, `try/catch(...)` en cada borde. Un fallo se convierte en código de error
   y se loguea; jamás en *unwind* a través de la ABI.
5. **Lotes.** `begin_frame` + `collect` + `drain_ready` + `ack_upload`: cuatro
   llamadas por frame, no una por cluster.
6. **Zero-copy.** Las páginas de geometría se descomprimen en C++ y se copian
   una sola vez a un staging buffer de wgpu; las muestras neuronales se leen
   directamente del ring mapeado por USB.
7. **Contratos de hilo.** Documentados en el header. C++ nunca toca objetos de
   wgpu; Rust nunca toca el `fd` de geometría.
8. **Versionado.** `tlaxtrelom_abi_version()` en cada biblioteca. Cambio incompatible
   = major; el runtime rechaza cargar una isla con major distinta.

### 3.3 Secuencia del camino caliente (geometría)

Diseño de destino con streaming por lotes. Hoy la isla ya expone
`begin_frame`/`collect`/`drain_ready`/`ack_upload` sobre `GeoView` v2 con
frustum culling, pero el trazo GPU sigue siendo directo desde el runtime Rust:
no hay *indirect draw* ni cola de páginas asíncrona desde disco.

```text
Render thread (Rust)              C++ tlaxtrelom-geometry              IO threads (C++)
      │                                  │                             │
      │ begin_frame(view, frame_id)      │                             │
      │─────────────────────────────────>│                             │
      │ collect(requests[], cap)         │  recorre cluster DAG        │
      │<─────────────────────────────────│  calcula error proyectado   │
      │                                  │  CAS Unloaded→Loading       │
      │                                  │  encola page requests ──────>│
      │                                  │                             │ pread()
      │                                  │                             │ zstd
      │                                  │<──── ready queue ───────────│
      │ drain_ready(pages[], cap)        │                             │
      │<─────────────────────────────────│                             │
      │ wgpu: write staging → copy GPU   │                             │
      │ ack_upload(page_id)              │  Ready→Resident             │
      │─────────────────────────────────>│                             │
      │ draw indirect (GPU-driven)       │                             │
```

### 3.4 Ownership en la práctica

| Dato | Owner | Vida | Cruce |
|---|---|---|---|
| `PageBlob` comprimido | C++ IO thread | hasta `ack_upload` | puntero `const void*` |
| Página descomprimida | C++ `in_flight` map | hasta `ack_upload` | puntero estable (heap) |
| Staging buffer GPU | Rust `wgpu` | 1–2 frames | nunca cruza a C++ |
| `NeuralBatch` | Rust hilo FullDive | 1 iteración | nunca cruza a C++ |
| `IntentFrame` | Rust triple buffer | último valor | lectura wait-free |
| HIR serializado | Python/TS (herramientas) | 1 compilación | JSON bin en disco o pipe; no cruza FFI |

---

## 4. Modelo de hilos

Diseño (destino en lo de prioridades RT y pools dedicados). Hoy: un hilo main
de render con sim a paso fijo en el mismo bucle, más un hilo propio para la
fuente FullDive. Del modelo abajo ya están implementados el gate con watchdog
de 50 ms y la neutralización (`gate.rs`/`fulldive.rs`).

| Hilo | Prioridad | Stack | Responsabilidad | Reglas |
|---|---|---|---|---|
| **Main / Render** | Normal | 8 MB | Event loop, frame graph, submit | Nunca bloquea en IO ni en BCI |
| **Sim workers** | Normal | 2–4 MB | Física, animación, jobs | Work-stealing; sin locks largos |
| **FullDive** | `SCHED_FIFO` 80 | 1 MB, `mlockall` | RX, DSP, decoder, gate | Sin allocs, sin syscalls, sin logs |
| **Geometry IO** | nice -5 | 1 MB | `pread`, cook TLXG (zstd: destino) | Solo C++; no toca GPU |
| **Audio** | RT | 512 KB | Mezcla, síntesis | Buffer size fijo, sin locks |
| **Asset import** | nice 10 | 8 MB | DCC, cook | Corre como tools Python/TS, no embebido |

### 4.1 FullDive sin bloquear el render

El hilo de FullDive escribe el último `IntentFrame` en un **triple buffer**
(`triple_buffer` crate): escritura *wait-free* para el productor, lectura
*latest-wins* para el consumidor. Los eventos discretos (clic, menú, "soltar")
van por un **SPSC ring** (`rtrb`) que el render drena una vez por frame. Si el
hilo BCI se atrasa, el render usa el último valor y aplica un *watchdog*: tras
50 ms sin datos, el intent se neutraliza.

¿Por qué no `Mutex`? Porque un `Mutex` compartido entre un hilo RT y el render
introduce *priority inversion* y *jitter*. ¿Por qué no `ArcSwap`? Porque asigna
y libera en el `store`; el triple buffer no asigna y tiene cotas duras.

---

## 5. Frame pipeline

Estado implementado (`crates/tlaxtrelom-core/src/renderer.rs`): dos pases por
frame —`tlaxtrelom.scene` (escena lineal al target HDR, con un draw recortado
por ojo en estéreo) y `tlaxtrelom.present` (tonemap al swapchain).

```text
                 ┌─────────────┐
  input ────────>│  fixed sim  │  120 Hz, dt = 8.333 ms, tope de subpasos
  (BCI, gamepad, │  (world.rs) │  determinista, acumulador
   teclado, XR)  └──────┬──────┘
                        │ estado autoritativo
                 ┌──────▼──────┐
                 │ interpolate │  alpha = accumulator / dt
                 └──────┬──────┘
                 ┌──────▼──────────────────────────────┐
                 │ pass 1: escena N-D/3D → HDR lineal  │ shaders/scene3d.wgsl
                 │ target Rgba16Float + MSAA           │ (solo entrega color lineal)
                 └──────┬──────────────────────────────┘
                 ┌──────▼──────────────────────────────┐
                 │ pass 2: present                     │ shaders/present.wgsl
                 │ tonemap ACES (RRT+ODT) · recorte ·  │
                 │ dither · viñeteado de confort       │
                 └──────┬──────────────────────────────┘
                 ┌──────▼──────┐
                 │   present   │  FifoQueue (Mailbox donde existe)
                 └─────────────┘
```

- **Paso fijo** para física, red y replay; el render interpola con `alpha`. El
  mundo vive en `world.rs`: no hay ECS con archetype todavía.
- **Caída de formato honesta.** Si el adaptador no acepta `Rgba16Float` como
  attachment, se prueba `Rgba32Float` y, en último término, 8 bits. El MSAA
  vive en el target HDR; pedir `Depth32Float` a 2× exige el feature
  `TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES`.
- **Modos dimensionales.** `--dimension {2d, 2.5d, 3d, 3.5d, 4d, 4.5d, 5d}`
  (`dimension.rs`) cambia la proyección de la escena, no el present. `3.5d` no es
  un eje más: interpola la cámara ortográfica del `2.5d` con la perspectiva del
  `3d` en `xr.rs`, usando el mismo `blend` que `4.5d` interpreta como 3D↔4D.
- **Destino.** Culling en GPU, *indirect draw*, *visibility buffer*, GBuffer,
  *frame graph* con *transient aliasing* y `GpuTimer` por pass: nada de eso
  existe aún; el pipeline de "cull → stream → GBuffer → lighting → post → UI"
  es el diseño al que se apunta.
- **Presupuesto**: 16.6 ms (60 Hz) / 8.3 ms (120 Hz) / 11.1 ms (90 Hz VR).

---

## 6. Datos y assets

### 6.1 Pipeline

Estado implementado: la herramienta de la isla C++ cuece mallas al formato
binario **TLXG v2** y el HIR canónico es **JSON ordenado con hash
blake2b512-128** (Python y TypeScript). Los paquetes `.tlaxtrelompack`,
FlatBuffers, KTX2/Basis y Opus **no existen**: son destino.

```text
DCC (Blender/Maya/Houdini)
   │  export glTF/USD/FBX
   ▼
Python / TS (tlaxtrelom_tools)  ── valida, cocina TLXG v2, emite HIR JSON
   │
Rust runtime  ── carga .tlxg por FFI POD; HIR JSON por archivo o pipe
   │
   ├─► Geometría: TLXG v2 (hoy) → páginas comprimidas + DAG (destino zstd/mmap)
   ├─► Texturas: (destino: KTX2/Basis, BC7/ASTC)
   ├─► Shaders: WGSL mantenido a mano (hoy) → HIR → SPIR-V offline (destino)
   └─► Audio: (destino, sin pipeline aún)
```

### 6.2 Formatos

**Real:**

- `.tlxg` — TLXG v2 (`GEO_ABI_VERSION = 2`): cabecera con origen de sector en
  `double`, clusters con atributos en `float`, leído por la isla C++ y validado
  contra la ABI del runtime.
- **HIR canónico** — JSON con claves ordenadas + hash blake2b512-128; es el
  único artefacto que cruzan Python/TS. Los backends SPIR-V/LLVM/C++ codegen
  sobre él son contrato (ver `docs/03-tlaxtrelom-ir.md`).

**Destino (sin schema ni código):**

- `.tlaxtrelompack`: header FlatBuffers versionado (`schema_version`,
  `engine_version`, `hash`), blobs alineados a 4 KB para `mmap`/`O_DIRECT`,
  tabla de páginas con CRC32C y firma Ed25519 opcional para DLC y mods.

### 6.3 Virtual geometry (resumen)

Implementado en `native/tlaxtrelom-geometry/`: malla → clusters → DAG con
error de LOD, recorrido con **frustum culling real** derivado de `GeoView` v2
y puente por lotes con el runtime Rust (`begin_frame`/`collect`/`drain_ready`/
`ack_upload`). Destino: *software raster* en compute para triángulos sub-píxel
escribiendo un `visibility buffer` (32-bit: `depth | cluster_id |
triangle_id`) y raster de hardware para clusters grandes.

---

## 7. Build y toolchain

### 7.1 Orquestación

No hay CMake obligatorio, ni `corrosion`, ni `maturin`, ni `sccache`, ni
`rust-toolchain.toml`/`CMakePresets.json` en el repo. La matriz real:

```text
cargo xtask ci                 (crates/xtask/src/main.rs: orquestador único)
  ├── cargo fmt --check · cargo clippy -D warnings
  ├── make -C native/tlaxtrelom-geometry all + test   (isla C++, Makefile)
  ├── make -C native/tlaxtrelom-spatial test          (C; Zig sin build)
  ├── make -C native/tlaxtrelom-harmonyos test        (puente ArkUI)
  ├── glslc + spirv-val sobre shaders GLSL → SPIR-V
  ├── cargo test --workspace (+ --features native-geometry)
  ├── pruebas C#, relay Go, Python, Lua, Node/TypeScript
  ├── tlaxtrelom --selftest --json                   (headless determinista)
  └── validación de enlaces de docs/
```

- `native/tlaxtrelom-hal` (C11, ABI 1) no usa toolchain de C++: la compila
  `crates/tlaxtrelom-core/build.rs` con `cc` y se enlaza por defecto vía la
  feature `native-hal`.
- Cada isla mantiene su `Makefile`; `CMakeLists.txt` es alternativa opcional.
- CI: **un solo runner Linux**. La matriz Linux/Windows/macOS con GPU es
  destino.
- Suite hoy: 123 tests Rust con `--all-features` (3 de ellos GPU offscreen
  bajo `--ignored`), 40 Python, 14 Lua, 24 TypeScript, más los smoke C++ de
  geometry, spatial y harmonyos.
- `cargo deny` para licencias y CVEs; `clippy -D warnings`.

### 7.2 Publicación y distribución

- Motor MIT de publicación abierta: el repo **es** la distribución. Los
  binarios ya no son "de distribución interna".
- Runtime: un binario Rust + la isla C++ (`libtlaxtrelom_geometry`) + assets
  `.tlxg`.
- Shipping: sin Python embebido, `panic=abort`, LTO.
- Guardias vivos a propósito: `publish = false` en el workspace Rust y
  `"private": true` en el paquete Node evitan publicación accidental en
  crates.io/npm; publicar ahí es decisión explícita del mantenedor.
- **Destino recomendado**: binarios del CI firmados (cosign/minisign) con SBOM
  CycloneDX adjunto, y reproducibilidad (commit + toolchain + `Cargo.lock` ⇒
  mismo hash). Hoy no existe pipeline de release firmado.

---

## 8. Observabilidad

Hoy (`crates/tlaxtrelom-core/src/metrics.rs` + CLI):

- **Contadores y percentiles**: frame time y latencias con p50/p95/p99 por
  nearest-rank; contadores de *drops*, de páginas de geometría y *digests*
  deterministas del mundo para el selftest.
- **`tlaxtrelom --probe`**: enumera adaptadores GPU sin abrir ventana
  (formatos HDR soportados, límites de features).
- **`--selftest --json`**: verificación headless de un solo golpe, parseable
  en CI.
- Replay determinista: semilla + input log + versión de HIR.

Destino (sin implementación): **Tracy** para CPU/GPU/locks, **puffin** para
el editor, timestamps de GPU por pass (`TIMESTAMP_QUERY`), histograma de
latencia BCI por etapa (ADC→ring→DSP→decode→IPC→apply) y telemetría opt-in
anonimizada, siempre sin datos neuronales crudos.

---

## 9. Seguridad, privacidad y propiedad intelectual

- **Memoria**: Rust safe por defecto; el `unsafe` queda acotado al puente POD
  de `geometry.rs` y a la envoltura HAL. En C++, ASan/UBSan están disponibles
  como opción de CMake (`TLAXTRELOM_SANITIZE`); el CI de hoy los corre vía
  Makefile sin sanitizers.
- **Plugins**: sandbox de capability; un plugin declara `render`, `io`, `neural`
  y el runtime le concede solo eso. Los plugins de terceros son código ajeno:
  revisión de licencia y de seguridad antes de enlazarlos.
- **BCI**: procesamiento on-device por defecto; datos crudos nunca salen sin
  consentimiento explícito; perfil de calibración cifrado (XChaCha20-Poly1305).
  Señales neuronales = datos personales sensibles: DPIA, retención mínima y
  prohibición de uso evaluativo (`docs/04-licencias.md § 6`).
- **Supply chain**: dependencias pinneadas, `cargo deny`, `cargo vet`, SBOM,
  firma de artefactos y CI sin secretos de producción.
- **IP**: cabecera SPDX **MIT** en todo archivo, `LICENSE` MIT en la raíz,
  `NOTICE` con atribuciones, allowlist de licencias (`deny.toml`) y
  prohibición de GPL/AGPL/SSPL como *dependencies*. Detalle en
  `docs/04-licencias.md`.
- **Secretos**: `gitleaks` en pre-commit y CI; claves de firma en HSM/KMS con
  doble custodia y rotación trimestral.

---

## 10. ADRs (Architecture Decision Records)

| ID | Decisión | Alternativa descartada | Consecuencia |
|---|---|---|---|
| 001 | Rust-first; C++ como isla | C++-first con Rust para tools | Curva de aprendizaje; mejor seguridad |
| 002 | wgpu como backend principal | `ash` puro / D3D12 puro | Menos control fino; se compensa con escape hatch |
| 003 | HIR propio y subconjunto de kernel | Transpilar C++/Python completos | Mucho menos scope; 95% del valor |
| 004 | ABI C plana + `unsafe`/`bytemuck` POD | `cxx` / `bindgen` masivo / COM | ABI estable, menos magia, sin generadores en el repo |
| 005 | Python solo en editor/tools | Python embebido en runtime | Más rápido y predecible; hoy ni enlaza: solo HIR JSON |
| 006 | BCI como input device | BCI como "API mágica" | Seguridad y testabilidad |
| 007 | Sim a paso fijo 120 Hz | Variable dt | Determinismo, red, replay |
| 008 | Cluster DAG + visibility buffer | Nanite propietario / LOD clásico | Complejidad alta; escalabilidad real. Implementado: DAG + culling; visibility buffer: destino |
| 009 | Surrogados neuronales con fallback | Solo ML / solo solver | Calidad + determinismo |
| 010 | "Cuántico" = tensor networks en GPU | QPU real | Físicamente honesto y útil |
| 011 | HIR canónico = JSON ordenado + hash blake2b512-128 | FlatBuffers / Protobuf | Multi-lenguaje real sin toolchain extra; cero-copy: destino |
| 012 | Un solo repo, `xtask` | Múltiples repos | Coherencia de versiones |
| 013 | Propietario e interno (código cerrado) | Open source con CLA | **Superseded por ADR-016** |
| 014 | Allowlist de licencias + SBOM + firma | Revisión manual de licencias | Coste de CI; evita contaminación GPL y builds no trazables |
| 015 | BCI tratado como dato sensible desde el diseño | Tratarlo como telemetría de producto | DPIA, consentimiento y acceso restringido; menos agilidad en pilots |
| 016 | Relicenciar a MIT y abrir el repo | Seguir propietario/interno con NDA a socios | Se pierde exclusividad de código; la comercialización pasa a soporte, hardware y certificación, no a la licencia. `publish=false`/`private=true` quedan como guardias |
| 017 | Coordenadas de dos capas: anclaje `f64` + offset `f32` con rebase | `f32` absoluto / `double` en todo el pipeline | Escala sin temblor ni colapso estéreo; coste: dos capas en cada API (GeoView v2, `--sector`) |
| 018 | Escena → HDR `Rgba16Float` → present con tonemap ACES | LDR directo con tonemap en el shader de escena | Color correcto y caídas honestas por adaptador; un pase extra, MSAA en el target HDR |

---

## 11. Hoja de ruta

**Hitos completados:**

| Versión | Entregable |
|---|---|
| v0.1 | Documentos de arquitectura, IR, FullDive, licencias |
| v0.2 | Runtime real: ventana wgpu, estéreo/XR, selftest headless `--json`, pipeline HDR + tonemap ACES, coordenadas de dos capas + rebase + `--sector`, TLXG v2 con frustum culling, CI multi-lenguaje en un runner Linux, relicencia MIT |

**Plan (sigue siendo destino):**

| Fase | Duración | Entregable | Criterio de salida |
|---|---|---|---|
| 1 | 6 meses | Render graph, ECS, assets, geometría virtual v0 (streaming por páginas + software raster) | Escena de 1 B triángulos a 60 Hz |
| 2 | 6 meses | Fluidos C++, HIR + nodos con backends SPIR-V/LLVM, editor mínimo | Un nivel jugable con scripting visual |
| 3 | 6 meses | FullDive v0 (mock + hardware), firmware MCU, calibración | < 1 ms de procesamiento, p99 |
| 4 | 6 meses | Vertical slice AAA, Lumen-like GI, audio, XR | Demo pública de 10 min |
| 5 | continuo | SDK público, consolas, móvil, soporte comercial y certificación de hardware | Certificación |

Con MIT, la fase de "licencias a socios bajo NDA" ya no aplica: la adopción se
fomenta con el repo abierto y el negocio va por soporte, hardware y
certificación. Ver ADR-016.

---

## 12. Riesgos principales

| Riesgo | Prob. | Impacto | Mitigación |
|---|---|---|---|
| Scope de "competir con UE5" | Alta | Fatal | Vertical slice primero; no prometer paridad |
| Complejidad del cluster DAG | Alta | Alto | Hecho el tramo DAG + culling; queda streaming por páginas y software raster |
| Calidad de señal BCI consumer | Alta | Alto | Híbrido con EMG/eye tracking; no depender solo de EEG |
| Latencia inalámbrica | Media | Alto | USB HS para < 1 ms; WiFi solo para telemetría |
| Deuda de FFI | Media | Alto | ABI versionada + tests de contrato + fuzzing |
| Burnout del equipo | Media | Alto | Hitos pequeños, demo cada 6 meses |
| Licencias de SDKs (haptics, códecs) | Media | Medio | Aislar en plugins; alternativas open source |
