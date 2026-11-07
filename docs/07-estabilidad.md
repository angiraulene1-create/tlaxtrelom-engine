# Tlaxtrelom Engine — Estabilidad

> Versión 0.2 · Alcance: invariantes de runtime, catálogo de fallos, límites de
> recursos, determinismo y estrategia de pruebas.

---

## 0. Qué significa "estable" aquí

No significa "nunca falla". Significa:

1. **Falla temprano y con contexto** en vez de corromper estado tres horas
   después.
2. **Un subsistema no tumba a los demás**: el render se recupera de una
   superficie perdida; un script no bloquea el host; un frame BCI inválido se
   neutraliza.
3. **El comportamiento es reproducible**: mismo input, misma versión y mismo
   hardware ⇒ mismo hash de simulación.
4. **Los límites son explícitos**: memoria, instrucciones, colas, pasos por
   frame y presupuesto de latencia.

---

## 1. Invariantes del runtime

| Invariante | Verificación |
|---|---|
| Toda configuración se valida antes de crear GPU/ventana | `Config::validate` |
| La simulación avanza a paso fijo y con tope por frame | `max_sim_steps`, tests de pacer |
| Un `NaN`/`Inf` no entra en matrices ni ejes | validación en XR, gate y Lua |
| El gate neutraliza sin frame nuevo | watchdog de 50 ms, tests |
| La isla C++ rechaza ABI incompatible | `GEO_ABI_VERSION`, smoke test |
| Un script no puede escapar del sandbox | suite Lua (14 tests) |
| El hash HIR es idéntico en Python y TypeScript | fixture compartido |
| El frame loop no asigna memoria en el hilo RT | contrato de `FullDive` |
| Un error de superficie no aborta si es recuperable | `SurfaceError::Lost/Outdated` |

---

## 2. Catálogo de fallos y respuesta

### 2.1 GPU / ventana

| Fallo | Respuesta |
|---|---|
| `SurfaceError::Lost` / `Outdated` | reconfigurar superficie y continuar |
| `SurfaceError::Timeout` | reintentar; contar error |
| `SurfaceError::OutOfMemory` | abortar limpio con diagnóstico |
| Adaptador ausente | `--probe` explica qué backend falta |
| Shader inválido | falla el build (test de parseo WGSL) |

Nunca se avanza la simulación más de `max_sim_steps` tras un stall. Un
breakpoint o una suspensión del portátil no produce una cascada de ticks.

### 2.2 XR

| Fallo | Respuesta |
|---|---|
| Pose no disponible | reutilizar la última pose válida, marcar el frame |
| IPD/FOV fuera de rango | rechazar en arranque |
| Frames tardíos consecutivos | bajar `render_scale` con histéresis |
| Matriz no finita | rechazar el frame, no enviarlo a GPU |

### 2.3 FullDive

| Fallo | Respuesta |
|---|---|
| Sin frames nuevos | watchdog ⇒ neutral + `GATED` |
| Confianza baja | neutral |
| Artefacto / electrodo caído | `ARTIFACT`, nunca intención válida |
| Replay inexistente | error de configuración al arrancar |
| Hardware ausente con `mock=false` | error accionable; no se inventan señales |
| Modo deshabilitado | intención neutral, sin hilo ni transporte |

### 2.4 Scripting Lua

| Fallo | Respuesta |
|---|---|
| Error de sintaxis | se reporta, el host sigue vivo |
| Bucle infinito | presupuesto de instrucciones lo corta |
| Allocación desmedida | presupuesto de memoria lo corta |
| `pcall` intenta ocultar el corte | el error de presupuesto se re-lanza |
| Cola de eventos llena | backpressure explícita |
| Acceso a `os`/`io`/`debug` | error de global no permitido |

### 2.5 Geometría C++

| Fallo | Respuesta |
|---|---|
| Asset con magic/versión inválidos | `ERR_FORMAT`; no se registra |
| Archivo inexistente | `ERR_IO` |
| Doble `ack_upload` | `ERR_STATE` |
| ABI distinta | rechazo en carga |
| Página corrupta | se descarta; el contador de fallos sube |

---

## 3. Límites de recursos

| Recurso | Límite | Dónde |
|---|---|---|
| Pasos de simulación por frame | 8 (configurable) | `Config::max_sim_steps` |
| `dt` máximo tras stall | 100 ms | `Config::max_frame_dt` |
| Canales BCI | 64 | `MAX_CHANNELS` |
| Sample rate BCI | 250–16 000 Hz | `Config::validate` |
| Cola de eventos Lua | 4096 | `script/tlaxtrelom/init.lua` |
| Logs Lua | 256 (ring) | `script/tlaxtrelom/init.lua` |
| Presupuesto Lua por defecto | 50 000 instrucciones / 16 MiB | `Runtime.new` |
| Acciones discretas | 16 bits | `gate::MAX_ACTIONS` |
| Páginas listas de geometría | configurable | `TlaxtrelomGeoConfig` |

Los límites son parte del contrato: subirlos exige revisar el peor caso de
latencia, no solo el caso feliz.

---

## 4. Determinismo

- **FP estricto** en simulación: sin `fast-math` ni reasociación.
- **Paso fijo**: el acumulador de simulación no depende del frame rate.
- **RNG explícito**: el RNG de Lua es un LCG sembrado por el host, no
  `math.random` global.
- **Hash de simulación**: `--selftest` compara el estado tras N ticks.
- **Hash de HIR**: forma canónica compartida entre Python y TypeScript.

Un test de determinismo que solo compara "no explotó" no sirve. Los tests
comparan hashes y fallan si cambian sin una actualización deliberada del
fixture.

---

## 5. Seguridad de FFI

- Handles opacos y versión de ABI al crear la isla.
- Sin excepciones C++ cruzando la ABI.
- Sin `panic!`/`unwrap` cruzando la ABI.
- Ownership documentado: quién asigna, quién libera y hasta cuándo es válido un
  puntero.
- `unsafe` acotado y documentado; Clippy lo vigila.
- Los tests de ABI son en C y C++ reales, no solo en Rust.

---

## 6. Estrategia de pruebas

| Capa | Cantidad actual | Comando |
|---|---|---|
| Rust | 61 tests | `cargo test --workspace` |
| Python | 39 tests | `cargo xtask python` |
| Lua | 14 tests | `cargo xtask lua` |
| TypeScript | 24 tests | `cargo xtask node` |
| C++ ABI | smoke test completo | `cargo xtask native-test` |
| Selftest | 600 ticks + 240 frames XR | `cargo xtask ci` |
| Docs | enlaces locales | `cargo xtask docs` |

La matriz de CI es exactamente `cargo xtask ci`. Si un comando funciona en
local pero no en CI, es un bug de la matriz, no una excusa.

---

## 7. Compatibilidad de plataforma

- **Linux x86_64**: plataforma de referencia (Vulkan, HAL C11, `SCHED_FIFO`).
- **Windows/macOS**: el runtime Rust compila; la HAL C11 usa rutas portables o
  stub. Verificar `--probe` y el backend `wgpu` antes de prometer soporte.
- **Sin GPU**: `--selftest` y `--headless` no requieren adaptador.
- **Sin C**: `--no-default-features` produce un binario Rust-only para entornos
  restringidos.

Una plataforma solo se declara soportada si pasa la matriz y el selftest en
hardware real.

---

## 8. Qué haría fallar una release

- `cargo xtask ci` en rojo.
- Cambio de hash de simulación o de HIR sin actualizar el fixture.
- Aumento de dependencias fuera de la allowlist de `deny.toml`.
- Cualquier ruta que permita a un script Lua tocar filesystem/red.
- Cualquier actuador FullDive sin watchdog o parada física documentada.
- Documentación que prometa una capacidad no implementada.

---

## 9. Deuda conocida

- Backend OpenXR real: contrato definido, integración pendiente.
- Transporte BCI de hardware: formato de replay y contrato definidos; falta el
  driver de dispositivo.
- Cobertura de pruebas de estrés de GPU y de sesiones XR largas.
- Firmado de artefactos y SBOM: política documentada; falta automatizar en CI.
- El conteo de instrucciones de Lua es una **cota inferior** muestreada por
  hooks; no es un contador exacto de instrucciones VM.
