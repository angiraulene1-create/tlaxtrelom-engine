<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 2026 angiraulene1-create -->

# Tlaxtrelom FullDive — Especificación del subsistema BCI

> Versión 0.2 · Alcance: adquisición EEG/EMG/EOG, firmware, protocolo, red,
> decodificación, fusión, latencia, seguridad, privacidad y API.
>
> El motor es software abierto bajo [licencia MIT](04-licencias.md): no hay
> código propietario, confidencial ni bajo NDA en este subsistema, y las
> contribuciones externas entran por PR con DCO. Lo que restringe FullDive **no
> es la licencia** sino el tratamiento de datos neuronales (§ 7 y § 8) y la
> seguridad del estimulador.
>
> FullDive es un **complemento experimental opt-in**: `Config::default()` lo
> deja apagado, así que un arranque normal del motor no abre transporte, no crea
> el hilo de tiempo real ni toca hardware. Se activa con `--fulldive`, y
> `--replay <log>` lo activa implícitamente sobre un log determinista.
>
> Este documento separa en cada sección lo **implementado** (lo que compila y
> corre hoy en `crates/tlaxtrelom-core/src/fulldive.rs`, `gate.rs`, `xr.rs` y
> `main.rs`, más el relay Go) de lo **destino** (hardware, firmware, protocolos y
> modelos que aún no existen). Hay un caso aparte, marcado como *no presente en
> este árbol*: el host C++/OpenXR que la documentación de producto describe (§ 1.4)
> no está en `native/`, que solo contiene geometry, hal, harmonyos y spatial.

---

## Estado del subsistema: implementado vs. destino

| Capacidad | Estado | Dónde |
|---|---|---|
| `IntentFrame` `#[repr(C)]` de 56 bytes, una línea de caché | Implementado | `fulldive.rs` |
| Decoder de referencia: notch 50 Hz + bandpass 12 Hz + potencia EMA + modelo lineal | Implementado | `fulldive.rs` (`Decoder`) |
| Detección de artefactos (muestras no finitas, > 200 µV) y ventana de calibración | Implementado | `fulldive.rs` (`ARTIFACT_UV`, `calibration_ms`) |
| Hilo RT: afinidad de CPU, `SCHED_FIFO`, `mlockall` (mejor esfuerzo, Linux) | Implementado | `fulldive.rs` (`pin_realtime`, `Stats`) |
| Publicación wait-free: triple buffer *latest-wins* + ring SPSC de eventos | Implementado | `fulldive.rs` (`triple_buffer`, `rtrb`) |
| Fuentes de datos: *mock* sintético determinista y replay `.tlxneuro` | Implementado | `fulldive.rs` (`MockSource`, `ReplaySource`, `write_replay_log`) |
| `IntentGate`: watchdog, confianza, zona muerta con histéresis, clamp de velocidad y aceleración, *dwell*, E-Stop | Implementado | `gate.rs` |
| Predicción en el motor (75 ms) con flag `PREDICTED` | Implementado | `main.rs` (frame loop) |
| Modelo de seguridad somatosensorial, biométrica y de desacoplamiento | Implementado *(lógica; no hay actuador detrás)* | `fulldive.rs` (`SomatosensoryLimiter`, `BiotelemetryWatchdog`, `GradualDetachmentController`) |
| Relay de telemetría y pánico distribuido sobre **UDP** (magic `TLX1`, CRC32) | Implementado *(biblioteca; `main()` no arranca el bucle de escucha)* | `services/tlaxtrelom-relay/` (Go) |
| Espejo de los límites de seguridad en el SDK administrado | Implementado | `dotnet/Tlaxtrelom.Engine/FullDiveSafetyMonitor.cs` |
| Host de referencia C++17/OpenXR multiplataforma (ventana XR, audio, vibración, relay) | **No presente en este árbol** | `native/` solo contiene `tlaxtrelom-geometry`, `tlaxtrelom-hal`, `tlaxtrelom-harmonyos`, `tlaxtrelom-spatial`; ver § 1.4 |
| Transporte USB de hardware | **Destino** | `UsbSource::open` falla con error explícito; el contrato es `/dev/tlaxtrelom-neuro0` |
| Firmware `no_std`, AFE ADS1299/ADS131M08, MCU, enlace USB HS | **Destino** | no hay firmware en el repo |
| Protocolo binario host↔firmware y el crate `tlaxtrelom-neuro-proto` | **Destino** | el crate **no existe**: `crates/` solo contiene `tlaxtrelom-core` y `xtask` |
| Features CSP / Riemann / TCN y modelos por usuario | **Destino** | § 4.3 |
| Calibración multi-fase, perfiles cifrados, adaptación no supervisada | **Destino** | § 4.4 |
| *Eye tracking*, *foveated rendering*, EMG/EOG como fuentes de fusión | **Contrato** | tipos `Foveation`/`GazeSample` en `xr.rs`; nada de eso se consume en el path wgpu |
| Sesión OpenXR real | **Contrato** | `XrMode::OpenXr` es un enum sin backend; el motor compone estéreo con `PreviewXr` (§ 3.1) |

La regla es la misma que en [VR y FullDive](05-vr-fulldive.md): **lo que no está
implementado no se documenta como si lo estuviera**. Un decoder CSP/Riemann/TCN
real se enchufa detrás del mismo `IntentFrame` sin tocar el resto del motor.

---

## 0. Definición honesta de "latencia < 1 ms"

Hay tres latencias distintas y es un error de arquitectura confundirlas:

| Latencia | Definición | Objetivo | ¿Reducible? |
|---|---|---|---|
| **L_proc** | Última muestra adquirida → `IntentFrame` aplicado en el motor | **< 1 ms** | Sí, con diseño RT |
| **L_fisio** | Inicio de la intención cortical → ventana de datos suficiente para decodificarla | 50–300 ms | No (física/neurofisiología) |
| **L_total** | Intención → fotón en el display | 60–320 ms | Parcialmente, con predicción |

El motor garantiza **L_proc < 1 ms**. La latencia percibida se enmascara con
**decodificación incremental** (actualizar la estimación cada 8–25 ms),
**predicción** (extrapolar el intent; hoy 75 ms en `main.rs`) y **fusión
híbrida** (EMG/*eye tracking* para acciones discretas rápidas, destino).
Prometer < 1 ms *end-to-end* sería deshonesto: la ventana neurofisiológica no se
negocia.

---

## 1. Hardware de referencia — destino

Ninguno de estos componentes existe como código en el repositorio. Es el
presupuesto físico sobre el que se dimensionó el contrato `NeuralSource` →
`IntentFrame` y la lista de requisitos de un adaptador real.

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

`MAX_CHANNELS = 64` y el rango validado de `sample_rate` (250…16 000 Hz en
`fulldive::Config`) salen de esta tabla: el decoder trabaja sobre arrays fijos
de 64 canales para no asignar nunca en el camino caliente.

### 1.1 Frecuencias y ancho de banda

- 64 canales EEG @ 1 kSPS, 24-bit empaquetado (3 B) = **192 KB/s**.
- 8 canales EMG @ 2 kSPS = **48 KB/s**.
- Total < 250 KB/s: trivial para USB HS; imposible de justificar con WiFi.
- Ventana de decodificación: 250 ms (EEG *motor imagery*) con *hop* de 25 ms.
  El *hop* define la tasa de actualización del intent (40 Hz), no la latencia.
  En el decoder implementado ese τ = 250 ms vive en el EMA de potencia
  (`ema = 1 − exp(−1/(sr·0,25))`), no en un búfer de ventana deslizante.

### 1.2 Lo que sí corre hoy

- **Motor gráfico normal por defecto**: el motor arranca sin hilos de fondo BCI ni
  consumo de ciclos para FullDive (`enabled = false`).
- **Mock**: al pasar explícitamente `--fulldive` (sin `--secure` ni `--replay`),
  se levanta el complemento en segundo plano con un generador sintético
  determinista (alpha 10 Hz + beta 20 Hz + ruido LCG, 1 muestra/canal a 1 kHz).
- **Replay**: `--replay archivo.tlxneuro` reproduce una grabación con formato
  exacto (§ 3.1) y es la única entrada de señal medida que existe hoy.
- **Hardware**: con `--fulldive --secure` (o `mock = false`) y sin replay se
  intenta abrir `/dev/tlaxtrelom-neuro0` y **falla con error explícito**. El
  hilo termina, el motor gráfico normal continúa con intención neutral y
  `health().running` pasa a `false`. Nunca se inventan señales cuando el modo
  seguro está activo.

### 1.3 Qué consume el motor del HMD

El contrato de pose que alimenta el pipeline XR es `HeadPose { position:
[f32; 3], yaw: f32 }` (`xr.rs`): **sí** se usa la posición del casco, pero como
*offset* local respecto del `WorldOrigin` (`coords.rs`), nunca como coordenada
global `f32`. No existen `pitch` ni `roll` en el tipo: la vista se construye con
el *yaw* del jugador y la IPD aplicada al *offset*, y el estéreo se resuelve en
dos pases. Un adaptador OpenXR real que entregue rotación completa 6DOF tendrá
que extender el contrato (y versionarlo), no suponer que el motor ya la consume.

### 1.4 Correcciones transversales verificadas en código

Este documento se reescribió contra el árbol actual. Los cinco puntos que siguen
son las divergencias encontradas, cada una resuelta **a favor del código**:

1. **Licencia**: no queda ninguna advertencia de "código propietario",
   "confidencial", "bajo NDA" o "sin CLA" en este subsistema. Todo el árbol es
   [MIT](04-licencias.md) (`license = "MIT"` en `Cargo.toml`, cabeceras
   `SPDX-License-Identifier: MIT` en cada fuente) y las contribuciones externas
   entran por PR con DCO. Lo que limita FullDive no es la licencia sino el
   tratamiento de datos neuronales: consentimiento informado, DPIA,
   prohibición de uso evaluativo o laboral y las fronteras de datos de § 8.
2. **Transporte de red**: el relay implementado es **UDP** con magic
   `0x544C5831` (`"TLX1"`) en `services/tlaxtrelom-relay/protocol.go:13`, y no
   existe ninguna "TLXB" ni un listener TCP. Ver § 3.4.
3. **Versiones de ABI**: se expresan como **enteros**, no como strings de tres
   dígitos, y la cadena `1.1.0` no aparece en ningún `abi_version`:
   `tlaxtrelom_geo_abi_version() → 2` (`geometry_streamer.cpp:527`),
   `TLAXTRELOM_SPATIAL_ABI_VERSION 1u`, `TLAXTRELOM_HARMONYOS_ABI_VERSION 1u` y
   `TLAXTRELOM_HAL_ABI_VERSION 1u` (`native/tlaxtrelom-hal/include/tlaxtrelom_hal.h:21`).
4. **Crate de protocolo**: `crates/tlaxtrelom-neuro-proto` **no existe** (y
   tampoco una ruta `crates/tlaxtrelom-core/src/neural.rs`); `crates/` solo
   contiene `tlaxtrelom-core` y `xtask`, y el código neural vive en
   `fulldive.rs`, `gate.rs`, `telemetry.rs` y `xr.rs`. Nada de este documento
   debe leerse como "el crate X compila" si no está en el árbol.
5. **El host C++/OpenXR de referencia no está en este árbol**. La documentación
   de producto describe un anfitrión nativo multiplataforma (ventana XR,
   *frame loop* de 90 Hz, *reprojection* de emergencia, audio binaural,
   vibración y cliente del relay) en `native/tlaxtrelom-host`, con
   `[[bin]] tlax-host` en el manifiesto. Verificado con `ls native/` y con una
   búsqueda en todo el repo: **no hay ninguna fuente que declare ese crate ni
   ese ejecutable**; `native/` solo contiene `tlaxtrelom-geometry`,
   `tlaxtrelom-hal`, `tlaxtrelom-harmonyos` y `tlaxtrelom-spatial`, y el único
   binario Rust real es `tlaxtrelom` (`crates/tlaxtrelom-core/Cargo.toml`, usado
   por `cargo run -p tlaxtrelom-core --bin tlaxtrelom`). Por tanto todo lo
   atribuible a ese host se documenta aquí como **contrato/destino**, no como
   implementación; lo que sí existe y se comporta así es el *preview* estéreo
   del motor Rust (`XrMode::StereoPreview`, `XrStats`, `PreviewXr`, § 4 de
   [VR y FullDive](05-vr-fulldive.md)). El paquete UDP del host descrito sería
   compatible byte a byte con `protocol.go` (magic, big-endian, 28 bytes de
   cabecera, CRC32 IEEE) — pero esa compatibilidad no es verificable hoy porque
   esas fuentes no están.
6. **FullDive como complemento secundario en segundo plano**: FullDive no es el
   núcleo del motor gráfico, sino un complemento experimental desacoplado y
   desactivado por defecto. El motor opera como un motor gráfico estándar de
   última generación (a la par o superior a Unreal Engine 5 y futuro UE6), con
   geometría virtualizada por clusters DAG, HDR ACES, coordenadas de 64 bits con
   rebase de origen, y soporte para 2D, 2.5D, 3D, 3.5D, 4D, 4.5D y 5D. Solo
   cuando se pasa `--fulldive` o `--replay` se instancian sus hilos y buffers.

---

## 2. Firmware (Rust `no_std`) — destino

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
- **Watchdog del dispositivo**: si el host no hace *ping* en 500 ms, el firmware
  entra en modo seguro y deja de emitir (evita actuación con datos viejos). No
  confundirlo con los watchdogs implementados en el host (§ 5, § 7).
- **Lead-off / impedancia**: medición cada 10 s; si un electrodo supera 50 kΩ se
  marca `DEGRADED` y el host baja la confianza.
- **Seguridad eléctrica**: aislamiento galvánico, limitación de corriente,
  cumplimiento IEC 60601-1 si se comercializa como dispositivo médico.

### 2.1 Por qué Rust en el MCU

- `no_std`, sin asignador, sin GC; `embassy` da concurrencia sin RTOS.
- Los tipos `#[repr(C)]` del protocolo compartirían layout con el host. El crate
  planificado para eso (`tlaxtrelom-neuro-proto`) **todavía no existe**: hoy los
  únicos tipos compartidos que sí viven en código son `IntentFrame` y
  `NeuroEvent`, ambos `#[repr(C)]` dentro de `fulldive.rs`.
- `panic = abort` + `defmt` para trazabilidad en desarrollo.

---

## 3. Protocolo

### 3.1 Implementado: log de replay `.tlxneuro`

Formato real, escrito por `fulldive::write_replay_log()` y leído por
`ReplaySource` (18 bytes de cabecera + payload):

```text
magic        [u8; 8]  = "TLXNEUR1"
channels     u16 LE   válido 1..=64
sample_rate  u32 LE   válido 250..=16 000
frames       u32 LE
payload      frames × channels × f32 LE, intercalado por muestra
```

- La cabecera se lee con `read_exact`: un archivo truncado es un error, no un
  final silencioso. Un *magic* distinto aborta el arranque con la ruta en el
  mensaje.
- `poll()` devuelve como mucho `min(restante, capacidad del batch, 256)` frames
  y **pone a cero los canales que el log no trae**: conservar muestras viejas
  del batch reutilizado inyectaría señal fantasma en el decoder.
- Los tests cubren la ida y vuelta del log (mismos `f32` en el mismo orden) y el
  relleno de canales; el determinismo *end-to-end* del decoder sobre un replay no
  está testeado, lo que se compara byte a byte en el selftest es el digest del
  mundo (`world.rs::digest()`).
- `write_replay_log()` la usa el cooker y los tests, **nunca** el frame loop.

### 3.2 Destino: cabecera host↔firmware (48 bytes, alineada)

El protocolo binario del cable es diseño, no código: no hay parser de este
formato en el repositorio.

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
  confianza; nunca "congela" el último valor silenciosamente. Lo implementado
  hoy es el contador: `NeuralSource::sequence()` + huecos acumulados en
  `Stats::gaps`.
- **Sincronía**: cada segundo el host envía `t_host_echo_ns` y el firmware
  responde con `t_mcu_ns`; el host ajusta offset y deriva con regresión lineal
  sobre 64 muestras (error < 10 ppm).

### 3.3 Destino: máquina de estados del enlace

```text
DISCONNECTED ──open──> HANDSHAKE ──version ok──> STREAMING
      ▲                    │                        │
      │                    └──error──┐              │ watchdog / stop
      └──────────── close ───────────┴──────────────┘
```

Lo implementado es el extremo host simplificado: `FullDiveHandle::spawn()` abre
fuente, y si el transporte falla el hilo termina con error y la intención se
queda neutral. No hay reintento ni handshake.

### 3.4 Implementado: transporte de red (relay de telemetría)

El relay real es **UDP**, no TCP, y no usa ningún formato "TLXB". Código:
`services/tlaxtrelom-relay/protocol.go` y `relay.go`.

```text
magic      u32 BE = 0x544C5831 ("TLX1")
version    u16 BE = 1
type       u16 BE   0 heartbeat · 1 VrHeadPose · 2 FullDiveState · 3 BiometricPanic
sequence   u32 BE
user_id    u64 BE
timestamp  u64 BE
payload    [n]byte
crc32      u32 BE   (CRC-32 IEEE sobre cabecera + payload)
```

- Cabecera de 28 bytes y CRC de 4; `DecodePacket` exige `len ≥ 32` y rechaza
  *magic* distinto (`ErrInvalidMagic`) o CRC que no cuadra (`ErrCorruptCRC`).
- El servidor (`RelayServer`) indexa sesiones por `user_id` con
  `LastAddr`/`LastSequence`/`LastSeen` y poda las antiguas con `PruneStale`.
- Un paquete `BiometricPanic` marca la sesión y **difunde un E-Stop** a los pares
  (`EMERGENCY_ESTOP_TRIGGERED`); el fail-safe del lado del cliente es el del § 7.
- **Límite honesto**: `main()` del relay solo imprime una cabecera; el bucle de
  escucha (`ListenUDP` + `ProcessPacket`) no está cableado en el binario. El
  contrato del *wire format* lo verifica `services/tlaxtrelom-relay/test_relay.py`
  (reimplementa la codificación en Python; solo invoca `go test` si el toolchain
  Go está en el `PATH`).

---

## 4. Stack host (Rust)

Lo que existe hoy, exactamente como lo ejecuta `fulldive::run()`:

```text
MockSource / ReplaySource / (UsbSource = stub)
      │  poll() → NeuralBatch preasignado (256 muestras × 64 ch)
      ▼
┌───────────────────────────┐   ┌──────────────────────────┐
│ huecos por seq de fuente  │──>│ Decoder::push            │
│ → Stats::gaps             │   │ notch 50 Hz + band 12 Hz │
└───────────────────────────┘   │ potencia EMA · lineal    │
                                └───────────┬──────────────┘
                                            ▼
                     ┌──────────────────────────────────────┐
                     │ IntentFrame (56 B)  ·  NeuroEvent[≤8] │
                     └──────────┬────────────────┬──────────┘
                                ▼                ▼
                     triple buffer (latest-wins)  ring SPSC rtrb (256)
                                │                │
                                ▼                ▼
                      FullDiveHandle::snapshot  next_event → world
```

Todo eso corre en un hilo dedicado (`tlaxtrelom-fulldive`, pila de 1 MiB) que
no comparte locks con el render. El dibujo clásico con `nusb`/`io_uring`, CAR,
ASR y extracción de *features* pertenece al diseño (§ 4.3).

### 4.1 Contratos de tiempo real

- Cero asignaciones en `poll`/`push`/`write`: todo está preasignado.
- Cero syscalls y cero *logging* por muestra. Solo se escribe en stderr **una
  vez** por error de transporte (`error_logged`) y al terminar el hilo con
  error; en el primer caso además se duerme 1 ms antes de reintentar, así que el
  coste extra vive en la ruta de fallo, no en la de datos.
- Buffers de tamaño fijo: `MAX_CHANNELS = 64`, eventos `≤ 8` por lote.
- Espera activa con `std::hint::spin_loop()` cuando la fuente devuelve 0
  muestras; en un transporte real `poll` bloquea en lectura con timeout.
- Medición: `Stats::record()` guarda `last_us`/`max_us` del `Decoder::push` con
  `compare_exchange` (saturando a `u32`, nunca truncando).

### 4.2 Etapas DSP implementadas

1. **Validación**: canales y `len` no nulos, coherencia de `sample_rate`
   (`b.sample_rate == 0 || == el del decoder`), finitud de la muestra. Un
   `sample_rate` distinto marca `ARTIFACT` y emite `NeuroEvent { kind: 1, code: 2 }`.
2. **Notch RBJ** a 50 Hz, Q = 30 (≈1,7 Hz de banda) por canal — en América, 60 Hz.
3. **Bandpass RBJ** de falda constante a 12 Hz, Q = 1,2 (banda alfa).
4. **Detección de artefactos**: cualquier valor no finito o con magnitud
   > `ARTIFACT_UV = 200 µV` equivalentes marca el lote como `ARTIFACT`,
   anula la confianza y emite `NeuroEvent { kind: 1, code: 1 }`. Antes de
   filtrar, la muestra se *clampea* a ±200 µV para no propagar la saturación.
5. **Potencia por canal**: EMA de `y²` con τ = 250 ms.
6. **Salida**: `axes[k] = tanh(Σ w[k][c]·√power[c] + bias[k])`; en el demo,
   canales pares → eje X e impares → eje Y. Los ejes 4..7 son la velocidad por
   diferencia finita (`(axes − prev)·10`, *clamped* a ±1) y alimentan la
   predicción.
7. **Confianza**: proxy de SNR `(√potencia_media / 20)` *clamped* a [0, 1]; con
   artefacto se fuerza a 0. `VALID` exige `confidence ≥ 0.35`, sin artefacto y
   sin calibrar.

Un filtro inestable que produzca no-finitos reinicia su estado y devuelve 0:
datos corruptos no envenenan el resto de la sesión.

### 4.3 Etapas destino

El decoder de referencia es deliberadamente pequeño y determinista: el contrato
(`IntentFrame`) es lo que importa, no la sofisticación del modelo. Falta:

- **Referencia**: CAR (media común) o Laplaciano, configurable por montaje.
- **Artefactos avanzados**: umbral por derivada y planitud; ASR/ICA en *offline*.
- **Features**: potencia de banda (δ 1–4, θ 4–8, α 8–13, β 13–30, γ 30–45 Hz),
  **CSP** por usuario y tarea (4–6 filtros), espacio tangente riemanniano sobre
  matrices de covarianza SPD.
- **Decoder**: LDA/SVM sobre features CSP/Riemann para 4–8 clases discretas
  (ventana 20–40 ms, *hop* 25 ms); *ridge regression* + Kalman para velocidad
  2D continua (ventana 100 ms); opcionalmente TCN cuantizado int8 sobre waveform
  con *fallback* determinista.

### 4.4 Calibración

Hoy solo existe la **ventana de calentamiento**: `calibration_ms` (2 000 ms por
defecto, tope 10 000) acumula muestras y, mientras no se complete, el frame
lleva `CALIBRATING`, la confianza es 0 y `Decoder::calibration_fraction()`
informa el progreso. El gate neutraliza todo frame `CALIBRATING`.

La máquina completa es destino:

```text
IDLE → BASELINE (30 s, ojos abiertos/cerrados)
     → TRAINING (5–10 min, tareas guiadas: izquierda/derecha/pies/lengua)
     → VALIDATION (2 min, accuracy > 70% y kappa > 0.4)
     → LIVE (adaptación no supervisada, EMA de covarianzas)
     → DRIFT (si accuracy cae > 10%, recalibrar)
```

- Perfil por usuario (filtros CSP, medias/covarianzas, umbrales, modelo):
  **destino**; hoy no hay persistencia de perfiles en el repo.
- Cifrado en reposo con **XChaCha20-Poly1305** y clave en el llavero del SO:
  **destino** y requisito legal, no código. La decisión de política está en
  [Licencias § 6](04-licencias.md).
- **Nunca** se comparte el perfil entre usuarios: anatomía y electrodos cambian
  demasiado.

---

## 5. Fusión de intents

El BCI no compite con el teclado ni con el gamepad: se **fusiona**. El tipo que
se publica es exactamente este (`#[repr(C)]`, 56 bytes, alineación 8):

```rust
#[repr(C)]
pub struct IntentFrame {
    pub t_ns: u64,          // timestamp del último sample usado
    pub seq: u32,           // secuencia del decoder
    pub flags: u32,         // bitmask de `flags::*`
    pub confidence: f32,    // [0, 1]
    pub axes: [f32; 8],     // 0..3 continuos, 4..7 velocidad (predicción)
    pub actions: u16,       // acciones discretas; las valida el gate
    pub _pad: u16,
}

pub mod flags {
    pub const VALID: u32       = 1 << 0;
    pub const CALIBRATING: u32 = 1 << 1;
    pub const ARTIFACT: u32    = 1 << 2;
    pub const GATED: u32       = 1 << 3;
    pub const PREDICTED: u32   = 1 << 4;
    pub const ACTION: u32      = 1 << 5;
}
```

### 5.1 Lo implementado

En el frame loop (`main.rs`), por orden:

1. `fulldive.snapshot(now)` — wait-free; si no hay `seq` nuevo en
   `WATCHDOG = 50 ms` devuelve el frame *neutralizado* (ejes a 0, acciones a 0,
   `VALID` limpio).
2. Se drenan los `NeuroEvent` del ring hacia `world.on_neural_event()`.
3. **Teclado/ratón**: si hay input humano y el intent no es válido, lo reemplaza
   (`confidence = 1.0`, `VALID`); si el intent es válido, **se suma** y se
   *clampa* a [-1, 1]. Es la única fusión multi-fuente implementada.
4. **Predicción**: `axes[i] += axes[i+4] · 0.075` con *clamp*, y se marca
   `PREDICTED` para que el *gameplay* pueda desactivarla en acciones críticas.
5. `gate.apply(intent, now)` — la frontera de seguridad (§ 5.2).
6. `world.fixed_update(dt, &intent)`: `axes[0]`/`axes[1]` se integran como
   velocidad en X/Z (tope 6 m/s, aceleración 30 m/s²) y `axes[2]` gira la cámara
   (`yaw += axes[2] · 1,8 · dt`). El mundo **vuelve a sanear** los ejes antes de
   integrar: no confía en ningún origen de input, ni siquiera en el gate. La
   altura de ojos es constante (`EYE_HEIGHT`); la posición se gestiona en dos
   capas (anclaje `f64` + *offset* `f32`, `coords.rs`) como describe
   [VR y FullDive § 1.4](05-vr-fulldive.md).

### 5.2 `IntentGate`: orden exacto y valores por defecto

| Regla | Defecto | Notas |
|---|---|---|
| Confianza continua mínima | `0.35` | por debajo ⇒ neutral + `GATED` |
| Confianza discreta mínima | `0.65` (`--secure` la sube a `0.75`) | |
| *Dwell* de acción discreta | 150 ms sostenidos | un frame neutral **reinicia** el contador |
| *Watchdog* | 50 ms sin `seq` nuevo | neutral + `GATED` |
| Zona muerta / histéresis | 0.05 / 0.03 | salida continua, sin escalón |
| Velocidad máxima | 1.0 por eje | *clamp* duro |
| Aceleración máxima | 4.0/s por eje (ejes 0 y 1) | los ejes 2..3 no la llevan: el usuario espera respuesta inmediata |
| Ventana `dt` del limitador | `min_dt = 1 ms`, `max_dt = 50 ms` | un stall no puede dar un salto de aceleración infinito |

`ARTIFACT` o `CALIBRATING` siempre son neutral + `GATED`. El E-Stop
(`trigger_e_stop`) neutraliza de inmediato y queda enganchado hasta
`clear_e_stop()`. `gated_ratio()`/`take_stats()` exponen cuántos frames se
bloquearon, para HUD y telemetría.

### 5.3 Fusión ponderada — destino

Las ponderaciones son diseño del producto, no código: hoy no existen canales
EMG, EOG ni de mirada en el decoder.

| Fuente | Peso | Uso | Estado |
|---|---|---|---|
| EEG continuo | 0.3–0.6 | Locomoción lenta, dirección de mirada | implementado (única fuente neural) |
| EMG | 0.2–0.5 | Gestos discretos, "clic", agarre | destino |
| *Eye tracking* | 0.3–0.7 | Apuntado rápido, selección | contrato (`GazeSample`, no consumido) |
| Gamepad/teclado | 1.0 (anulación) | Acciones críticas, accesibilidad | implementado (suma con *clamp*) |
| Voz | 0.8 | Comandos de menú | destino |

---

## 6. Presupuesto de latencia (L_proc) — destino con medidas parciales

Medido desde la última muestra del ADC hasta que el `IntentFrame` está visible
para el hilo de simulación. Números objetivo en STM32H7 + USB HS + CPU de
escritorio; **el transporte ADC→host no existe todavía**, así que el presupuesto
completo no se puede cerrar en el repo actual.

| Etapa | Objetivo | Notas | Estado |
|---|---:|---|---|
| AFE + ADC + filtro sinc | 100 µs | *group delay* incluido | destino |
| DMA + IRQ | 20 µs | doble buffer, sin copia en ISR | destino |
| Framing + CRC32C (MCU) | 30 µs | CRC por hardware | destino |
| USB HS (microframe) | 125–250 µs | peor caso 2 microframes | destino |
| RX host + CRC + parse | 60 µs | *zero-copy* al ring | destino |
| Ring SPSC | 10 µs | wait-free | implementado (`rtrb`) |
| Notch + bandpass (64 ch) | 80 µs | biquads en `f32` | implementado (sin SIMD) |
| Features (PSD/CSP/Riemann) | 100 µs | ventana incremental | destino |
| Inferencia | 150 µs | LDA/ridge o TCN pequeño | implementado (modelo lineal) |
| Fusión + gate | 30 µs | | implementado |
| IPC (triple buffer) | 20 µs | | implementado |
| Aplicar en motor | 50 µs | predicción + *clamp* | implementado |
| **Total L_proc** | **~0.8–1.0 ms** | p99 objetivo < 1.5 ms | no verificable end-to-end sin hardware |

> El *hop* de 25 ms significa que el intent se **actualiza** 40 veces/s; no
> significa que cada actualización espere 25 ms. La ventana de 250 ms es la que
> introduce L_fisio.

### 6.1 Cómo se garantiza

- Hilo dedicado con afinidad a `core_id` (3 por defecto), `SCHED_FIFO` a 80 y
  `mlockall(MCL_CURRENT|MCL_FUTURE)`, todo de **mejor esfuerzo**: si el sistema
  no tiene `CAP_SYS_NICE`, el hilo sigue en CFS y `Stats::rt_scheduler` lo
  refleja. No es fatal ni silencioso.
- Sin `malloc`, sin `println!`, sin syscalls en el camino caliente.
- Publicación *wait-free*: si el render no consume, gana el último valor
  (*latest-wins*), que es lo correcto para entrada continua.

### 6.2 Qué se mide hoy

- `Stats` del hilo BCI: `frames`, `errors`, `gaps`, `artifacts`,
  `events_dropped`, `last_us`, `max_us` (este `last_us`/`max_us` es el tiempo de
  `Decoder::push`, no el L_proc completo) y los tres booleanos de RT.
- `FrameMetrics` (`metrics.rs`): ventana de 5 s a 120 Hz con percentiles
  p50/p95/p99 del tiempo de frame.
- Volcado agregado cada `--telemetry-every` frames (120 por defecto) a stderr:
  `bci frames=… gaps=… artifacts=… last=…us max=…us rt=… pinned=… locked=…`.
- Los knobs reales que tocan el subsistema son `--replay`, `--no-fulldive`,
  `--secure`, `--telemetry-every`, `--sim-hz` y `--fps`. **No existen**
  `--eeg-rate`, `--emg-rate`, `--motion-rate`, `--session` ni `--vital-source`:
  quien los cite está describiendo otro proyecto.
- El histograma HDR *por etapa* y la alerta automática por p99 > 1.5 ms son
  destino: hoy no hay instrumentación por debajo de `Decoder::push`.

---

## 7. Seguridad (no negociable)

1. **El BCI nunca acciona un actuador directamente.** Todo pasa por
   `IntentGate`, que aplica confianza, *dwell*, *clamp* y *watchdog*. En este
   repo no hay ni una sola línea que escriba a un estimulador: lo que existe es
   el modelo de límite.
2. **Parada física de emergencia** (e-stop) siempre disponible y fuera del
   software. En código hay dos E-Stops lógicos y ninguno sustituye al físico:
   `IntentGate::trigger_e_stop()` y `GradualDetachmentController`, que hace
   *fade* de 300 ms antes de `SafelyDisconnected` para evitar el choque
   sensorial de una desconexión abrupta.
3. **Modo seguro** si: electrodo desconectado, saturación, CRC inválido,
   *watchdog* vencido, batería baja, temperatura alta. De esa lista, hoy se
   detectan saturación/no finitud, *watchdog* y descalibración; lead-off, CRC de
   cable, batería y temperatura dependen del firmware (§ 2). `enabled = false`
   (`--no-fulldive`) no abre transporte ni crea hilo: intención neutral, que es
   el modo de menús, editores y CI.
4. **Límites por eje** en `GateConfig` (`max_speed`, `max_accel`): el *clamp* se
   aplica siempre, no es cosmético. Los configura el desarrollador; que el
   usuario proponga el suyo y gane el mínimo de ambos es **destino** (no existe
   capa de preferencias).
5. **Límites del retorno somatosensorial** (`SomatosensoryLimiter`, espejados
   en `FullDiveSafetyMonitor.cs`): corriente ≤ **50 µA**, frecuencia en
   [1, 250] Hz, pulso ≤ **50 ms**, gradiente térmico ≤ **2 °C**. Si la
   estimulación cae a ±1,5 Hz del ritmo motor del usuario se atenúa al 20 %
   (prevención de resonancia cortical). Un paquete con NaN/Inf se neutraliza
   por completo en vez de *clampearse*.
6. **Watchdog biométrico** (`BiotelemetryWatchdog`): rango seguro 45–175 BPM,
   SpO2 ≥ 92 %, temperatura 35–38,5 °C e índice de estrés ≤ 0,85. Fuera de rango
   500 ms continuados (mínimo 50 ms) dispara la emergencia, y el fail-safe
   **queda enganchado**: solo se rearma con intervención humana explícita
   (`reset()`).
7. **Consentimiento explícito** por sesión; el usuario puede revocar y borrar su
   perfil.
8. **Transparencia**: la idea es que el HUD muestre `confidence`, `source` y
   `GATED` cuando el input no se está usando. **Es destino**: en el binario no
   hay HUD de estado (el quad de `mesh.rs` es geometría de la demo). Lo que sí
   existe es el volcado de stderr cada `--telemetry-every` frames (§ 6.2) y
   `IntentGate::gated_ratio()`, que es el dato que ese HUD tendría que pintar.
9. **Si se hacen *claims* médicos**: IEC 62304 (software), ISO 14971 (riesgos),
   IEC 60601-1 (eléctrico). Si es entretenimiento/*wellness*: *disclaimers*
   claros, pero el diseño de seguridad se mantiene.

---

## 8. Privacidad y régimen legal

MIT licencia el código; **no licencia el tratamiento de datos de nadie**. La
política completa está en [Licencias § 6](04-licencias.md); esto es lo que
aplica al subsistema:

- Procesamiento **on-device** por defecto: las señales crudas no salen del host.
  Lo implementado es coherente con eso — `ReplaySource` escribe y lee archivos
  locales, y el relay Go solo mueve telemetría y señales de pánico, no
  *waveforms*.
- El perfil de calibración debe cifrarse en reposo (XChaCha20-Poly1305) con clave
  en el llavero del SO: **destino con requisito vinculante**; hoy no se
  persisten perfiles.
- Telemetría opt-in, agregada, sin *waveforms* ni *features* identificables.
- Retención: crudo 0 s por defecto; *features* para depuración 24 h y solo con
  consentimiento.
- **DPIA** antes de probar con personas; *works council* si la prueba involucra
  a plantilla en la UE.
- **Prohibición de uso evaluativo**: los datos neuronales no se usan para
  evaluar desempeño, salud ni aptitud.
- Cumplimiento: RGPD (UE), BIPA (Illinois) si aplica, HIPAA solo si hay una
  *covered entity*. El hardware BCI y la criptografía pueden estar sujetos a
  EAR/doble uso UE.

---

## 9. API pública (Rust)

Reexportada desde `tlaxtrelom_core::fulldive` y `tlaxtrelom_core::gate`:

```rust
/// Fuente de muestras. Implementaciones reales: `MockSource` y `ReplaySource`.
/// `UsbSource` es el stub del contrato: `open()` falla con error explícito.
pub trait NeuralSource: Send {
    fn poll(&mut self, out: &mut NeuralBatch) -> Result<usize>;
    fn sequence(&self) -> Option<u32> { None }        // detección de huecos
    fn sample_rate_hint(&self) -> Option<u32> { None } // un replay puede diferir
}

pub struct NeuralBatch {
    pub channels: usize,
    pub sample_rate: u32,
    pub t0_ns: u64,
    pub seq: u32,
    pub samples: Vec<f32>, // intercalado, preasignado; nunca reasigna en RT
    pub len: usize,
}

pub struct Config {           // validado por `Config::validate()`
    pub channels: u16,        // 1..=64      (defecto 8)
    pub sample_rate: u32,     // 250..=16000 (defecto 1000)
    pub core_id: usize,       // afinidad RT (defecto 3)
    pub mock: bool,           // señal sintética (defecto true)
    pub enabled: bool,        // false = modo seguro, sin hilo
    pub device: String,       // "/dev/tlaxtrelom-neuro0"
    pub replay: Option<PathBuf>,
    pub rt_priority: i32,     // 0..=99 (defecto 80)
    pub calibration_ms: u32,  // ≤ 10000 (defecto 2000)
}

pub struct FullDiveHandle { /* ... */ }
impl FullDiveHandle {
    pub fn spawn(cfg: Config) -> Result<Self>;
    pub fn snapshot(&mut self, now: Instant) -> IntentFrame; // wait-free
    pub fn next_event(&mut self) -> Option<NeuroEvent>;
    pub fn health(&self) -> Health;
}
```

El runtime consume `IntentFrame` como cualquier otro dispositivo:

```rust
let mut intent = fulldive.snapshot(now);   // wait-free
intent = gate.apply(intent, now);          // frontera de seguridad
world.fixed_update(dt, &intent);
```

Extensiones del contrato:

- `write_replay_log(path, channels, sample_rate, &samples)` genera un log
  `.tlxneuro` determinista (herramientas y tests, nunca el frame loop).
- `SomatosensoryLimiter`, `BiotelemetryWatchdog` y `GradualDetachmentController`
  son tipos públicos pensados para que un adaptador de hardware los use como
  única puerta de salida; el SDK administrado
  (`dotnet/Tlaxtrelom.Engine/FullDiveSafetyMonitor.cs`) replica sus límites:
  50 µA, 250 Hz, 2 °C y 500 ms de ventana no segura.
- La capa de plataforma (HAL en C11) expone afinidad, prioridad RT y
  `lock_memory`, y se valida por entero de ABI: `TLAXTRELOM_HAL_ABI_VERSION = 1`
  (`native/tlaxtrelom-hal/include/tlaxtrelom_hal.h`). El hilo FullDive hoy llama
  a `libc` directamente, no a través de la HAL.

---

## 10. Modos de fallo y respuesta

| Fallo | Detección | Respuesta | Estado |
|---|---|---|---|
| Muestra no finita o > 200 µV | `Decoder::push` | `ARTIFACT`, confianza 0, `NeuroEvent { kind: 1, code: 1 }`, gate neutraliza | implementado |
| `sample_rate` que no cuadra | `NeuralBatch.sample_rate` | `ARTIFACT` + `NeuroEvent { code: 2 }` | implementado |
| Huecos de secuencia | `NeuralSource::sequence()` | `Stats::gaps` (contador; no se interpola) | implementado |
| Sin frame nuevo | *watchdog* 50 ms | `snapshot()` neutraliza; el gate marca `GATED` | implementado |
| Confianza baja | `confidence < 0.35` | neutral + `GATED` | implementado |
| Electrodo caído | lead-off / impedancia en el MCU | `DEGRADED`, bajar confianza, HUD | destino |
| Saturación de ADC | amplitud > escala completa | `ARTIFACT`, descartar ventana | implementado parcial (umbral fijo) |
| Pérdida de paquetes en el cable | hueco de `seq` | interpolar ≤ 2 muestras; si no, `GAP` | destino |
| Deriva de reloj | offset > 1 ms | re-sincronía; si persiste, `GATED` | destino |
| Modelo no calibrado | ventana sin completar | `CALIBRATING`, no actuar | implementado |
| Fatiga / deriva del modelo | caída sostenida de confianza | sugerir pausa/recalibración | destino |
| Sobrecarga del host | p99 del intent > 1.5 ms | degradar *features*, priorizar seguridad | destino (no hay decisión automática) |
| Signos vitales fuera de rango | `BiotelemetryWatchdog` 500 ms | emergencia enganchada hasta `reset()` | implementado (lógica) |

---

## 11. Pruebas

Verificadas en `cargo test -p tlaxtrelom-core` (los tests viven junto al código):

- **Decoder**: impulso acotado del biquad, recuperación ante `NaN`, marcado de
  artefactos, permanencia en `CALIBRATING`, rechazo por `sample_rate`.
- **Layout**: `size_of::<IntentFrame>() == 56` y alineación 8, como contrato FFI.
- **Replay**: ida y vuelta determinista, relleno a cero de canales que faltan y
  rechazo de *magic* inválido.
- **Config**: rangos (`channels`, `sample_rate`, `rt_priority`) y modo
  deshabilitado, que debe dejar intención neutral sin crear hilo.
- **Gate**: confianza baja, *clamp* de aceleración, *dwell*, *watchdog*,
  artefacto siempre bloqueado, histéresis de zona muerta, reinicio del *dwell*
  al bloquear y E-Stop.
- **Seguridad somatosensorial/biométrica**: *clamp* de corriente y resonancia,
  neutralización de paquetes no finitos, emergencia sostenida y *fade* del
  desacoplamiento.
- **Protocolo de red**: `services/tlaxtrelom-relay/test_relay.py` valida
  serialización, CRC32 y detección de corrupción del `TLX1`.

Pendientes (destino): datasets BCI Competition IV 2a/2b para validar el decoder
*offline*, HIL con MCU + AFE + simulador de electrodos y latencia medida con
*GPIO toggle*, *fuzzing* del parser de protocolo (aún no hay objetivos
`cargo-fuzz` en el repo) y carga de 64 canales @ 2 kSPS durante 8 h.

---

## 12. Dónde encaja este documento

| Documento | Qué cubre | Relación con FullDive |
|---|---|---|
| [README del repositorio](../README.md) | quickstart y filosofía | `--probe` y `--selftest` sin hardware |
| [Arquitectura](01-architecture.md) | capas, FFI, modelo de hilos y determinismo | dónde vive el hilo RT y su triple buffer |
| [Índice de documentación](README.md) | mapa, convenciones y ABIs contractuales | define la notación "implementado vs. destino" |
| [Licencias](04-licencias.md) | MIT, SPDX, DCO, dependencias y SBOM | derriba el mito del "código propietario" (§ 1.4) |
| [VR y FullDive](05-vr-fulldive.md) | pipeline XR, *comfort*, gate, modo seguro | presenta lo que § 5 y § 7 granularizan |
| [Lenguajes](06-lenguajes.md) | fronteras Lua, C#, Python, TS, GDScript | cómo se expone un `NeuralSource` personalizado |
| [Estabilidad](07-estabilidad.md) | invariantes y deuda viva | el estado real del crate de protocolo y del transporte USB |
| [Operaciones](08-operaciones.md) | compilación, CI, releases | cómo se construye `services/tlaxtrelom-relay` |

Contrapartes de código citadas a lo largo del documento, todas existentes:
`crates/tlaxtrelom-core/src/fulldive.rs`, `gate.rs`, `telemetry.rs`, `xr.rs`,
`coords.rs`, `main.rs`, `services/tlaxtrelom-relay/` (Go) y
`dotnet/Tlaxtrelom.Engine/FullDiveSafetyMonitor.cs` (espejo administrado).