# Contribuir a Tlaxtrelom Engine

> Repositorio **propietario y confidencial**. Solo colaboradores autorizados.
> No se aceptan contribuciones externas sin acuerdo escrito y revisión legal.

---

## 1. Antes de escribir código

1. Lee [Arquitectura](docs/01-architecture.md) y
   [Lenguajes](docs/06-lenguajes.md).
2. Verifica que el cambio pertenece al lenguaje correcto. Si toca el frame
   loop, es Rust. Si toca streaming de geometría, es C++. Si es autoría, va
   fuera del runtime.
3. Si el cambio altera un contrato (ABI, schema HIR, formato cocinado, hash),
   actualiza **todos** los consumidores en el mismo commit:
   - ABI: header C + Rust + smoke test.
   - HIR: Python + TypeScript + fixtures de hash.
   - Formato de asset: constante de versión + test de rechazo.
4. Si el cambio afecta a seguridad física o FullDive, revisa
   [SECURITY.md](SECURITY.md) y [VR/FullDive](docs/05-vr-fulldive.md).

---

## 2. Convenciones

- **Cabecera SPDX** en todo archivo nuevo:
  ```text
  SPDX-License-Identifier: LicenseRef-Tlaxtrelom-Proprietary
  Copyright (c) 2026 Tlaxtrelom Engine. All rights reserved.
  CONFIDENTIAL — internal use only.
  ```
- **Formato**: `cargo fmt --all`. El CI ejecuta `--check`.
- **Lints**: `cargo clippy --workspace --all-targets --all-features -- -D warnings`.
- **Sin `unwrap`/`expect`** en rutas de runtime; usa errores con contexto.
- **Sin `panic!` cruzando FFI.** Código de error, no unwind.
- **Idioma**: documentación y mensajes de usuario en español; identificadores
  en inglés cuando el ecosistema lo exija.
- **Commits**: imperativo, alcance corto. Ejemplo:
  `xtask: valida enlaces locales antes de CI`.
- **Un cambio, una intención.** No mezclar refactor masivo con feature.

---

## 3. Código por lenguaje

### Rust

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

Ese comando ejecuta fmt, clippy, C++, Rust, Python, Lua, Node, selftest y
validación de enlaces. No hay atajos: si falla en local, falla en CI.

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

## 7. Propiedad intelectual

- Todo lo producido en el repositorio pertenece a Tlaxtrelom Engine.
- No copiar código de terceros sin revisión de licencia y aprobación legal.
- Las licencias permitidas están en [deny.toml](deny.toml) y
  [Propiedad intelectual](docs/04-propiedad-intelectual.md).
- No publicar fragmentos del repositorio en foros, gists o redes.
