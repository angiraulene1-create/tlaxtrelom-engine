<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 2026 angiraulene1-create -->

# Tlaxtrelom Engine — VR y FullDive

> Versión 0.2 · Alcance: pipeline XR, composición estéreo, confort, entrada
> FullDive, gate de seguridad y modo degradado.

---

## 0. Qué está implementado y qué es contrato

| Capacidad | Estado | Dónde |
|---|---|---|
| Ventana + `wgpu` (Vulkan/DX12/Metal) | Implementado | `crates/tlaxtrelom-core/src/renderer.rs` |
| Scene → target HDR lineal → present con tonemap ACES, recorte, dither | Implementado | `renderer.rs`, `shaders/scene3d.wgsl`, `shaders/present.wgsl` |
| Preview estéreo side-by-side (un pase de escena, un *draw* recortado por ojo) | Implementado | `--stereo`, `renderer.rs` (`draw_scene` + `set_scissor_rect`) |
| Multiview real (un solo draw para ambos ojos) | **Contrato** | no hay multiview ni view-mapping en el path wgpu |
| Coordenadas de dos capas (`WorldOrigin` f64 + `LocalPose` f32) con rebase | Implementado | `coords.rs` (`REBASE_THRESHOLD = 1024 m`) |
| Alcance de sector (`--sector`: fija `near`, `far` y niebla) | Implementado | `config.rs`, `xr.rs` (`DepthRange`) |
| Modos dimensionales `2d…5d` con blend de ejes extra | Implementado | `--dimension`, `dimension.rs` |
| Sesión XR con presupuesto de frame y escala adaptativa | Implementado | `xr.rs` |
| Filtro 1€ y reducción de jitter 6DOF | Implementado | `xr.rs` (`OneEuroFilter`, `PoseFilter`) |
| Predicción de pose a display time MTP | Implementado | `xr.rs` (`PosePredictor`) |
| Viñeteado dinámico de confort | **Parcial** | la apertura la calcula `ComfortVignette` (`xr.rs`); el uniform `comfortVignette` sólo existe en el shader de contrato GLSL, que el runtime no carga |
| Foveación de render (fixed / eye-tracked) | **Parcial/Contrato** | enum `Foveation` y struct `GazeSample` en `xr.rs`; **no** hay sampler de fijación ni mapeo de vista en el path wgpu. Los shaders GLSL que la implementan no se cargan |
| Shaders GLSL multiview/foveación compilados y validados | Implementado *(como artefacto)* | `shaders/glsl/` + `glslc`/`spirv-val` en CI; **ningún `.spv` se carga en runtime** |
| Cinemática espacial 6DOF en cuaterniones duales | Implementado | `native/tlaxtrelom-spatial/` (el binario final usa la versión C) |
| FullDive: decoder de referencia, replay, gate, watchdog | Implementado | `fulldive.rs`, `gate.rs` |
| Monitor biométrico (BPM, SpO2, temperatura) | Implementado | `fulldive.rs` (`BiotelemetryWatchdog`) |
| Limitador somatosensorial y anti-resonancia | Implementado | `fulldive.rs` (`SomatosensoryLimiter`) |
| Desconexión gradual y E-Stop sin shock | Implementado | `fulldive.rs` (`GradualDetachmentController`) |
| Relay de telemetría de red y pánico distribuido (UDP) | Implementado | `services/tlaxtrelom-relay/` (Go) |
| SDK de alto nivel y monitores de seguridad | Implementado | `dotnet/Tlaxtrelom.Engine/` (C#) |
| Transporte BCI de hardware | Contrato | `/dev/tlaxtrelom-neuro0`, replay `.tlxneuro` |
| Actuadores críticos | **Fuera de alcance** | requieren parada física independiente |

La regla es simple: **lo que no está implementado no se documenta como si lo
estuviera**. `OpenXr` existe como enum y como contrato; el host actual usa
`PreviewXr` determinista. Un runtime OpenXR real debe cumplir el mismo contrato
de poses, tiempos y presupuesto.

---

## 1. Pipeline de frame XR

El frame real son **dos pases** —escena y presentación— contra un target HDR
offscreen, no un sombreado directo al swapchain:

```text
input (XR/FullDive/teclado)
        │
        ▼
IntentGate ──► simulación a paso fijo ──► pose predicha ──► matriz view/proj por ojo
        │                                                           │
        ▼                                                           ▼
eventos discretos                                    PASE 1 · escena → target HDR
                                          (aquí se aplica la viñeta de confort)
                                                                       │
                                                          Rgba16Float (caída:
                                                          Rgba32Float → 8 bits)
                                                                       │
                                                                       ▼
                                                    PASE 2 · present: tonemap ACES
                                                    (RRT+ODT), recorte y dither
                                                                       │
                                                                       ▼
                                                        swapchain sRGB / compositor
```

Detalles verificados en `renderer.rs`:

- La escena se sombrea en **lineal** sobre un target `Rgba16Float`; si el
  adaptador no lo acepta se degrada a `Rgba32Float` y, en último término, a 8
  bits (el tonemap pierde alcance, no corrección).
- El pase de escena (`shaders/scene3d.wgsl`) aplica la **viñeta de confort** con
  la apertura que fija `XrSession::begin_frame` desde `ComfortVignette`
  (`StereoFrame::comfort_aperture` → uniform `scene.comfort.x`); en reposo la
  apertura es 1.0 y la viñeta no toca ningún píxel. El pase de presentación
  (`present.wgsl`) sigue sin aplicar viñeta: solo tonemap, recorte y dither. El
  contrato GLSL (`shaders/glsl/stereo_multiview.frag`) recibe el mismo signal en
  su `comfortVignette.x` y CI lo compila, pero el runtime no lo carga.
- **MSAA se negocia contra el target HDR**, no contra el swapchain. Pedir 2×
  sobre `Depth32Float` exige la feature `TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES`;
  sin ella el device solo garantiza 1× y 4×.
- En estéreo se **dibuja dos veces dentro del mismo render pass**: cada ojo
  ocupa su mitad del target, recortada con `set_scissor_rect`, y tiene su
  propio buffer de uniforms `Globals` (izquierda/derecha). No hay multiview, ni
  mapeo de vista, ni *attach*/*detach* por ojo.

Invariantes:

1. **El frame se predice al *display time***, no al tiempo de CPU.
2. **La pose nunca se inventa a medias.** Si el runtime no entrega una pose
   nueva, se reutiliza la última válida y se marca el frame.
3. **El presupuesto de frame se mide.** `XrStats` expone frames tardíos; si se
   encadenan, el host baja `render_scale` antes de perder el frame rate.
4. **La simulación no corre más de `max_sim_steps` por frame.** Un stall de
   100 ms no produce una espiral de la muerte.
5. **Las matrices se validan.** Un `NaN` en view/proj se rechaza; no se propaga
   a la GPU.

### 1.1 Modos

| Modo | Flag | Uso |
|---|---|---|
| Mono | *(por defecto)* | escritorio, depuración |
| Estéreo preview | `--stereo` | validar IPD, FOV y shader sin headset |
| OpenXR | `XrMode::OpenXr` | backend real, pendiente de integración |
| Headless | `--headless` / `--selftest` | CI, servidores, determinismo |

`--stereo` fija 90 Hz y `target_fps` coherente con `refresh_hz`. Un `--fps`
posterior actualiza ambos.

### 1.2 Parámetros validados

- `refresh_hz ∈ [60, 240]`.
- `render_scale ∈ [0.5, 2.0]`.
- `ipd_m ∈ [0.050, 0.080]` m (50–80 mm humanos).
- `fov_y_deg ∈ [50, 130]`.
- `input_latency_budget_us ∈ (0, 5000]`.

Un valor fuera de rango aborta el arranque con un mensaje accionable. No hay
"modo tolerante" que convierta un IPD corrupto en ojos separados 30 cm.

### 1.3 Otros knobs del frame loop

| Flag | Rango | Qué hace |
|---|---|---|
| `--dimension {2d,2.5d,3d,3.5d,4d,4.5d,5d}` | modos discretos | modo dimensional con blend de ejes extra (`dimension.rs`); `--dimension-blend` y `--dimension-focal` ajustan la proyección N-D. En `3.5d` el blend mezcla la cámara ortográfica con la perspectiva (`xr.rs`, `perspective_blend`) |
| `--msaa {1,2,4}` | 1, 2 o 4 | muestras del target HDR; 2× en `Depth32Float` depende del adaptador (feature `TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES`) |
| `--sector <m>` | 1…1e8 m (def. 10000) | alcance del streamer: fija `far`, deriva `near` (el depth buffer reparte su resolución en `[near, far]`) y escala la niebla vía `DepthRange` |

El layout estéreo es **side-by-side en un solo pase**: cada ojo se dibuja
sobre su mitad de la misma view del target HDR, con recorte y uniforms
propios. `View::identity()` ya no existe: el fallback sin GPU es
`View::neutral()`, una perspectiva de 90° (una identidad podaría hasta el último
cluster cocinado, porque el frustum se deriva de `view_proj`).

### 1.4 Coordenadas de dos capas: por qué dos capas

El techo de un mundo "de años luz" no es el motor sino la aritmética del `f32`
(24 bits de mantisa). El ULP del `f32` deja de ser útil mucho antes de lo que
sugiere un `far` grande:

| Distancia del origen | ULP aproximado del `f32` | Consecuencia |
|---|---|---|
| 2^14 m (~16 km) | > 1 mm | el detalle submilimétrico se pierde |
| 2^20 m (~1000 km) | ~6 cm | sumar medio IPD al ojo ya no cambia el número |
| 1e11 m | ~8 km | el LOD del streamer no distingue clusters vecinos |

Por eso `coords.rs` separa dos capas: **`WorldOrigin`** (anclaje global en `f64`,
metros) y **`LocalPose`** (offset `f32` respecto del anclaje, siempre de cientos
de metros). El offset se **pliega al anclaje al superar 1024 m**
(`REBASE_THRESHOLD`), de forma determinista. El IPD se suma al offset local,
nunca al anclaje. El límite honesto del `f64`: la resolución milimétrica llega
a ~9e9 m del anclaje — sobra para un sistema planetario, no para una galaxia en
un solo origen; para eso el contenido se parte en **sectores**, cada uno con su
`WorldOrigin`, y `--sector` acota el `[near, far]` dibujable dentro de cada uno.

---

## 2. Confort y estabilidad visual

- **Filtro 1€ (One-Euro Filter)**: reduce jitter estacionario submilimétrico sin
  añadir retardo de fase ante aceleraciones rápidas (`xr.rs:OneEuroFilter`).
- **Predictor de Pose MTP**: compensa la latencia *motion-to-photon* extrapolando
  posición y orientación hacia el display time con límites de velocidad (15 m/s)
  y aceleración angular (20 rad/s), acotando la ventana a 50 ms.
- **Viñeteado dinámico de confort**: atenúa el campo visual periférico ante
  rotaciones bruscas, combatiendo el conflicto vestibular causante del mareo por
  movimiento (VR sickness). `XrSession::begin_frame` mide la velocidad angular
  sobre el delta de yaw desenvuelto, `ComfortVignette` la convierte en apertura
  [0.4, 1.0] y `scene3d.wgsl` la aplica en el pase de escena vía
  `StereoFrame::comfort_aperture` → `scene.comfort.x`; en estéreo la viñeta se
  centra por ojo (usa NDC de clip, no posición de framebuffer). No hay
  desenfoque periférico ni ningún otro efecto de confort que no esté en esa
  lista: cada efecto es un uniform, no una promesa.
- **Escala de render adaptativa**: el control mira frames tardíos consecutivos,
  no un solo *spike*. La histéresis evita oscilar entre calidades.
- **Foveación (Parcial/Contrato)**: el tipo `Foveation { Off, Fixed,
  EyeTracked }` y el `GazeSample` existen en `xr.rs` como contrato, pero **el
  path wgpu no implementa foveación**: no hay sampler de fijación ni mapeo de
  vista variable. Los shaders GLSL de foveación multiview
  (`shaders/glsl/fulldive_foveated.comp`) se compilan y validan con
  `glslc`+`spirv-val` en CI, y **ningún `.spv` se carga en tiempo de
  ejecución**. La degradación con eye-tracking (`EyeTracked` → `Fixed`) es
  comportamiento del contrato, no del binario publicado.
- **Comfort**: `Seated` o `RoomScale`. El modo no cambia la física; cambia los
  límites de locomoción y las advertencias del host.
- **Sin reproyección casera**: si el compositor XR ofrece reproyección, el
  motor entrega poses y tiempos correctos y deja que el compositor haga su
  trabajo. Inventar un warp en el motor duplica errores.

---

## 3. FullDive: de la señal a la intención y retroalimentación segura

```text
NeuralSource ──► NeuralBatch ──► Decoder ──► IntentFrame ──► triple buffer
  (USB/replay)     (preasignado)  (DSP+ML)    (POD, 1 kHz)     (wait-free)

Retorno: SomatosensoryPacket ──► SomatosensoryLimiter ──► BioWatchdog
  (retroalimentación sensorial)   (clamp 50µA / anti-resonancia) (E-Stop suave)
```

- `IntentFrame` es `#[repr(C)]`, 56 bytes, una línea de caché.
- El hilo RT no asigna memoria, no hace syscalls y no comparte locks con el
  render: triple buffer + ring SPSC.
- El decoder de referencia es pequeño y determinista. Un decoder CSP/Riemann/TCN
  real se enchufa detrás del mismo contrato.
- **Seguridad somatosensorial**: corriente acotada a un máximo de 50 µA, pulsos
  máximos de 50 ms y gradiente térmico de 2 °C. Si la estimulación entra en fase
  de resonancia con el ritmo motor del usuario (±1.5 Hz), se atenúa al 20% para
  evitar bucles de retroalimentación cortical descontrolados.
- **Watchdog biométrico (`BiotelemetryWatchdog`)**: monitorea frecuencia cardíaca
  (45–175 BPM), SpO2 (≥92%), temperatura corporal y estrés. Si se sobrepasan los
  umbrales por más de 500 ms, activa el desacoplamiento inmediato.
- **Desconexión gradual (`GradualDetachmentController`)**: parada de emergencia (E-Stop)
  con rampa suave de fade-out sensorial durante 300 ms, impidiendo el choque cognitivo
  de una desconexión abrupta.

### 3.1 Modos de operación y estado en segundo plano

| Modo | Configuración | Comportamiento |
|---|---|---|
| Seguro / normal *(defecto)* | `enabled = false` | FullDive apagado. No se abre transporte ni hilo; el motor opera como motor gráfico estándar |
| Mock experimental | `--fulldive` (sin hardware) | Activa FullDive en segundo plano con señal sintética determinista |
| Replay determinista | `--replay archivo.tlxneuro` | Activa FullDive en segundo plano y reproduce un log grabado; no toca hardware |
| Hardware BCI real | `--fulldive --secure` | Activa FullDive en segundo plano y conecta con `/dev/tlaxtrelom-neuro0` |

El motor arranca **por defecto como un motor gráfico convencional de alto rendimiento**, con FullDive completamente **desactivado en segundo plano** (`enabled = false`). De esta manera, el host no reserva hilos de tiempo real ni realiza polling innecesario.

FullDive funciona exclusivamente como un **complemento experimental secundario opt-in**:
- Se activa intencionalmente con `--fulldive` o `--replay`.
- Con `--no-fulldive` se fuerza el apagado preventivo incluso si otros parámetros intentaran activarlo.
- `--secure` exige alta confianza (≥ 0.75) y desactiva el mock para exigir interfaz real o replay validado.

### 3.2 Gate de seguridad

Orden exacto de aplicación en `IntentGate`:

1. **Watchdog**: sin frame nuevo en `watchdog` (50 ms por defecto) ⇒ neutral.
2. **Confianza**: por debajo del umbral continuo ⇒ neutral.
3. **Deadzone + histéresis**: evita temblor alrededor del centro.
4. **Límites de velocidad y aceleración**: clamp duro, no cosmético.
5. **Dwell** para acciones discretas: 150 ms sostenidos por defecto.
6. **Predicción**: solo en el host, marcada y desactivable.

Todas las operaciones son O(1) y sin asignaciones. El gate vive en el frame
loop, no en un hilo de herramientas.

### 3.3 Qué NO se permite

- Accionar un actuador crítico (brazo, silla, puerta, láser) sin una parada
  física independiente del software.
- Interpretar un frame `ARTIFACT` o `CALIBRATING` como intención válida.
- Prometer latencia end-to-end < 1 ms: la latencia neurofisiológica es de
  decenas a cientos de milisegundos. El motor garantiza **L_proc < 1 ms**;
  ver [FullDive](02-fulldive.md) para la definición honesta.

---

## 4. Telemetría mínima

`--selftest --json` reporta:

- `xr_frames`, `xr_late`, `xr_render_scale`;
- `valid_steps`, `gated_steps`, `action_frames`;
- `sim_hash` y `determinism_ok`;
- `geometry_pages`;
- HAL (cores, memoria, SIMD, RT).

En ejecución normal, el host emite telemetría agregada cada
`--telemetry-every` frames. Un frame individual nunca dispara una decisión de
calidad; se usan ventanas.

---

## 5. Checklist antes de probar en un headset

1. `tlaxtrelom --probe` enumera un adaptador válido.
2. `tlaxtrelom --stereo --fps 90` mantiene el frame budget sin errores.
3. `tlaxtrelom --selftest --json` reporta `determinism_ok: true`.
4. IPD y FOV corresponden al usuario; no se usan valores de ejemplo.
5. FullDive sigue apagado si no hay hardware BCI conectado: viene apagado por
   defecto y sólo `--fulldive` (o `--replay`) lo levanta.
6. El espacio físico tiene límites configurados y una persona adicional
   presente en pruebas de locomoción.
