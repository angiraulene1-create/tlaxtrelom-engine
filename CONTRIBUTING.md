<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 2026 angiraulene1-create -->

# Contribuir a Tlaxtrelom Engine

> Repositorio de **código abierto bajo licencia MIT**. Se aceptan contribuciones
> externas por Pull Request, con sign-off DCO y la matriz de CI en verde.

---

## 1. Antes de escribir código

1. Lee [Arquitectura](docs/01-architecture.md) y
   [Lenguajes](docs/06-lenguajes.md).
2. Verifica que el cambio pertenece al lenguaje correcto. Si toca el frame
   loop, es Rust. Si toca streaming de geometría, es C++. Si es autoría, va
   fuera del runtime.
3. Si el cambio altera un contrato (ABI, schema HIR, formato cocinado, hash),
   actualiza **todos** los consumidores en el mismo commit:
   - ABI: header C/C++ + constante Rust + smoke test. Versiones actuales:
     geometría **2** (`GEO_ABI_VERSION`, formato cocinado `TLXG` v2),
     spatial **1**, HarmonyOS **1**, HAL **1**
     (`TLAXTRELOM_HAL_ABI_VERSION 1u`).
   - HIR: Python + TypeScript + fixtures de hash.
   - Formato de asset: constante de versión + test de rechazo.
4. Si el cambio afecta a seguridad física o FullDive, revisa
   [SECURITY.md](SECURITY.md) y [VR/FullDive](docs/05-vr-fulldive.md).

---

## 2. Convenciones

- **Cabecera SPDX** en todo archivo nuevo:
  ```text
  SPDX-License-Identifier: MIT
  Copyright (c) 2026 angiraulene1-create
  ```
- **Formato**: `cargo fmt --all`. El CI ejecuta `--check`.
- **Lints**: `cargo clippy --workspace --all-targets --all-features -- -D warnings`.
- **Sin `unwrap`/`expect`** en rutas de runtime; usa errores con contexto.
- **Sin `panic!` cruzando FFI.** Código de error, no unwind.
- **Idioma**: documentación y mensajes de usuario en español; identificadores
  en inglés cuando el ecosistema lo exija.
- **Commits**: imperativo, alcance corto. Ejemplo:
  `xtask: valida enlaces locales antes de CI`.
- **Sign-off**: cada commit lleva `Signed-off-by:` (DCO 1.1) declarando que se
  tiene derecho a licenciar el parche bajo MIT.
- **Un cambio, una intención.** No mezclar refactor masivo con feature.

---

## 3. Código por lenguaje

### Rust

- MSRV **1.83** (`rust-version` en el workspace y `clippy.toml`).
- `unsafe` solo con un bloque `// SAFETY:` que explique el invariante.
- No añadir dependencias sin justificar licencia y tamaño.
- Tests unitarios junto al módulo; tests de integración cuando cruzan FFI.
- Errores con `anyhow::Context` en el host y tipos propios en librerías.

### C++

- C++20, sin excepciones ni RTTI cruzando la ABI.
- `-Wall -Wextra -Wpedantic -Wconversion -Wshadow` limpios.
- Ownership documentado en el header.
- Cualquier cambio de layout exige subir `GEO_ABI_VERSION` y el smoke test.

### C11

- Sin dependencias externas.
- `_GNU_SOURCE`/`_DEFAULT_SOURCE` solo cuando la API lo requiera.
- Fallback portable cuando la plataforma no soporte una capacidad.

### Python

- Python 3.11+, sin dependencias obligatorias.
- Sin `pickle` para datos del motor.
- Tests con `unittest`; la CLI debe fallar con código 2 y mensaje claro.

### Lua

- Sin globals no declaradas.
- Sin `os`, `io`, `package`, `require`, `debug`.
- Todo script debe poder cortarse por presupuesto.
- Los tests viven en `script/tests/run.lua`.

### TypeScript

- Node 22.6+, módulos ESM, imports con extensión `.ts`.
- Sin dependencias de runtime.
- El hash y la forma canónica deben coincidir con Python; los fixtures son
  vinculantes.

---

## 4. Pruebas obligatorias

Antes de pedir revisión:

```bash
cargo xtask ci
```

Ese comando ejecuta, en un solo entorno Linux, los 15 pasos de la matriz:
`fmt`, `clippy -D warnings`, geometría C++ (`all` + `test`), spatial C,
HarmonyOS, `glslc` + `spirv-val` de los 3 shaders GLSL, `cargo test
--workspace`, `cargo test --workspace --features native-geometry`, contratos
C#, relay Go, Python, Lua, Node/TS, selftest headless `--json` y validación de
enlaces de docs. No hay atajos: si falla en local, falla en CI (el workflow
`ci.yml` invoca exactamente este comando).

Añade tests para:

- todo bug corregido;
- todo contrato nuevo (ABI, schema, formato, hash);
- todo límite de seguridad (presupuesto, watchdog, gate);
- toda ruta de error que antes era un `panic`.

---

## 5. Documentación

- Actualiza el documento afectado en el mismo commit.
- No prometas capacidades no implementadas. Si es un contrato, dilo.
- Los enlaces locales deben resolver (`cargo xtask docs`).
- Añade una entrada en [CHANGELOG.md](CHANGELOG.md) para cambios visibles.

---

## 6. Revisión

Un cambio se acepta si:

1. La matriz de CI está en verde.
2. El diseño respeta la frontera de lenguajes y el ownership.
3. Hay tests que fallarían sin el cambio.
4. La documentación refleja el estado real.
5. No introduce secretos, dependencias no aprobadas ni deuda de seguridad.
6. Si toca FullDive, un revisor adicional de seguridad lo aprueba.

---

## 7. Licencia y atribución

- El repositorio está bajo [MIT](LICENSE); al enviar un PR licencias tu
  contribución bajo esa misma licencia. No hay cesión de derechos ni CLA.
- Cada archivo nuevo conserva la cabecera SPDX MIT.
- No copiar código de terceros sin revisar su licencia: la allowlist está en
  [deny.toml](deny.toml) y las reglas completas en
  [Licencias y cadena de suministro](docs/04-licencias.md).
- Nada de secretos en el repo, ni en un PR: un repositorio público los filtra
  en cuanto se commitean.
- El nombre «Tlaxtrelom Engine» no se licencia con el código; un derivado se
  presenta como derivado.
