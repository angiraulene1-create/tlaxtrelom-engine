# Tlaxtrelom Engine — Arquitectura del sistema

> Versión 0.1 · Documento vivo · Autor: Arquitectura Principal
> Alcance: runtime, fronteras FFI, modelo de hilos, build y ADRs.

---

## 0. Resumen ejecutivo

Tlaxtrelom es un motor **Rust-first** con dos islas nativas:

- **C++** para geometría virtual, fluidos, adaptadores de hardware y plugins de
  terceros. Es código que ya existe, que depende de SDKs propietarios o que
  necesita control fino de memoria/SIMD.
- **Python** para el pipeline de contenido, el editor y la autoría. Nunca para
  el frame loop de un juego publicado.

El motor es **propietario y de uso interno**: no se publica, no se licencia como
open source y no se distribuye código fuera de la organización. La política de
licencias, secretos, SBOM, firma de artefactos y datos BCI vive en
`docs/04-propiedad-intelectual.md`; este documento asume esas reglas.

La frontera es una **ABI C plana** (`extern "C"`), versionada, con handles
opacos y contratos de ownership explícitos. Para ergonomía se usa `cxx` en
Rust↔C++ y PyO3 en Rust↔Python, pero el *hot path* siempre expone funciones C
puras. El objetivo no es "que los tres lenguajes se llamen mucho", sino **que se
llamen poco y bien**.

### Las cinco decisiones que definen el motor

1. **Rust posee el frame loop y la memoria del runtime.** No hay GC en el hilo
   de render; no hay `shared_ptr` atravesando la FFI.
2. **C++ es una isla, no un co-runtime.** Se invoca por lotes (comando de frame,
   páginas de geometría) y devuelve datos en buffers compartidos.
3. **Python es una herramienta, no un ciudadano del runtime.** El editor lo
   embebe; el juego exportado no lo enlaza.
4. **Un solo IR para tres frontends.** Texto, nodos y bloques bajan a `Tlaxtrelom HIR`
   (tipado, SSA, con efectos) y de ahí a SPIR-V/LLVM/Rust/C++.
5. **FullDive es un dispositivo de entrada más.** Se abstrae como `NeuralSource`
   → `IntentFrame`, con *gate*, *watchdog* y presupuesto de latencia medible.

---

## 1. Objetivos y no-objetivos

### Objetivos

- Renderizado agnóstico Vulkan/DX12/Metal con un solo backend mantenido (wgpu)
  y una vía de escape a `ash`/D3D12 para *work graphs*, *mesh shaders* y
  features de próxima generación.
- Geometría virtual con *cluster DAG*, streaming asíncrono y *visibility buffer*.
- Simulación determinista a paso fijo (120 Hz) con fluidos y destrucción en C++.
- Un pipeline de autoría que permita a un diseñador prototipar con bloques, a un
  TA construir nodos y a un ingeniero escribir kernels, **produciendo el mismo
  artefacto intermedio**.
- Un stack BCI con latencia de procesamiento sub-milisegundo y seguridad
  verificable.
- Build reproducible en Linux/Windows/macOS, con toolchains pinneadas.

### No-objetivos (explícitos)

- **No** compilamos C++ arbitrario ni Python arbitrario al mismo AST. Eso exige
  reimplementar un compilador de C++ y la semántica dinámica de Python. Lo que
  hacemos es definir un **subconjunto de kernel** tipado y determinista que los
  tres lenguajes pueden expresar mediante anotaciones; el código *host* sigue
  siendo nativo. Ver `docs/03-tlaxtrelom-ir.md § 1`.
- **No** prometemos latencia end-to-end < 1 ms para intención cortical. La
  ventana fisiológica (50–300 ms) es irreducible. El < 1 ms es el presupuesto
  *última muestra → input aplicado*. Ver `docs/02-fulldive.md § 8`.
- **No** hay "física cuántica" en el sentido de un QPU en tiempo real. Hay
  solvers de tensor networks/campos cuánticos simulados en GPU, opcionales y
  fuera del camino determinista. Ver ADR-010.
- **No** habrá paridad con UE5 en el primer año. Habrá un *vertical slice*
  técnicamente creíble y una arquitectura que no haya que reescribir.

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
│  Backends: SPIR-V (GPU) · LLVM/Cranelift (CPU) · Rust codegen · C++ stubs    │
├──────────────────────────────────────────────────────────────────────────────┤
│                          RUNTIME RUST  (host / shipping)                     │
│  ECS · scheduler · frame graph · assets · input · audio · net · replay       │
│  ┌──────────────┐ ┌──────────────┐ ┌──────────────┐ ┌─────────────────────┐ │
│  │ tlaxtrelom-render  │ │ tlaxtrelom-fulldive│ │ tlaxtrelom-physics │ │ tlaxtrelom-script (VM)    │ │
│  │ wgpu + FG    │ │ BCI host     │ │ Rust solver  │ │ HIR interpreter     │ │
│  └──────────────┘ └──────────────┘ └──────────────┘ └─────────────────────┘ │
├──────────────────────────────────────────────────────────────────────────────┤
│                        ISLAS NATIVAS  (C++ / firmware)                       │
│  tlaxtrelom-geometry (cluster DAG + IO)  ·  tlaxtrelom-fluids (FLIP/MPM)                 │
│  tlaxtrelom-haptics (SDK vendors)        ·  firmware BCI (Rust no_std, Embassy)    │
├──────────────────────────────────────────────────────────────────────────────┤
│                            HAL / PLATAFORMA                                  │
│  Vulkan · DX12 · Metal · OpenXR · USB HS / Ethernet · audio · filesystem     │
└──────────────────────────────────────────────────────────────────────────────┘
```

### 2.1 Mapa de crates y módulos

```text
crates/
  tlaxtrelom-core        # bootstrap, frame loop, ECS, scheduler, recursos
  tlaxtrelom-render      # device wgpu, frame graph, passes, virtual geometry GPU
  tlaxtrelom-ir          # HIR/MIR, parser, passes, codegen
  tlaxtrelom-graph       # frontend de nodos (JSON/FlatBuffers → HIR)
  tlaxtrelom-blocks      # frontend de bloques (árbol → HIR)
  tlaxtrelom-fulldive    # transporte, DSP, decoder, gate, calibración
  tlaxtrelom-neuro-proto # protocolo binario compartido host↔firmware (no_std)
  tlaxtrelom-bindings    # módulo PyO3 (solo herramientas/editor)
  tlaxtrelom-cli         # xtask: build, cook, replay, bench
native/
  tlaxtrelom-geometry    # C++: streaming de geometría de alta densidad
  tlaxtrelom-fluids      # C++: FLIP/MPM, destrucción, tela
  tlaxtrelom-haptics     # C++: adaptadores OpenXR/vendors
python/
  tlaxtrelom_tools/      # pipeline, graph, blocks, bindings
firmware/
  fulldive-mcu/    # Rust no_std: ADC, DMA, framing, CRC, watchdog
```

---

## 3. La frontera FFI

### 3.1 Matriz de comunicación

| Desde → Hacia | Mecanismo | Frecuencia | Datos | Coste |
|---|---|---|---|---|
| Rust → C++ | `extern "C"` (+ `cxx` ergonómico) | 1/frame + 1/página | POD, punteros, buffers | ~ns por llamada |
| C++ → Rust | *callbacks* C con `void* user` | 1/frame | stats, páginas listas | ~ns |
| Rust → Python | PyO3 | Solo editor/CLI | objetos, numpy, bytes | µs–ms + GIL |
| Python → Rust | PyO3, `allow_threads` | Comandos de tooling | HIR serializado | µs–ms |
| C++ → Python | **Prohibido en runtime**; pybind11 solo en tools | — | — | — |
| Firmware → Host | USB HS / Ethernet, protocolo binario | 1 kHz | muestras EEG/EMG | ~125–250 µs |

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
| HIR serializado | Python/editor | 1 compilación | `&[u8]` con GIL liberado |

---

## 4. Modelo de hilos

| Hilo | Prioridad | Stack | Responsabilidad | Reglas |
|---|---|---|---|---|
| **Main / Render** | Normal | 8 MB | Event loop, frame graph, submit | Nunca bloquea en IO ni en BCI |
| **Sim workers** | Normal | 2–4 MB | Física, animación, jobs | Work-stealing; sin locks largos |
| **FullDive** | `SCHED_FIFO` 80 | 1 MB, `mlockall` | RX, DSP, decoder, gate | Sin allocs, sin syscalls, sin logs |
| **Geometry IO** | nice -5 | 1 MB | `pread`, zstd | Solo C++; no toca GPU |
| **Audio** | RT | 512 KB | Mezcla, síntesis | Buffer size fijo, sin locks |
| **Asset import** | nice 10 | 8 MB | DCC, cook | Puede usar Python |
| **Editor Python** | Normal | 8 MB | Scripts, UI tools | GIL; jamás en shipping |

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

```text
                 ┌─────────────┐
  input ────────>│  fixed sim  │  120 Hz, dt = 8.333 ms
  (BCI, gamepad, │  (Rust/C++) │  determinista, acumulador
   teclado, XR)  └──────┬──────┘
                        │ estado autoritativo
                 ┌──────▼──────┐
                 │ interpolate │  alpha = accumulator / dt
                 └──────┬──────┘
                 ┌──────▼──────┐
                 │ frame graph │  passes: cull → stream → GBuffer →
                 │  (wgpu)     │  lighting → post → UI
                 └──────┬──────┘
                 ┌──────▼──────┐
                 │   present   │  Mailbox/Fifo, latency 1–2
                 └─────────────┘
```

- **Paso fijo** para física, red y replay. El render interpola.
- **Frame graph** con barreras automáticas y *transient aliasing*.
- **GPU-driven**: culling y LOD en compute; el CPU solo emite *indirect draws*.
- **Presupuesto**: 16.6 ms (60 Hz) / 8.3 ms (120 Hz) / 11.1 ms (90 Hz VR).
  El frame graph expone un `GpuTimer` por pass y aborta el pass si excede.

---

## 6. Datos y assets

### 6.1 Pipeline

```text
DCC (Blender/Maya/Houdini)
   │  export glTF/USD/FBX
   ▼
Python (tlaxtrelom_tools)  ── importa, valida, cocina, genera HIR de shaders
   │  escribe .tlaxtrelompack (FlatBuffers + blobs)
   ▼
Rust asset server  ── resuelve dependencias, versiona, sirve por mmap
   │
   ├─► Geometría: páginas comprimidas (zstd) + DAG de clusters
   ├─► Texturas: KTX2/Basis, mips, BC7/ASTC
   ├─► Shaders: HIR → SPIR-V (offline) + cache en disco
   └─► Audio: Opus/Vorbis + metadata
```

### 6.2 Formato `.tlaxtrelompack`

- Header FlatBuffers versionado (`schema_version`, `engine_version`, `hash`).
- Blobs alineados a 4 KB para `mmap` y `O_DIRECT`.
- Tabla de páginas con offset/tamaño comprimido/tamaño crudo/CRC32C.
- Firma opcional (Ed25519) para DLC y mods.

### 6.3 Virtual geometry (resumen)

- Malla → clusters de 128 triángulos → DAG jerárquico con error de LOD.
- Cada nodo del DAG referencia una **página** (64–256 KB) con los clusters
  hijos y sus atributos comprimidos.
- Runtime: recorrido *front-to-back*, error proyectado en píxeles, petición de
  páginas no residentes, LRU con presupuesto de VRAM.
- Raster: *hardware raster* para clusters grandes + *software raster* en compute
  para triángulos sub-píxel, escribiendo un `visibility buffer` (32-bit:
  `depth | cluster_id | triangle_id`).
- Detalle en `native/tlaxtrelom-geometry/`.

---

## 7. Build y toolchain

### 7.1 Orquestación

```text
cargo xtask build --release
  ├── cargo build --workspace            (Rust, LTO thin, panic=abort en shipping)
  ├── cmake --preset release             (C++ vía corrosion)
  │     └── tlaxtrelom-geometry, tlaxtrelom-fluids, tlaxtrelom-haptics
  ├── maturin build --release            (PyO3, solo editor/tools)
  └── python -m tlaxtrelom_tools.cook          (assets, shaders, HIR cache)
```

- `rust-toolchain.toml` pinneado; `CMakePresets.json` pinneado.
- `sccache` + `mold`/`lld`; builds incrementales < 30 s en un Ryzen 9.
- CI matrix: Linux (Vulkan), Windows (DX12), macOS (Metal); runners con GPU.
- `cargo deny` para licencias y CVEs; `clippy -D warnings`; `sanitizers` en C++.

### 7.2 Empaquetado y distribución interna

- Runtime: un binario Rust + `.so`/`.dll` de las islas C++ + `.tlaxtrelompack`.
- Editor: binario + Python embebido (o `uv` gestionado) + tools.
- Shipping: **sin Python**, sin símbolos de debug, `panic=abort`, LTO, PGO.
- **Todo binario sale del CI**, firmado (`cosign`/minisign) y con SBOM
  CycloneDX adjunto; nunca de una estación de trabajo.
- **Registries privados**: `publish = false` en el workspace, espejo interno de
  crates.io, CI sin salida directa a Internet.
- **Reproducibilidad**: mismo commit + toolchain + `Cargo.lock` ⇒ mismo hash.
  Sin esto la firma no prueba nada.
- La distribución a socios es binaria, bajo NDA y con marca de agua por
  licenciatario. Ver `docs/04-propiedad-intelectual.md § 4`.

---

## 8. Observabilidad

- **Tracy** para CPU/GPU/locks; **puffin** para el editor.
- Timestamps de GPU por pass (`TIMESTAMP_QUERY`), histograma de frame time.
- FullDive: histograma de latencia por etapa (ADC→ring→DSP→decode→IPC→apply),
  contadores de *drops*, *sequence gaps*, *artifacts*.
- Telemetría **opt-in**, anonimizada, sin datos neuronales crudos.
- Replay determinista: semilla + input log + versión de HIR.

---

## 9. Seguridad, privacidad y propiedad intelectual

- **Memoria**: Rust safe por defecto; `unsafe` auditado y encapsulado en
  `tlaxtrelom-ffi`. C++ con `-fsanitize=address,undefined` en CI y `gsl::span`.
- **Plugins**: sandbox de capability; un plugin declara `render`, `io`, `neural`
  y el runtime le concede solo eso. Los plugins de terceros son código ajeno:
  revisión de licencia y de seguridad antes de enlazarlos.
- **BCI**: procesamiento on-device por defecto; datos crudos nunca salen sin
  consentimiento explícito; perfil de calibración cifrado (XChaCha20-Poly1305).
  Señales neuronales = datos personales sensibles: DPIA, retención mínima y
  prohibición de uso evaluativo (`docs/04-propiedad-intelectual.md § 6`).
- **Supply chain**: dependencias pinneadas, `cargo deny`, `cargo vet`, SBOM,
  firma de artefactos y CI sin secretos de producción.
- **IP**: cabecera SPDX propietaria en todo archivo, `NOTICE` con atribuciones,
  allowlist de licencias (`deny.toml`) y prohibición de GPL/AGPL/SSPL.
- **Secretos**: `gitleaks` en pre-commit y CI; claves de firma en HSM/KMS con
  doble custodia y rotación trimestral.

---

## 10. ADRs (Architecture Decision Records)

| ID | Decisión | Alternativa descartada | Consecuencia |
|---|---|---|---|
| 001 | Rust-first; C++ como isla | C++-first con Rust para tools | Curva de aprendizaje; mejor seguridad |
| 002 | wgpu como backend principal | `ash` puro / D3D12 puro | Menos control fino; se compensa con escape hatch |
| 003 | HIR propio y subconjunto de kernel | Transpilar C++/Python completos | Mucho menos scope; 95% del valor |
| 004 | ABI C + `cxx` ergonómico | `bindgen` masivo / COM | ABI estable, menos magia |
| 005 | Python solo en editor/tools | Python embebido en runtime | Más rápido y predecible |
| 006 | BCI como input device | BCI como "API mágica" | Seguridad y testabilidad |
| 007 | Sim a paso fijo 120 Hz | Variable dt | Determinismo, red, replay |
| 008 | Cluster DAG + visibility buffer | Nanite propietario / LOD clásico | Complejidad alta; escalabilidad real |
| 009 | Surrogados neuronales con fallback | Solo ML / solo solver | Calidad + determinismo |
| 010 | "Cuántico" = tensor networks en GPU | QPU real | Físicamente honesto y útil |
| 011 | FlatBuffers para HIR/assets | JSON / Protobuf | Zero-copy, versionado |
| 012 | Un solo repo, `xtask` | Múltiples repos | Coherencia de versiones |
| 013 | Propietario e interno (código cerrado) | Open source con CLA | Sin comunidad ni API pública estable; control total de IP y roadmap |
| 014 | Allowlist de licencias + SBOM + firma | Revisión manual de licencias | Coste de CI; evita contaminación GPL y builds no trazables |
| 015 | BCI tratado como dato sensible desde el diseño | Tratarlo como telemetría de producto | DPIA, consentimiento y acceso restringido; menos agilidad en pilots |

---

## 11. Hoja de ruta

| Fase | Duración | Entregable | Criterio de salida |
|---|---|---|---|
| 0 | 3 meses | Esqueleto, CI, ABI, frame loop, triángulo | 120 Hz estables, 0 warnings |
| 1 | 6 meses | Render graph, ECS, assets, geometría virtual v0 | Escena de 1 B triángulos a 60 Hz |
| 2 | 6 meses | Fluidos C++, HIR + nodos, editor mínimo | Un nivel jugable con scripting visual |
| 3 | 6 meses | FullDive v0 (mock + hardware), calibración | < 1 ms de procesamiento, p99 |
| 4 | 6 meses | Vertical slice AAA, Lumen-like GI, audio, XR | Demo interna de 10 min |
| 5 | continuo | SDK interno, consolas, móvil, licencias a socios bajo NDA | Certificación |

---

## 12. Riesgos principales

| Riesgo | Prob. | Impacto | Mitigación |
|---|---|---|---|
| Scope de "competir con UE5" | Alta | Fatal | Vertical slice primero; no prometer paridad |
| Complejidad del cluster DAG | Alta | Alto | Empezar con LOD clásico + streaming; DAG después |
| Calidad de señal BCI consumer | Alta | Alto | Híbrido con EMG/eye tracking; no depender solo de EEG |
| Latencia inalámbrica | Media | Alto | USB HS para < 1 ms; WiFi solo para telemetría |
| Deuda de FFI | Media | Alto | ABI versionada + tests de contrato + fuzzing |
| Burnout del equipo | Media | Alto | Hitos pequeños, demo cada 6 meses |
| Licencias de SDKs (haptics, códecs) | Media | Medio | Aislar en plugins; alternativas open source |
