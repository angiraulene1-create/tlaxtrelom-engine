<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 2026 angiraulene1-create -->

# Tlaxtrelom Engine — Arquitectura Multiplataforma

> Versión 0.2 · Alcance: soporte verificado en **Linux** (CI) y rutas de
> portabilidad **objetivo/contrato** para **Windows**, **macOS** y
> **HarmonyOS for PC** (no verificadas en CI).

---

## 0. Filosofía de Portabilidad

Tlaxtrelom Engine no utiliza capas de abstracción pesadas ni emuladores en tiempo de ejecución. Cada plataforma se integra a través de contratos de ABI C planos (`HAL`, `Geometry`, `Spatial`, `XR`), adaptadores nativos y backends gráficos directos de bajo nivel:

```text
┌─────────────────────────────────────────────────────────────────────────────┐
│                       TLAXTRELOM ENGINE CORE (Rust)                        │
│          Bucle determinista · mundo · FullDive Gate · XR · wgpu            │
└──────┬──────────────────────┬──────────────────────┬────────────────────────┘
       │                      │                      │
       ▼                      ▼                      ▼
┌──────────────┐       ┌──────────────┐       ┌──────────────┐       ┌──────────────┐
│    LINUX     │       │   WINDOWS    │       │    MACOS     │       │  HARMONYOS   │
│              │       │              │       │              │       │    FOR PC    │
│ Vulkan 1.3   │       │ DX12 / VK    │       │ Metal 3      │       │ Vulkan / GL  │
│ Wayland/X11  │       │ Win32 API    │       │ Cocoa/AppKit │       │ XComponent   │
│ sched_fifo   │       │ QPC / WinRT  │       │ mach_time    │       │ HongMeng NDK │
└──────────────┘       └──────────────┘       └──────────────┘       └──────────────┘
```

**Advertencia honesta:** Linux es la única plataforma donde este repo se compila,
renderiza y testa en CI (un solo runner Linux). Windows, macOS y HarmonyOS son
**objetivos** soportados por diseño (backends `wgpu`, ABI C planos, HAL con stub
portable), no entornos verificados. Ninguna celda de la matriz de abajo debe
leerse como un "✓ probado".

---

## 1. Matriz de Plataformas

Leyenda: **Objetivo** = ruta diseñada, sin verificación en CI. **Contrato
(smoke)** = compilación/tests de humo del puente nativo sobre Linux; no prueba
el SO destino. **Verificado** = se ejecuta en CI.

| Característica | Linux | Windows (Win32) | macOS (Darwin) | HarmonyOS for PC |
|---|---|---|---|---|
| **API Gráfica Primaria** | Verificado: Vulkan | Objetivo: DX12 / Vulkan (elección de `wgpu`) | Objetivo: Metal 3 (vía `wgpu`) | Contrato (smoke): Vulkan / GLES |
| **Backend `wgpu`** | Verificado: `Vulkan` | Objetivo: `Dx12` / `Vulkan` | Objetivo: `Metal` | Objetivo: `Vulkan` / `Gl` |
| **Gestor de Ventanas** | Verificado: Wayland / X11 (`winit`) | Objetivo: Win32 `HWND` (`winit`) | Objetivo: AppKit `NSWindow` (`winit`) | Contrato (smoke): `XComponent` Rosen |
| **Reloj de Alta Resolución** | Verificado: `clock_gettime(CLOCK_MONOTONIC)` | Objetivo: `QueryPerformanceCounter` | Objetivo: `mach_absolute_time` | Contrato (smoke): `clock_gettime` |
| **Afinidad de Núcleos** | Verificado: `sched_setaffinity` | Objetivo: `SetThreadAffinityMask` | Objetivo: `THREAD_AFFINITY_POLICY` | Contrato (smoke): `sched_setaffinity` |
| **Prioridad de Hilos RT** | Verificado: `SCHED_FIFO` [1, 99] | Objetivo: `THREAD_PRIORITY_TIME_CRITICAL` | Objetivo: política RR | Contrato (smoke): `SCHED_FIFO` |
| **Bloqueo de Memoria** | Verificado: `mlockall` | Objetivo: `SetProcessWorkingSetSize` | Objetivo: `mlockall` | Contrato (smoke): `mlockall` |
| **I/O de Mallas** | Verificado: `pread`/`preadv` en hilo de IO dedicado | Objetivo: `ReadFile` con `OVERLAPPED` | Objetivo: `pread` | Objetivo: `pread` / musl POSIX |
| **Formato de Biblioteca** | Verificado: `.so` | Objetivo: `.dll` | Objetivo: `.dylib` | Objetivo: `.so` (NDK OpenHarmony) |

### 1.1 La garantía multiplataforma que sí existe

La portabilidad real del renderer no descansa en promesas de backend, sino en
**negociación por adaptador en tiempo de ejecución** (`renderer.rs`):

- **Cascada de formato HDR**: la escena se dibuja en `Rgba16Float`; si el
  adaptador no lo soporta para uso como target, se degrada a `Rgba32Float`; y
  si tampoco, a 8 bits por canal (el tonemap ACES pierde alcance, no corrección).
  Un adaptador débil nunca impide arrancar.
- **Negociación de MSAA**: las muestras se piden contra el target HDR y el
  depth `Depth32Float`; MSAA 2× solo se usa si el device reporta
  `TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES` (el soporte 2× depende del
  adaptador). Sin la feature, el rango seguro es 1× o 4×.
- **HAL con fallback**: sin la feature `native-hal` (`--no-default-features`)
  el binario usa un stub portable en Rust puro.

---

## 2. Detalles de Implementación por Sistema

### 2.1 Linux
- **Objetivo**: Estaciones de desarrollo, servidores dedicados y cascos VR autónomos basados en Linux. **Es la plataforma de referencia: la única verificada en CI.**
- **Gráficos**: Vulkan vía `wgpu`. El estéreo se resuelve en **un pase de escena con un draw recortado por ojo** (side-by-side) contra el target HDR, más un pase de presentación; **no** se usa multiview ni `VK_KHR_multiview` en el path actual — es destino, no presente.
- **Sincronización**: Hilo FullDive con `mlockall` y prioridad de tiempo real para asegurar latencia de procesamiento $L_{proc} < 1\text{ ms}$. La integración RT del runtime Rust (`libc`: `SCHED_FIFO`, `mlockall`, afinidad) es una dependencia **solo-linux** (`[target.'cfg(target_os = "linux")']`): en los demás SOs ese camino ni se compila en CI, luego no está verificado.

### 2.2 Windows *(objetivo; sin verificación en CI)*
- **Objetivo**: PCs de escritorio de alto rendimiento, SteamVR, Meta Quest Link y simuladores empresariales.
- **DirectX 12 y Vulkan**: `wgpu` selecciona DX12 o Vulkan automáticamente según drivers disponibles; la cascada HDR/MSAA de §1.1 es lo que hace el arranque tolerante a drivers dispares.
- **I/O de Geometría Virtual**: diseño previsto: `platform_pread` con `ReadFile` y offsets `OVERLAPPED` de 64 bits. Verificado solo como contrato de la isla C++ en Linux (`pread`); sin build con MSVC en CI.
- **Compilación**: compatible por diseño con MSVC (`cl.exe`) y MinGW; no probado en CI.

### 2.3 macOS *(objetivo; sin verificación en CI)*
- **Objetivo**: Estaciones Apple Silicon (M1/M2/M3/M4) e Intel Mac.
- **Metal 3**: shaders WGSL traducidos a MSL (Metal Shading Language) por `naga`/`wgpu`.
- **Optimizaciones ARM64**: detección de nivel SIMD vía HAL (stub portable fuera de Linux).
- **Temporización**: diseño previsto sobre `mach_timebase_info`; no compilado en CI.

### 2.4 HarmonyOS for PC (OpenHarmony NEXT PC) *(contrato; sin host OHOS real)*
- **Objetivo**: Computadoras de escritorio y portátiles ejecutando HarmonyOS PC (Kirin PC ARM64 y x86_64).
- **Integración XComponent**: módulo nativo [`native/tlaxtrelom-harmonyos/`](../native/tlaxtrelom-harmonyos/) (`ABI_VERSION = 1`) que conecta la ventana gráfica de ArkUI con el render loop nativo. **El puente es un contrato probado por smoke test en Linux: no existe un host OpenHarmony real en CI**, por lo que ni el NDK oficial ni Rosen se han ejercitado de verdad.
- **Kernel HongMeng**: uso previsto del subsistema POSIX (`sched_setaffinity`, `sysconf`); verificado solo contra el stub del smoke test.

---

## 3. Comprobación y CI

La CI de este repo corre en **un único runner Linux**. Los comandos de abajo
son smoke tests nativos de Linux: validan ABI, compilación y lógica de los
puentes, **no** verifican Windows, macOS ni HarmonyOS.

```bash
# Smoke test de puente HarmonyOS PC (en Linux, sin NDK ni host OHOS reales)
make -C native/tlaxtrelom-harmonyos test

# Smoke test C de la cinemática espacial (Zig no se compila; la ruta C es la enlazada)
make -C native/tlaxtrelom-spatial test

# Smoke test de isla C++ de geometría
make -C native/tlaxtrelom-geometry test

# Ejecución completa de la suite (solo Linux)
cargo xtask ci
```
