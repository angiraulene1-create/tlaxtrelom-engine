# Tlaxtrelom FullDive — Especificación del subsistema BCI

> Versión 0.1 · Alcance: adquisición EEG/EMG/EOG, firmware, protocolo,
> decodificación, fusión, latencia, seguridad y API.

---

## 0. Definición honesta de "latencia < 1 ms"

Hay tres latencias distintas y es un error de arquitectura confundirlas:

| Latencia | Definición | Objetivo | ¿Reducible? |
|---|---|---|---|
| **L_proc** | Última muestra adquirida → `IntentFrame` aplicado en el motor | **< 1 ms** | Sí, con diseño RT |
| **L_fisio** | Inicio de la intención cortical → ventana de datos suficiente para decodificarla | 50–300 ms | No (física/neurofisiología) |
| **L_total** | Intención → fotón en el display | 60–320 ms | Parcialmente, con predicción |

El motor garantiza **L_proc < 1 ms**. La latencia percibida se enmascara con
**decodificación incremental** (actualizar la estimación cada 8–25 ms), **predicción**
(extrapolar el intent 50–100 ms) y **fusión híbrida** (EMG/eye tracking para
acciones discretas rápidas). Prometer < 1 ms end-to-end sería deshonesto.

---

## 1. Hardware de referencia

| Bloque | Componente | Especificación | Por qué |
|---|---|---|---|
| AFE EEG | TI ADS1299 (×8 para 64 canales) | 24-bit, 250 SPS–16 kSPS, 0.1 µV RMS | Bajo ruido, bias drive, lead-off |
| AFE EMG | TI ADS131M08 | 24-bit, 8 kSPS | Alta frecuencia, entrada diferencial |
| MCU | STM32H743 (Cortex-M7, 480 MHz) | 1 MB RAM, USB HS, 4× DMA | Determinismo, USB HS |
| Enlace | USB 2.0 HS (microframes 125 µs) | 480 Mbps | Único enlace cableado con < 1 ms |
| Alternativa | 100BASE-T1 + PTP | 100 Mbps, sincronía hardware | Aislamiento y distancia |
| Aislamiento | Digital isolator + DC/DC | 5 kV | Seguridad del usuario |
| HMD | OpenXR 1.1 | 90–120 Hz, eye tracking | Fusión y *foveated rendering* |
| Hápticos | OpenXR Haptics / vendor SDK | 1 kHz | Cierre sensorial |

**Regla**: si el enlace es WiFi, el presupuesto de 1 ms es imposible (2–10 ms de
jitter). WiFi solo para telemetría o para un modo "relajado" no crítico.

### 1.1 Frecuencias y ancho de banda

- 64 canales EEG @ 1 kSPS, 24-bit empaquetado (3 B) = **192 KB/s**.
- 8 canales EMG @ 2 kSPS = **48 KB/s**.
- Total < 250 KB/s: trivial para USB HS; imposible de justificar con WiFi.
- Ventana de decodificación: 250 ms (EEG motor imagery) con *hop* de 25 ms.
  El *hop* define la tasa de actualización del intent (40 Hz), no la latencia.

---

## 2. Firmware (Rust `no_std`)

```text
┌──────────┐   SPI    ┌──────────┐   DMA    ┌──────────────┐
│ ADS1299  │─────────>│ STM32H7  │─────────>│ ring buffer  │
│ (64 ch)  │  DRDY    │ Embassy  │  doble   │ 2 × 32 KB    │
└──────────┘          └────┬─────┘  buffer  └──────┬───────┘
                           │                        │
                    DWT cycle counter        framing + CRC32C
                           │                        │
                           └──────────> USB HS CDC (bulk) ──> host
```

- **`no_std` + Embassy**: tareas async para ADC, USB, watchdog e impedancia.
- **DMA doble buffer**: IRQ de *half-transfer* y *transfer-complete*; nunca se
  copia en la ISR, solo se publica el índice.
- **Timestamp**: `DWT->CYCCNT` a 480 MHz (resolución 2.08 ns) + sincronía con el
  host por intercambio de 4 timestamps (estilo PTP simplificado).
- **Framing**: paquete cada 1 ms (1 muestra/canal) o cada 2 ms (2 muestras).
- **Watchdog**: si el host no hace *ping* en 500 ms, el firmware entra en modo
  seguro y deja de emitir (evita actuación con datos viejos).
- **Lead-off / impedancia**: medición cada 10 s; si un electrodo supera 50 kΩ,
  se marca `DEGRADED` y el host baja la confianza.
- **Seguridad eléctrica**: aislamiento galvánico, limitación de corriente,
  cumplimiento IEC 60601-1 si se comercializa como dispositivo médico.

### 2.1 Por qué Rust en el MCU

- `no_std`, sin asignador, sin GC; `embassy` da concurrencia sin RTOS.
- Los tipos `#[repr(C)]` del protocolo se comparten con el host
  (`tlaxtrelom-neuro-proto`), eliminando drift de layout.
- `panic = abort` + `defmt` para trazabilidad en desarrollo.

---

## 3. Protocolo binario (`tlaxtrelom-neuro-proto`)

### 3.1 Cabecera (48 bytes, alineada)

```c
typedef struct __attribute__((packed)) TlaxtrelomNeuroHeader {
    uint16_t magic;          // 0xAE01
    uint8_t  version;        // major.minor empaquetado
    uint8_t  flags;          // START, STOP, IMPEDANCE, MARKER
    uint32_t seq;            // secuencia monotónica; gaps = pérdida
    uint64_t t_mcu_ns;       // timestamp del primer sample del payload
    uint64_t t_host_echo_ns; // último t_host recibido (sincronía)
    uint16_t channels;       // 64
    uint16_t sample_rate;    // 1000
    uint8_t  samples_per_ch; // 1
    uint8_t  adc_bits;       // 24
    uint16_t payload_bytes;  // channels * samples * 3 (+ markers)
    uint32_t crc32c;         // sobre header (con crc=0) + payload
    uint8_t  reserved[8];    // futuro: montaje, batería, temperatura
} TlaxtrelomNeuroHeader;           // 2+1+1+4+8+8+2+2+1+1+2+4+8 = 44 → padding a 48
```

- **Endianness**: little-endian en el cable; conversión explícita.
- **Payload**: `i24` empaquetado, intercalado por muestra. El host lo expande a
  `f32` una sola vez, en el ring, sin allocations.
- **CRC32C** por hardware en el MCU; verificación en el host antes de publicar.
- **Pérdida**: `seq` discontinuo → el host interpola o marca `GAP` y baja la
  confianza; nunca "congela" el último valor silenciosamente.
- **Sincronía**: cada segundo, el host envía `t_host_echo_ns`; el firmware
  responde con `t_mcu_ns`. El host ajusta offset y deriva con regresión lineal
  sobre 64 muestras (error < 10 ppm).

### 3.2 Máquina de estados del enlace

```text
DISCONNECTED ──open──> HANDSHAKE ──version ok──> STREAMING
      ▲                    │                        │
      │                    └──error──┐              │ watchdog / stop
      └──────────── close ───────────┴──────────────┘
```

---

## 4. Stack host (Rust)

```text
USB HS / Ethernet
      │  nusb / io_uring, ring mapeado
      ▼
┌─────────────────┐   ┌─────────────────┐   ┌─────────────────┐
│ Transport       │──>│ Ring SPSC       │──>│ DSP pipeline    │
│ (parse + CRC)   │   │ (sin allocs)    │   │ notch, bandpass │
└─────────────────┘   └─────────────────┘   │ CAR, ASR        │
                                            └────────┬────────┘
                                                     ▼
                                            ┌─────────────────┐
                                            │ Feature extract │
                                            │ PSD, CSP, Riemann│
                                            └────────┬────────┘
                                                     ▼
                                            ┌─────────────────┐
                                            │ Decoder + gate  │
                                            │ LDA/ridge/TCN   │
                                            └────────┬────────┘
                                                     ▼
                                            ┌─────────────────┐
                                            │ IntentFrame     │
                                            │ triple buffer   │
                                            └─────────────────┘
```

### 4.1 Etapas DSP

1. **Validación**: `seq`, CRC, `channels`, `sample_rate`, saturación.
2. **Notch adaptativo** 50/60 Hz (biquad; LMS si la línea deriva).
3. **Bandpass**: EEG 0.5–45 Hz; EMG 20–450 Hz; EOG 0.1–10 Hz.
4. **Referencia**: CAR (common average) o Laplaciano; configurable por montaje.
5. **Artefactos**: umbral de amplitud (> 150 µV), derivada, planitud; ASR/ICA
   opcional para offline. En runtime, rechazo simple y rápido.
6. **Features**:
   - Potencia de banda (δ 1–4, θ 4–8, α 8–13, β 13–30, γ 30–45 Hz) por canal.
   - **CSP** (Common Spatial Patterns) por usuario y tarea (4–6 filtros).
   - **Riemannian tangent space** sobre matrices de covarianza SPD: robusto con
     pocos datos y recalibración barata.
7. **Decoder**:
   - Discreto (4–8 clases): LDA/SVM sobre features CSP/Riemann; 20–40 ms de
     ventana, *hop* 25 ms.
   - Continuo (velocidad 2D): ridge regression + Kalman; ventana 100 ms.
   - Opcional: TCN cuantizado (int8) sobre waveform; solo si el hardware lo
     justifica y con *fallback* determinista.

### 4.2 Calibración

```text
IDLE → BASELINE (30 s, ojos abiertos/cerrados)
     → TRAINING (5–10 min, tareas guiadas: izquierda/derecha/pies/lengua)
     → VALIDATION (2 min, accuracy > 70% y kappa > 0.4)
     → LIVE (adaptación no supervisada, EMA de covarianzas)
     → DRIFT (si accuracy cae > 10%, recalibrar)
```

- Perfil por usuario: filtros CSP, medias/covarianzas, umbrales, modelo.
- Almacenamiento cifrado (XChaCha20-Poly1305), clave derivada de OS keychain.
- **Nunca** se comparte el perfil entre usuarios: la anatomía y los electrodos
  cambian demasiado.

---

## 5. Fusión de intents

El BCI no compite con el gamepad: se **fusiona**.

```rust
pub struct IntentFrame {
    pub t_ns: u64,          // timestamp del último sample usado
    pub seq: u32,           // secuencia del decoder
    pub flags: u32,         // VALID | CALIBRATING | ARTIFACT | GATED | PREDICTED
    pub confidence: f32,    // [0, 1]
    pub axes: [f32; 8],     // 0..3 continuos, 4..7 discretos (0/1)
}
```

Reglas de fusión:

| Fuente | Peso | Uso |
|---|---|---|
| EEG continuo | 0.3–0.6 | Locomoción lenta, dirección de mirada |
| EMG | 0.2–0.5 | Gestos discretos, "clic", agarre |
| Eye tracking | 0.3–0.7 | Apuntado rápido, selección |
| Gamepad/teclado | 1.0 (override) | Acciones críticas, accesibilidad |
| Voz | 0.8 | Comandos de menú |

- **Gate**: `confidence >= 0.65` para acciones discretas; `0.35` para continuas.
- **Dwell**: 150 ms de intención sostenida para confirmar un clic.
- **Histéresis**: evitar parpadeo entre clases; suavizado EMA.
- **Clamp**: velocidad máxima y aceleración máxima por seguridad.
- **Watchdog**: 50 ms sin frame → `NEUTRAL`; 500 ms → `GATED`.
- **Predicción**: `axes += velocity * horizon` (50–100 ms), con clamp y
  decaimiento; se marca `PREDICTED` para que el gameplay pueda desactivarla.

---

## 6. Presupuesto de latencia (L_proc)

Medido desde la última muestra del ADC hasta que el `IntentFrame` está visible
para el hilo de simulación. Números objetivo en un STM32H7 + USB HS + Ryzen 9.

| Etapa | Objetivo | Notas |
|---|---:|---|
| AFE + ADC + filtro sinc | 100 µs | group delay incluido |
| DMA + IRQ | 20 µs | doble buffer, sin copia en ISR |
| Framing + CRC32C (MCU) | 30 µs | hardware CRC |
| USB HS (microframe) | 125–250 µs | peor caso 2 microframes |
| RX host + CRC + parse | 60 µs | zero-copy al ring |
| Ring SPSC | 10 µs | wait-free |
| Notch + bandpass (64 ch) | 80 µs | SIMD, biquads en `f32` |
| Features (PSD/CSP/Riemann) | 100 µs | ventana incremental |
| Inferencia | 150 µs | LDA/ridge o TCN pequeño |
| Fusión + gate | 30 µs | |
| IPC (triple buffer) | 20 µs | |
| Aplicar en motor | 50 µs | predicción + clamp |
| **Total L_proc** | **~0.8–1.0 ms** | p99 objetivo < 1.5 ms |

> El *hop* de 25 ms significa que el intent se **actualiza** 40 veces/s; no
> significa que cada actualización espere 25 ms. La ventana de 250 ms es la que
> introduce L_fisio.

### 6.1 Cómo se garantiza

- Hilo dedicado, `SCHED_FIFO` 80, `mlockall(MCL_CURRENT|MCL_FUTURE)`.
- Sin `malloc`, sin `println!`, sin syscalls en el camino caliente.
- Buffers preasignados; el decoder escribe en arrays de tamaño fijo.
- `std::hint::spin_loop()` en espera activa del ring (con *backoff*).
- Aislamiento de CPU (`isolcpus`) y `IRQ affinity` en el host.
- Medición continua: histograma HDR por etapa; alerta si p99 > 1.5 ms.

---

## 7. Seguridad (no negociable)

1. **El BCI nunca acciona un actuador directamente.** Todo pasa por
   `IntentGate`, que aplica confianza, dwell, clamp y watchdog.
2. **Parada física de emergencia** (e-stop) siempre disponible y fuera del
   software.
3. **Modo seguro** si: electrodo desconectado, saturación, CRC inválido,
   watchdog vencido, batería baja, temperatura alta.
4. **Límites de velocidad/fuerza** configurables por el usuario y por el
   desarrollador; el mínimo de ambos gana.
5. **Consentimiento explícito** por sesión; el usuario puede revocar y borrar
   el perfil.
6. **Transparencia**: el HUD muestra `confidence`, `source` y `GATED` cuando el
   input no se está usando.
7. **Si se hacen claims médicos**: IEC 62304 (software), ISO 14971 (riesgos),
   IEC 60601-1 (eléctrico). Si es entretenimiento/wellness: disclaimers claros,
   pero el diseño de seguridad se mantiene.

---

## 8. Privacidad

- Procesamiento **on-device** por defecto; las señales crudas no salen del host.
- El perfil de calibración se cifra en reposo; la clave vive en el keychain.
- Telemetría opt-in, agregada, sin waveforms ni features identificables.
- Retención: crudo 0 s por defecto; features para debug, 24 h, con consentimiento.
- Cumplimiento: GDPR (UE), BIPA (Illinois) si aplica, HIPAA solo si hay un
  *covered entity*.

---

## 9. API pública (Rust)

```rust
/// Fuente de muestras. Implementaciones: USB, Ethernet, replay, mock.
pub trait NeuralSource: Send {
    fn poll(&mut self, out: &mut NeuralBatch) -> Result<usize, NeuroError>;
    fn clock(&self) -> NeuroClock;
}

pub struct NeuralBatch {
    pub channels: u16,
    pub sample_rate: u32,
    pub t0_ns: u64,
    pub samples: Vec<f32>, // intercalado, preasignado; nunca reasigna en RT
    pub len: usize,
}

pub struct IntentGate {
    min_confidence: f32,
    dwell: Duration,
    max_velocity: [f32; 2],
    watchdog: Duration,
}

impl IntentGate {
    pub fn apply(&mut self, raw: IntentFrame, now: Instant) -> IntentFrame;
}
```

El runtime consume `IntentFrame` como cualquier otro dispositivo:

```rust
let intent = fulldive.snapshot(now);       // wait-free
world.fixed_update(dt, &intent);
```

---

## 10. Modos de fallo y respuesta

| Fallo | Detección | Respuesta |
|---|---|---|
| Electrodo caído | lead-off / impedancia | `DEGRADED`, bajar confianza, HUD |
| Saturación | amplitud > full-scale | `ARTIFACT`, descartar ventana |
| Pérdida de paquetes | `seq` gap | interpolar ≤ 2 muestras; si no, `GAP` |
| Deriva de reloj | offset > 1 ms | re-sync; si persiste, `GATED` |
| Modelo no calibrado | accuracy < umbral | `CALIBRATING`, no actuar |
| Fatiga / deriva | caída de confianza sostenida | sugerir pausa/recalibración |
| Desconexión | timeout 50 ms | `NEUTRAL`; 500 ms → `GATED` |
| Sobrecarga del host | p99 > 1.5 ms | degradar features; priorizar seguridad |

---

## 11. Pruebas

- **Unitarias**: biquads, CRC, parser, gate, watchdog.
- **Sintéticas**: generador de alpha/beta con SNR controlado; inyección de
  artefactos y gaps.
- **Datasets**: BCI Competition IV 2a/2b para validar el decoder offline.
- **Replay**: log binario → mismo `IntentFrame` byte a byte (determinismo).
- **HIL**: MCU + AFE + simulador de electrodos; latencia medida con GPIO toggle.
- **Fuzzing**: parser de protocolo (`cargo-fuzz`), FFI (`libFuzzer`).
- **Carga**: 64 ch @ 2 kSPS durante 8 h; medir *drift*, *drops*, memoria.
