<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 2026 angiraulene1-create -->

# Política de seguridad — Tlaxtrelom Engine

> Proyecto de **código abierto bajo licencia MIT**. Los fallos de seguridad se
> reportan por privado antes de abrir cualquier issue público: este repositorio
> es visible para todo el mundo, y un issue con la explicación de un fallo es
> ya el exploit. Usa los **Private security advisories** de GitHub (Security →
> Report a vulnerability).

---

## 1. Versiones soportadas

| Versión | Soporte | Notas |
|---|---|---|
| `0.2.x` | Activo | runtime, HAL, geometría, tooling |
| `0.1.x` | Solo lectura | sin parches salvo riesgo crítico |

Una versión deja de recibir parches cuando se publica la siguiente minor. Las
ramas de desarrollo no tienen SLA: si un fallo solo existe en `main`, se arregla
y ya.

---

## 2. Cómo reportar

1. Abre una **security advisory privada** en el repositorio. No uses issues
   públicos, Discussions ni redes sociales para describir el fallo.
2. Incluye:
   - versión y commit exactos;
   - plataforma, driver y hardware;
   - pasos mínimos de reproducción;
   - impacto esperado (corrupción, fuga, DoS, seguridad física);
   - si el fallo afecta a FullDive o a un actuador, márcalo como **crítico**.
3. No publiques detalles hasta que el parche esté disponible. La ventana de
   divulgación coordinada objetivo es de 90 días o la publicación del parche,
   lo que ocurra antes.

No hay programa de bug bounty ni recompensas: el proyecto es MIT y sin
financiación para pagos por fallos.

---

## 3. Alcance

### En alcance

- Runtime Rust: memoria, FFI, recuperación de superficie, gate y watchdog.
- Coordenadas de dos capas (`coords.rs`): aritmética `f64`/`f32`, rebase a
  1024 m y alcance de sector (`--sector` hasta 1e8 m).
- Isla C++: parsing de assets `TLXG` v2 (`kAssetVersion = 2`), descompresión,
  frustum culling, contratos de ownership.
- HAL C11 (ABI 1): afinidad, memoria bloqueada, hashing.
- Islas C/Zig de cinemática espacial (ABI 1) y del puente HarmonyOS (ABI 1):
  ABI y validación de argumentos.
- Relay Go y SDK C#: framing de red, P/Invoke y validación de poses.
- Shaders WGSL y GLSL: negociación de MSAA y formatos HDR.
- Tooling Python/TypeScript: validación de HIR, hashing, CLI.
- Sandbox Lua: escape de globals, presupuestos, carga de scripts.
- Cadena de build y artefactos: dependencias, SBOM, firma.

### Fuera de alcance

- Hardware BCI de terceros (se reporta a su fabricante, coordinando la cadena).
- Drivers gráficos y runtimes OpenXR.
- Infraestructura de red/CI de la organización.
- Ingeniería social y ataques físicos.

---

## 4. Seguridad física y FullDive

Cualquier fallo que permita:

- saltarse el `IntentGate` o el watchdog;
- accionar un actuador crítico sin parada física independiente;
- interpretar artefactos como intención válida;
- mantener una acción "pegada" al perder foco o sesión;

se trata como **crítico**, con parche y revisión antes de cualquier otra tarea.

El motor no sustituye una parada de emergencia certificada. Un casco BCI nunca
debe controlar un actuador crítico solo por software.

---

## 5. Divulgación

- Confirmación al reportador en un plazo objetivo de 2 días hábiles.
- Evaluación y mitigación según severidad; los críticos (sección 4) van primero.
- El fallo y su parche se documentan en `CHANGELOG.md` y, si corresponde, en la
  advisory pública, cuando la mitigación está desplegada.
- Los agradecimientos a quien reporta salen del texto de la advisory, no de un
  anuncio previo.

---

## 6. Buenas prácticas para contribuidores

- No incluir secretos, claves, tokens ni datos personales en commits, tests,
  fixtures o logs.
- No desactivar el sandbox Lua ni el gate para "probar rápido".
- No añadir dependencias fuera de la allowlist de `deny.toml` sin revisión.
- Ejecutar `cargo xtask ci` antes de pedir revisión.
- Reportar cualquier `unsafe` nuevo con su invariante y su test.
- No commitear secretos: un repositorio público los filtra en cuanto se
  suben (`gitleaks` los busca, pero la responsabilidad es del autor).
