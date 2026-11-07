# Tlaxtrelom Engine — Arquitectura Multiplataforma

> Versión 0.2 · Alcance: soporte nativo para **Linux**, **Windows**, **macOS** y **HarmonyOS for PC**.

---

## 0. Filosofía de Portabilidad

Tlaxtrelom Engine no utiliza capas de abstracción pesadas ni emuladores en tiempo de ejecución. Cada plataforma se integra a través de contratos de ABI C planos (`HAL`, `Geometry`, `Spatial`, `XR`), adaptadores nativos y backends gráficos directos de bajo nivel:

```text
┌─────────────────────────────────────────────────────────────────────────────┐
│                       TLAXTRELOM ENGINE CORE (Rust)                        │
│                 Bucle determinista · ECS · FullDive Gate · XR               │
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

---

## 1. Matriz de Plataformas

| Característica | Linux | Windows (Win32) | macOS (Darwin) | HarmonyOS for PC |
|---|---|---|---|---|
| **API Gráfica Primaria** | Vulkan 1.3 | DirectX 12 / Vulkan | Metal 3 | Vulkan 1.2+ / GLES |
| **Backend `wgpu`** | `Vulkan` | `Dx12` / `Vulkan` | `Metal` | `Vulkan` / `Gl` |
| **Gestor de Ventanas** | Wayland / X11 (`winit`) | Win32 `HWND` | AppKit `NSWindow` | `XComponent` Rosen |
| **Reloj de Alta Resolución** | `clock_gettime(CLOCK_MONOTONIC)` | `QueryPerformanceCounter` | `mach_absolute_time` | `clock_gettime(CLOCK_MONOTONIC)` |
| **Afinidad de Núcleos** | `sched_setaffinity` | `SetThreadAffinityMask` | `THREAD_AFFINITY_POLICY` | `sched_setaffinity` |
| **Prioridad de Hilos RT** | `SCHED_FIFO` [1, 99] | `THREAD_PRIORITY_TIME_CRITICAL` | `sched_priority` RR | `SCHED_FIFO` [1, 99] |
| **Bloqueo de Memoria** | `mlockall(MCL_CURRENT)` | `SetProcessWorkingSetSize` | `mlockall(MCL_CURRENT)` | `mlockall(MCL_CURRENT)` |
| **I/O Asíncrono de Mallas**| `pread` / `io_uring` | `ReadFile` (OVERLAPPED) | `pread` | `pread` / musl POSIX |
| **Formato de Biblioteca** | `.so` | `.dll` | `.dylib` | `.so` (OpenHarmony NDK)|

---

## 2. Detalles de Implementación por Sistema

### 2.1 Linux
- **Objetivo**: Estaciones de desarrollo, servidores dedicados y cascos VR autónomos basados en Linux.
- **Gráficos**: Vulkan 1.3 con soporte de extensiones multiview (`VK_KHR_multiview`) para estéreo single-pass.
- **Sincronización**: Hilo FullDive con `mlockall` y prioridad de tiempo real para asegurar latencia de procesamiento $L_{proc} < 1\text{ ms}$.

### 2.2 Windows
- **Objetivo**: PCs de escritorio de alto rendimiento, SteamVR, Meta Quest Link y simuladores empresariales.
- **DirectX 12 y Vulkan**: `wgpu` selecciona DX12 o Vulkan automáticamente según drivers disponibles.
- **I/O de Geometría Virtual**: Implementa `platform_pread` con llamadas no bloqueantes `ReadFile` utilizando offsets `OVERLAPPED` de 64 bits sin desplazar el puntero global del archivo.
- **Compilación**: Compatible con MSVC (`cl.exe`) y MinGW (`gcc`/`clang`).

### 2.3 macOS
- **Objetivo**: Estaciones Apple Silicon (M1/M2/M3/M4) e Intel Mac.
- **Metal 3**: Shaders traducidos internamente a MSL (Metal Shading Language) por Naga/wgpu.
- **Optimizaciones ARM64**: Aceleración vectorial por hardware mediante instrucciones NEON (`SimdLevel = 1`).
- **Temporización**: Uso de `mach_timebase_info` para conversión exacta de ticks de CPU a nanosegundos sin fluctuación de frecuencia.

### 2.4 HarmonyOS for PC (OpenHarmony NEXT PC)
- **Objetivo**: Computadoras de escritorio y portátiles ejecutando HarmonyOS PC (arquitecturas Kirin PC ARM64 y x86_64).
- **Integración XComponent**: Módulo nativo [`native/tlaxtrelom-harmonyos/`](../native/tlaxtrelom-harmonyos/) que conecta la ventana gráfica de ArkUI con el render loop nativo.
- **Kernel HongMeng**: Aprovecha el subsistema POSIX de alta eficiencia con `sched_setaffinity` y `sysconf` para detección de núcleos y memoria física disponible.

---

## 3. Comprobación y CI

Para validar la compilación multiplataforma y los puentes nativos:

```bash
# Smoke test de puente HarmonyOS PC
make -C native/tlaxtrelom-harmonyos test

# Smoke test de cinemática espacial C/Zig
make -C native/tlaxtrelom-spatial test

# Smoke test de isla C++ de geometría
make -C native/tlaxtrelom-geometry test

# Ejecución completa de la suite
cargo xtask ci
```
