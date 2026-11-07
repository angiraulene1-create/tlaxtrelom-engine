# Tlaxtrelom Engine — Propiedad intelectual y modelo interno

> **Clasificación: CONFIDENCIAL — uso interno.**
> Versión 0.1 · Este documento define cómo se protege, licencia, construye y
> distribuye el motor. No es asesoramiento legal: es la política de ingeniería
> que legal revisa y aprueba.

---

## 0. Decisión

Tlaxtrelom es un motor **propietario y de uso interno**. No es open source, no se
publica en registries públicos y no se distribuye código fuente fuera de la
organización.

| Aspecto | Decisión |
|---|---|
| Código fuente | Repositorio privado, acceso por SSO y *need to know* |
| Licencia | Propietaria, todos los derechos reservados (`LicenseRef-Tlaxtrelom-Proprietary`) |
| Distribución | Binarios firmados a equipos internos; licencia a socios solo bajo NDA |
| Contribuciones externas | Cerradas; contratistas firman cesión de IP antes del primer commit |
| Dependencias | Allowlist de licencias, SBOM y atribución automatizada |
| Publicación | Prohibida sin aprobación de Legal + Seguridad + Dirección |

**Consecuencia arquitectónica:** al no haber comunidad externa, no hay CLA, no
hay API pública estable y no hay obligación de compatibilidad. A cambio, se
asume el coste de mantener todo el *toolchain* y de auditar cada dependencia.

---

## 1. Clasificación de la información

| Nivel | Qué incluye | Controles |
|---|---|---|
| **Interno** | README, guías de build, documentación de uso | SSO, repo privado |
| **Confidencial** | Arquitectura, IR, shaders propios, diseño de gameplay | SSO + *need to know*, marca de agua en exports |
| **Restringido** | FullDive (BCI), modelos de calibración, datos de usuarios | Cifrado en reposo, auditoría de acceso, sin export |
| **Secreto** | Claves de firma, secretos de CI, credenciales de SDKs | Secret manager, nunca en el repo, rotación |

Todo archivo nuevo lleva cabecera:

```text
SPDX-License-Identifier: LicenseRef-Tlaxtrelom-Proprietary
Copyright (c) 2026 <Compañía>. All rights reserved.
CONFIDENTIAL — internal use only.
```

`SPDX-License-Identifier` permite que los escáneres (ScanCode, FOSSA,
`cargo-deny`) distingan lo propio de lo third-party sin heurísticas.

---

## 2. Titularidad

1. **Empleados:** contrato de trabajo con cláusula de cesión de derechos de
   explotación y de invenciones (patentes). Revisar legislación local: en
   España, por ejemplo, el art. 51 LPI y el régimen de invenciones laborales.
2. **Contratistas y estudios externos:** acuerdo de cesión **firmado antes** del
   acceso al repo. Sin cesión no hay commit: el control es técnico (no se les da
   permiso de escritura hasta que Legal confirma).
3. **Prácticas/becarios:** mismo régimen que contratistas.
4. **Contribuciones de IA:** el código generado por asistentes se revisa como
   cualquier otro; se prohíbe pegar fragmentos de fuentes con licencia
   incompatible o sin procedencia. El *prompt* y el output no se consideran
   automáticamente propios: hay que verificar similitud y licencia.
5. **Derechos morales:** no se ceden en algunas jurisdicciones; el contrato
   incluye renuncia en la medida permitida y la política interna es no atribuir
   autoría individual en el producto.

**Registro de IP:** un `NOTICE` interno y un inventario de componentes con
autor, licencia y obligaciones. Se actualiza en cada release, no cada año.

---

## 3. Política de dependencias

### 3.1 Allowlist

| Licencia | Estado | Condición |
|---|---|---|
| MIT, ISC, BSD-2/3, Zlib, Unlicense, CC0 | ✅ Permitida | Atribución en `NOTICE` |
| Apache-2.0 | ✅ Preferida | Incluye concesión de patentes; cláusula de represalia |
| MIT OR Apache-2.0 (dual) | ✅ Preferida | Elegir Apache-2.0 para la concesión de patentes |
| MPL-2.0 | ⚠️ Con revisión | *Copyleft* por archivo: se permite si no se modifica el archivo |
| LGPL-2.1/3.0 | ⚠️ Solo enlace dinámico | Aprobación de Legal; no *static link*; permitir sustitución |
| EPL-2.0 | ⚠️ Con revisión | Aislar en proceso/herramienta, no en runtime |
| GPL-2.0/3.0, AGPL | ❌ Prohibida | Contamina el producto |
| SSPL, BUSL, Commons Clause | ❌ Prohibida | Restricciones de uso comercial |
| Sin licencia / "propietaria" | ❌ Prohibida | Sin permiso escrito del titular |

### 3.2 Dependencias actuales (verificar en CI, no a mano)

| Componente | Licencia esperada | Nota |
|---|---|---|
| `wgpu`, `naga` | MIT OR Apache-2.0 | Backend gráfico |
| `winit` | Apache-2.0 | Ventanas/event loop |
| `triple_buffer`, `rtrb` | MIT OR Apache-2.0 | IPC *wait-free* |
| `anyhow`, `bytemuck`, `pollster` | MIT OR Apache-2.0 | Utilidades |
| `libc` | MIT OR Apache-2.0 | Solo Linux |
| `zstd` (libzstd) | BSD-3-Clause **OR** GPL-2.0 | **Elegir BSD** explícitamente |
| CMake / GCC / Clang | BSD/Apache/GPL con excepción de runtime | Herramientas, no se distribuyen |

> `zstd` es el caso clásico de licencia dual: si se elige GPL por accidente, el
> producto entero queda contaminado. `deny.toml` fija la elección y el build de
> CMake la documenta.

### 3.3 Datasets, modelos y assets

El código no es el único riesgo de licencia:

- **Datasets de investigación** (p. ej. BCI Competition IV 2a/2b) suelen tener
  licencia **solo académica**. Se usan para validar el decoder offline; para
  entrenar un modelo que se distribuya hace falta permiso escrito o un dataset
  propio.
- **Modelos preentrenados** (pesos) tienen su propia licencia, distinta de la
  del código que los produce. Un modelo "open weights" puede prohibir uso
  comercial.
- **Assets** (texturas, audio, fuentes, mocap) necesitan cadena de custodia
  documentada; una fuente de sistema no es licenciable para embeber.
- **SDKs de middleware** (audio, física, haptics): licencia por asiento,
  plataforma y a veces por título. Se aíslan en adaptadores y no se mezclan con
  el core.

### 3.4 Controles automáticos
```text
CI (pull request)
  ├── cargo deny check licenses bans advisories sources
  ├── cargo about generate notice   → NOTICE del release
  ├── pip-audit / safety            → tools Python
  ├── ScanCode (C++ y vendor)       → fragmentos con licencia desconocida
  ├── gitleaks                      → secretos
  └── SBOM CycloneDX + firma cosign → artefacto
```

Reglas:

- **Sin excepciones silenciosas.** Una excepción se declara en `deny.toml` con
  justificación, autor y fecha de revisión.
- **Sin descargas arbitrarias.** El CI usa el espejo interno; `Cargo.lock` y
  `CMakePresets.json` están pinneados.
- **Sin `git submodule` sin auditoría.** Un submódulo es código de terceros con
  su propia licencia y su propio historial.

---

## 4. Distribución interna

### 4.1 Repositorios y registries

- GitHub/GitLab self-hosted o Enterprise Cloud con SSO, 2FA obligatorio,
  *branch protection* y *signed commits*.
- **Espejo interno** de crates.io y de paquetes del sistema (Artifactory/Nexus).
  El CI no tiene salida directa a Internet.
- `publish = false` en todos los crates del workspace. Publicar requiere un
  cambio explícito y aprobación; no debe ser un accidente de `cargo publish`.

### 4.2 Artefactos

| Artefacto | Firma | Verificación |
|---|---|---|
| Binario del editor | `cosign`/minisign + hash SHA-256 | El lanzador verifica antes de ejecutar |
| Runtime de juego | Firma + *provenance* SLSA nivel 3 | Solo builds de CI, nunca de una estación |
| `.tlaxtrelompack` | Ed25519 (DLC/mods internos) | El runtime rechaza paquetes sin firma en modo *retail* |
| SBOM | CycloneDX adjunto al release | Inventario consultable por Legal/Seguridad |

- **Reproducibilidad:** mismo commit + mismo toolchain + mismo lockfile ⇒ mismo
  hash. Sin esto, la firma no prueba nada.
- **Air-gapped:** el build debe poder correr sin red una vez poblado el espejo.
- **Trazabilidad:** cada binario embebe `git describe`, versión de HIR y hash
  del SBOM.

### 4.3 Licenciamiento a socios (si ocurre)

- Contrato de licencia binaria, NDA, auditoría de uso y cláusula de auditoría.
- Entrega de headers C **solo** si el socio escribe plugins; nunca del runtime
  completo.
- Marca de agua por licensee en builds de prensa/socios para detectar filtraciones.
- Prohibición de *reverse engineering* en la medida permitida por ley (y
  advertencia de que en la UE la interoperabilidad tiene límites legales).

---

## 5. Secretos y cadena de suministro

1. **Nunca** claves, tokens ni certificados en el repo, en un `.env` commiteado
   ni en logs. `gitleaks` en *pre-commit* y en CI.
2. Secretos en Vault/KMS/Secret Manager; el CI los inyecta en tiempo de build y
   los borra al terminar.
3. Rotación trimestral de claves de firma; claves en HSM o *hardware token*.
4. Dependencias: *pinning* + `cargo vet` para crates críticos + revisión manual
   de cualquier `build.rs` nuevo (un `build.rs` es ejecución arbitraria en el CI).
5. **`unsafe` y FFI** revisados por dos personas; el código que cruza la ABI se
   considera superficie de ataque.
6. Modelo de amenaza: insider, dependencia comprometida, filtración de build,
   y exfiltración de datos BCI. Cada uno tiene control y dueño.

---

## 6. FullDive: datos personales y regulación

Las señales EEG/EMG son **datos personales** y, en muchos regímenes, datos de
categoría especial (salud/biometría). El diseño ya minimiza: procesamiento
on-device, crudo retenido 0 s, perfil cifrado.

Reglas adicionales por ser interno:

- **Base legal:** consentimiento explícito por sesión + evaluación de impacto
  (DPIA) antes de pilotos con empleados.
- **Works council / comité de empresa:** obligatorio informar y negociar si se
  prueba con plantilla en la UE.
- **Prohibición de uso evaluativo:** los datos neuronales no se usan para
  evaluar desempeño, salud o aptitud. Se documenta y se audita.
- **Retención:** crudo 0 s; features 24 h con consentimiento; modelos agregados
  sin identificadores.
- **Acceso:** *need to know* + registro de auditoría; ni el equipo de gameplay
  ve waveforms.
- **Si se comercializa como dispositivo médico:** IEC 62304, ISO 14971,
  IEC 60601-1, y el régimen de producto sanitario. Si es *wellness*, disclaimers
  claros pero el diseño de seguridad se mantiene.
- **Export control:** el hardware BCI y la criptografía pueden estar sujetos a
  EAR/EU dual-use. La criptografía estándar (XChaCha20, TLS) suele caer en la
  excepción *mass-market*, pero se revisa antes de exportar hardware.

---

## 7. Upstream y comunidad (política restrictiva)

No somos open source, pero usamos open source. Eso implica:

- **Consumir** es libre dentro de la licencia (con atribución).
- **Modificar** una dependencia exige registrar el *fork* (repo privado,
  `vendor/`), mantener el aviso de licencia y no mezclar código propietario en
  el mismo archivo si la licencia es MPL/LGPL.
- **Devolver parches** (*upstreaming*) requiere aprobación de Legal: un parche
  puede revelar arquitectura. Regla por defecto: se devuelven solo *bugfixes*
  genéricos, nunca código que describa el diseño del motor.
- **No** se aceptan PRs externos ni se publica un roadmap.
- **Publicación defensiva** (opcional): si se quiere evitar que un tercero
  patente una técnica, se publica un *technical disclosure* sin código. Decisión
  de Dirección + Legal, no de ingeniería.

---

## 8. Marcas y nombres

- "Tlaxtrelom Engine" es un **nombre en clave interno**. Antes de uso externo:
  búsqueda de marcas (EUIPO/USPTO), dominios y handles.
- El nombre definitivo y el logo se aprueban por Marketing + Legal.
- No se usa el nombre de un middleware de terceros para describir el motor
  ("compatible con X" requiere revisar la política de marca del proveedor).

---

## 9. Checklist de release interno

```text
[ ] Código con cabecera SPDX propia en todos los archivos nuevos
[ ] cargo deny + ScanCode + gitleaks en verde
[ ] NOTICE generado y revisado (atribuciones correctas)
[ ] SBOM CycloneDX adjunto y firmado
[ ] Build reproducible desde commit limpio en CI
[ ] Binario firmado; hash publicado en el canal interno
[ ] Sin símbolos de debug ni rutas absolutas del desarrollador
[ ] Sin secretos en el binario (strings/entropía)
[ ] Revisión de licencias de assets (texturas, audio, fuentes) — no solo código
[ ] Si toca FullDive: DPIA vigente y consentimiento documentado
[ ] Aprobación de Legal si hay nueva dependencia o nuevo socio
```

---

## 10. Riesgos y controles

| Riesgo | Impacto | Control |
|---|---|---|
| Dependencia GPL entra por un `Cargo.lock` | Contaminación legal del producto | `deny.toml` + revisión de PR |
| `build.rs` malicioso en una dependencia | Ejecución en CI, robo de secretos | `cargo vet`, espejo, CI sin secretos de producción |
| Filtración de código por un contratista | Pérdida de ventaja competitiva | Cesión previa, permisos mínimos, marca de agua |
| Filtración de build a socios | Piratería / ingeniería inversa | Firma, watermark por licensee, auditoría |
| Datos BCI usados para evaluación | Sanción legal y reputacional | Prohibición explícita + DPIA + auditoría |
| Pérdida de claves de firma | Suplantación de builds | HSM, doble custodia, rotación |
| Dependencia abandonada con CVE | Vulnerabilidad sin parche | `cargo deny advisories`, plan de *fork* interno |
| IA genera código con licencia incompatible | Reclamo de terceros | Revisión + escaneo de similitud + política de prompts |

---

## 11. Resumen para ingeniería

1. Todo archivo nuevo: cabecera SPDX propietaria.
2. Toda dependencia nueva: allowlist + `deny.toml` + `NOTICE`.
3. Nada de secretos en el repo; el CI los inyecta.
4. Nada de `cargo publish`; el workspace es privado.
5. Todo binario sale firmado y reproducible del CI, nunca de una laptop.
6. FullDive: datos de personas, no *telemetry*. Consentimiento, minimización,
   cifrado y prohibición de uso evaluativo.
7. Ante la duda legal: se pregunta **antes** del merge, no después del release.
