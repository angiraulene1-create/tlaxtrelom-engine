# Tlaxtrelom Engine — VR y FullDive

> Versión 0.2 · Alcance: pipeline XR, composición estéreo, confort, entrada
> FullDive, gate de seguridad y modo degradado.

---

## 0. Qué está implementado y qué es contrato

| Capacidad | Estado | Dónde |
|---|---|---|
| Ventana + `wgpu` (Vulkan/DX12/Metal) | Implementado | `crates/tlaxtrelom-core/src/renderer.rs` |
| Preview estéreo side-by-side | Implementado | `--stereo`, `shaders/stereo.wgsl` |
| Sesión XR con presupuesto de frame y escala adaptativa | Implementado | `xr.rs` |
| Filtro 1€ y reducción de jitter 6DOF | Implementado | `xr.rs` (`OneEuroFilter`, `PoseFilter`) |
| Predicción de pose a display time MTP | Implementado | `xr.rs` (`PosePredictor`) |
| Viñeteado dinámico anti-motion sickness | Implementado | `xr.rs` (`ComfortVignette`) |
| Shaders multiview GLSL/SPIR-V con foveación | Implementado | `shaders/glsl/` |
| Cinemática espacial 6DOF en cuaterniones duales | Implementado | `native/tlaxtrelom-spatial/` (C/Zig) |
| FullDive: decoder de referencia, replay, gate, watchdog | Implementado | `fulldive.rs`, `gate.rs` |
| Monitor biométrico (BPM, SpO2, temperatura) | Implementado | `fulldive.rs` (`BiotelemetryWatchdog`) |
| Limitador somatosensorial y anti-resonancia | Implementado | `fulldive.rs` (`SomatosensoryLimiter`) |
| Desconexión gradual y E-Stop sin shock | Implementado | `fulldive.rs` (`GradualDetachmentController`) |
| Relay de telemetría de red y pánico distribuido | Implementado | `services/tlaxtrelom-relay/` (Go) |
| SDK de alto nivel y monitores de seguridad | Implementado | `dotnet/Tlaxtrelom.Engine/` (C#) |
| Transporte BCI de hardware | Contrato | `/dev/tlaxtrelom-neuro0`, replay `.tlxneuro` |
| Actuadores críticos | **Fuera de alcance** | requieren parada física independiente |

La regla es simple: **lo que no está implementado no se documenta como si lo
estuviera**. `OpenXr` existe como enum y como contrato; el host actual usa
`PreviewXr` determinista. Un runtime OpenXR real debe cumplir el mismo contrato
de poses, tiempos y presupuesto.

---

## 1. Pipeline de frame XR

```text
input (XR/FullDive/teclado)
        │
        ▼
IntentGate  ──► simulación a paso fijo ──► pose predicha
        │                                      │
        │                                      ▼
        │                             matriz view/proj por ojo
        │                                      │
        ▼                                      ▼
eventos discretos                    render estéreo (wgpu)
                                               │
                                               ▼
                                      present / compositor XR
```

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

---

## 2. Confort y estabilidad visual

- **Filtro 1€ (One-Euro Filter)**: reduce jitter estacionario submilimétrico sin
  añadir retardo de fase ante aceleraciones rápidas (`xr.rs:OneEuroFilter`).
- **Predictor de Pose MTP**: compensa la latencia *motion-to-photon* extrapolando
  posición y orientación hacia el display time con límites de velocidad (15 m/s)
  y aceleración angular (20 rad/s), acotando la ventana a 50 ms.
- **Viñeteado dinámico de confort**: atenúa el campo visual periférico de forma
  asimétrica ante rotaciones bruscas o altas aceleraciones lineales, combatiendo
  el conflicto vestibular causante del mareo por movimiento (VR sickness).
- **Escala de render adaptativa**: el control mira frames tardíos consecutivos,
  no un solo *spike*. La histéresis evita oscilar entre calidades.
- **Foveación**: `Off`, `Fixed`, `EyeTracked`. `EyeTracked` requiere seguimiento
  ocular validado; sin él se degrada a `Fixed`. Shaders GLSL de foveación calculan
  la densidad radial por compute shader.
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

### 3.1 Modos de operación

| Modo | Configuración | Comportamiento |
|---|---|---|
| Seguro / deshabilitado | `enabled = false`, `mock = false` | No se abre transporte ni hilo; intención neutral |
| Mock de desarrollo | `mock = true` | Señal sintética determinista para el editor |
| Replay | `--replay archivo.tlxneuro` | Reproduce un log; no toca hardware |
| Hardware | `mock = false`, `enabled = true` | Abre el dispositivo BCI configurado |

El modo por defecto del host es **mock** para desarrollo. `--no-fulldive` fuerza
el modo seguro. `--secure` exige confianza discreta alta (0,75) y desactiva el
mock.

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
5. `--no-fulldive` está activo si no hay hardware BCI conectado.
6. El espacio físico tiene límites configurados y una persona adicional
   presente en pruebas de locomoción.
