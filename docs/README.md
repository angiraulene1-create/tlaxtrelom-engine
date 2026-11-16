<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 2026 angiraulene1-create -->

# Documentación de Tlaxtrelom Engine

> Versión 0.2 · Documento vivo bajo **licencia MIT** (ver
> [LICENSE](../LICENSE)). Todo enlace local del README, de `docs/` y de los
> metadatos se valida en CI con `cargo xtask docs`.

---

## Mapa

| # | Documento | Cuándo leerlo |
|---|---|---|
| — | [README del repositorio](../README.md) | visión general y quickstart |
| 01 | [Arquitectura](01-architecture.md) | capas, FFI, hilos, build, ADRs |
| 02 | [FullDive](02-fulldive.md) | hardware BCI, protocolo, latencia, seguridad |
| 03 | [Tlaxtrelom IR](03-tlaxtrelom-ir.md) | tipos, efectos, HIR/MIR, frontends, backends |
| 04 | [Licencias y cadena de suministro](04-licencias.md) | MIT, SPDX, dependencias, SBOM, firma |
| 05 | [VR y FullDive](05-vr-fulldive.md) | pipeline XR, confort, gate, modos seguros |
| 06 | [Lenguajes](06-lenguajes.md) | fronteras y contrato de cada lenguaje |
| 07 | [Estabilidad](07-estabilidad.md) | invariantes, fallos, determinismo, pruebas |
| 08 | [Operaciones](08-operaciones.md) | build, CI, release, diagnóstico |
| 09 | [Multiplataforma](09-multiplataforma.md) | Linux, Windows, macOS y HarmonyOS for PC |

Metadatos del repositorio, también validados en CI: [LICENSE](../LICENSE),
[CHANGELOG](../CHANGELOG.md), [CONTRIBUTING](../CONTRIBUTING.md) y
[SECURITY](../SECURITY.md).

---

## Orden de lectura recomendado

### Para entender el motor

1. [README](../README.md) — tesis y quickstart.
2. [Arquitectura](01-architecture.md) — por qué Rust posee el runtime.
3. [Lenguajes](06-lenguajes.md) — qué hace cada uno y qué no.
4. [Tlaxtrelom IR](03-tlaxtrelom-ir.md) — el contrato entre autoría y backends.

### Para trabajar en VR/FullDive

1. [VR y FullDive](05-vr-fulldive.md) — pipeline y modos seguros.
2. [FullDive](02-fulldive.md) — latencia honesta y seguridad BCI.
3. [Estabilidad](07-estabilidad.md) — watchdog, gate y límites.

### Para operar el repositorio

1. [Operaciones](08-operaciones.md) — build, tests, CI, release.
2. [Estabilidad](07-estabilidad.md) — qué haría fallar una release.
3. [Licencias](04-licencias.md) — SPDX, dependencias y SBOM.

---

## Convenciones

- **SPDX** obligatorio en código nuevo:
  `SPDX-License-Identifier: MIT`.
- **Versiones contractuales, literalmente**: ABIs geometría **2** (`TLXG` v2),
  spatial **1**, HarmonyOS **1**, HAL **1**; MSRV Rust 1.83; Python 3.11+,
  Lua 5.4+, Node 22.6+. La documentación debe citar estos números como están
  en el código, no como diana.
- **Español** para documentación y mensajes de usuario; inglés para
  identificadores de código cuando el ecosistema lo exige.
- **Sin promesas no implementadas.** Si una capacidad es un contrato, se marca
  como tal; si es deuda, se lista en [Estabilidad § 9](07-estabilidad.md).
- **Los enlaces locales deben resolver.** CI falla si un enlace apunta a un
  archivo inexistente.

---

## Documentos que no existen (a propósito)

- Un documento que prometa latencia FullDive end-to-end < 1 ms: es
  neurofisiológicamente falso. Ver [FullDive § 0](02-fulldive.md).
- Un documento que prometa paridad con UE5: el alcance está en
  [Arquitectura](01-architecture.md).
- Un CLA: con MIT basta el sign-off DCO de cada commit. Ver
  [Licencias § 2](04-licencias.md).
- Un tutorial de plugins de terceros antes de que exista una ABI de plugins
  estable.
