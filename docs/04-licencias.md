<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 2026 angiraulene1-create -->

# Tlaxtrelom Engine — Licencias, propiedad y cadena de suministro

> **Versión 0.2 · El motor está licenciado bajo MIT.**
> Este documento define cómo se aplica esa licencia al código propio, cómo se
> auditan las dependencias de terceros y qué se exige antes de publicar un
> artefacto.

---

## 0. Modelo de licenciamiento

Tlaxtrelom Engine es software de código abierto bajo los términos de la
**[Licencia MIT](../LICENSE)**. Cualquier persona puede usar, copiar, modificar,
fusionar, publicar, distribuir, sublicenciar y vender el código y sus copias,
conforme a esa licencia.

| Aspecto | Decisión |
|---|---|
| Código fuente | Repositorio público (`angiraulene1-create/tlaxtrelom-engine`) |
| Licencia principal | MIT (`LICENSE`), expresada como `SPDX-License-Identifier: MIT` |
| Copyright | `Copyright (c) 2026 angiraulene1-create` |
| Distribución | Código fuente abierto; binarios firmados y reproducibles desde CI |
| Contribuciones externas | Abiertas por Pull Request, con DCO firmada — ver [CONTRIBUTING.md](../CONTRIBUTING.md) |
| Dependencias | Allowlist de licencias permisivas validada por `cargo-deny` |

MIT es *permisiva*: no impone reciprocidad, pero **obliga a conservar el aviso
de copyright y el de licencia** en todas las copias o partes sustanciales del
software. Ese aviso es el que llevan las cabeceras SPDX de cada archivo.

**Lo que MIT no concede:** la licencia del código no transfiere derechos sobre
el nombre, el logo ni otras marcas del proyecto (§ 8), ni autoriza implicar que
un derivado respalda o proviene del proyecto original.

**Lo que MIT no cubre:** el hardware BCI, sus esquemas y sus firmwares son
artefactos físicos con régimen propio (§ 6). Que el código sea abierto no
convierte un casco neural en un producto garantizado; el aviso de ausencia de
garantía de la licencia se lee junto con las advertencias de seguridad de
[SECURITY.md](../SECURITY.md).

---

## 1. Aviso de licencia en cada archivo

Todo archivo nuevo lleva cabecera SPDX en el primer comentario posible:

```text
SPDX-License-Identifier: MIT
Copyright (c) 2026 angiraulene1-create
```

Con el estilo de comentario del lenguaje: `//` en Rust/C++/Zig/GLSL/WGSL/TS/Go,
`#` en Python/shell/TOML/YAML/Makefile, `--` en Lua, `/* … */` en headers C y
`<!-- … -->` en Markdown. En GLSL la cabecera va **después** de `#version`, que
es la única línea que puede abrir el archivo.

`SPDX-License-Identifier` permite que ScanCode, FOSSA o `cargo-deny` distingan
lo propio de lo de terceros sin heurísticas, y que `cargo about` genere el
`NOTICE` sin revisar archivo por archivo.

> Un identificador SPDX mal escrito no invalida la licencia, pero sí rompe el
> escáner y el `NOTICE`: la cabecera es un contrato mecánico, no un comentario.

---

## 2. Titularidad del copyright

1. **Autor principal:** el copyright del código histórico y del que se produzca
   bajo MIT corresponde a `angiraulene1-create`, que es quien publica la
   licencia.
2. **Contribuciones externas:** se licencian bajo MIT por el propio acto del PR
   (la licencia del repositorio es la licencia de la contribución). Se pide
   sign-off `Signed-off-by:` (DCO 1.1) como declaración de que quien envía el
   parche tiene derecho a licenciarlo así. **No se exige cesión de derechos ni
   CLA**: una licencia permisiva no la necesita.
3. **Colaboradores por contrato o prácticas:** igual que los externos; lo que se
   verifica es que tengan derecho a licenciar el aporte, no que lo cedan.
4. **Contribuciones asistidas por IA:** se revisan como cualquier otro parche.
   Está prohibido aportar fragmentos copiados de fuentes con licencia
   incompatible o de procedencia desconocida; un modelo no puede otorgar una
   licencia que no tiene.
5. **Derechos morales:** no se ceden en varias jurisdicciones (en España, art.
   14 LPI). MIT no los afecta: la atribución adicional queda en el
   `Copyright` de la cabecera y en `NOTICE`.

**Registro de autoría:** `NOTICE` con atribuciones propias y de terceros, e
inventario de componentes con licencia y obligaciones. Se regenera en cada
release, no «cada año a ver si acaso».

---

## 3. Política de dependencias

MIT no cambia esta parte: la licencia propia es permisiva, pero una dependencia
restrictiva sigue contaminando el producto que la enlaza.

### 3.1 Allowlist

| Licencia | Estado | Condición |
|---|---|---|
| MIT, ISC, BSD-2/3, Zlib, Unlicense, CC0 | ✅ Permitida | Atribución en `NOTICE` |
| Apache-2.0 | ✅ Preferida | Incluye concesión de patentes; cláusula de represalia |
| MIT OR Apache-2.0 (dual) | ✅ Preferida | Elegir Apache-2.0 por la concesión de patentes |
| MPL-2.0 | ⚠️ Con revisión | *Copyleft* por archivo: se permite si no se modifica el archivo |
| LGPL-2.1/3.0 | ⚠️ Solo enlace dinámico | Aprobación explícita; no *static link*; permitir sustitución |
| EPL-2.0 | ⚠️ Con revisión | Aislar en herramienta o proceso, no en runtime |
| GPL-2.0/3.0, AGPL | ❌ Prohibida en runtime | Incompatibles con un producto abierto permisivo sin decidirlo conscientemente |
| SSPL, BUSL, Commons Clause | ❌ Prohibida | Restricciones de uso comercial |
| Sin licencia / "propietaria" | ❌ Prohibida | Sin permiso escrito del titular |

MIT es compatible con GPL *hacia adelante* (un derivado GPL puede incluir código
MIT), pero una dependencia GPL **impondría GPL al binario resultante**, lo cual
es una decisión de proyecto, no un detalle legal. Por eso la allowlist sigue
cerrada.

### 3.2 Dependencias actuales (verificar en CI, no a mano)

| Componente | Licencia esperada | Nota |
|---|---|---|
| `wgpu`, `naga` | MIT OR Apache-2.0 | Backend gráfico |
| `winit` | Apache-2.0 | Ventanas/event loop |
| `triple_buffer`, `rtrb` | MIT OR Apache-2.0 | IPC *wait-free* |
| `anyhow`, `bytemuck`, `pollster` | MIT OR Apache-2.0 | Utilidades |
| `libc` | MIT OR Apache-2.0 | Solo Linux |
| `cc` (build-dependency) | MIT OR Apache-2.0 | Enlace de la HAL C11 |
| `zstd` (libzstd) | BSD-3-Clause **OR** GPL-2.0 | **Elegir BSD** explícitamente |
| CMake / GCC / Clang | BSD/Apache/GPL con excepción de runtime | Herramientas, no se distribuyen |

> `zstd` es el caso clásico de licencia dual: si se elige GPL por accidente, el
> producto entero queda contaminado. `deny.toml` fija la elección y el build de
> CMake la documenta.

### 3.3 Datasets, modelos y assets

El código no es el único riesgo de licencia:

- **Datasets de investigación** (p. ej. BCI Competition IV 2a/2b) suelen tener
  licencia **solo académica**. Sirven para validar el decoder offline; para
  entrenar un modelo distribuido hace falta permiso escrito o dataset propio.
- **Modelos preentrenados** (pesos) tienen licencia propia, distinta de la del
  código que los produce. Un modelo "open weights" puede prohibir uso comercial.
- **Assets** (texturas, audio, fuentes, mocap) necesitan cadena de custodia
  documentada; una fuente de sistema no es licenciable para embeber. El MIT del
  repositorio **no** re-licencia assets de terceros: si un asset no es propio,
  su licencia se declara junto al archivo y se lista en `NOTICE`.
- **SDKs de middleware** (audio, física, haptics): licencia por asiento,
  plataforma y a veces por título. Se aíslan en adaptadores y no se mezclan con
  el core; un adaptador GPL-licensed no puede entrar en el runtime.

### 3.4 Controles automáticos

```text
CI (pull request)
  ├── cargo deny check licenses bans advisories sources
  ├── cargo about generate notice        → NOTICE del release
  ├── escaneo de cabeceras SPDX          → todo archivo fuente declara MIT
  ├── pip-audit                          → tools Python
  ├── ScanCode (islas C++ y vendor)      → fragmentos con licencia desconocida
  ├── gitleaks                           → secretos
  └── SBOM CycloneDX + firma cosign      → artefacto
```

Reglas:

- **Sin excepciones silenciosas.** Una excepción se declara en `deny.toml` con
  justificación, autor y fecha de revisión.
- **Sin descargas arbitrarias.** `Cargo.lock` y los presets de CMake están
  pinneados; CI instala dependencias solo desde el registry declarado.
- **Sin `git submodule` sin auditoría.** Un submódulo es código de terceros con
  su propia licencia y su propio historial.

---

## 4. Distribución de artefactos

### 4.1 Repositorio y registries

- Repositorio público con SSO, 2FA, *branch protection* y commits firmados.
- `publish = false` en los crates del workspace y `"private": true` en el
  paquete Node. **Ser abierto no significa publicarse en todos los registries**:
  publicar es una decisión explícita, no un accidente de `cargo publish`.
  Quien quiera el paquete desde crates.io o npm debe pedirlo con un cambio
  consciente en esos manifiestos.

### 4.2 Artefactos

| Artefacto | Firma | Verificación |
|---|---|---|
| Binario del editor/host | `cosign`/minisign + SHA-256 | El lanzador verifica antes de ejecutar |
| Runtime | Firma + *provenance* SLSA nivel 3 | Solo builds de CI, nunca de una estación |
| `.tlaxtrelompack` | Ed25519 (DLC/mods) | El runtime rechaza paquetes sin firma en modo *retail* |
| SBOM | CycloneDX adjunto al release | Inventario consultable |

- **Reproducibilidad:** mismo commit + mismo toolchain + mismo lockfile ⇒ mismo
  hash. Sin esto, la firma no prueba nada.
- **Sin red:** el build debe poder correr una vez poblado el caché.
- **Trazabilidad:** cada binario embebe `git describe`, versión de HIR y hash
  del SBOM.

### 4.3 Distribución cerrada sobre código abierto

Un tercero puede tomar el código, compilarlo y vender un binario: MIT lo
permite, y el *source-available* con cláusulas restrictivas (SSPL, BUSL) se
rechaza precisamente para no depender de ese modelo. Lo que **no** cede MIT:

- el nombre y el logo (§ 8);
- la marca de que el derivado es "Tlaxtrelom Engine" sin avisar de sus cambios;
- ningún dato de usuario: quien distribuye un producto con FullDive asume sus
  propias obligaciones regulatorias (§ 6).

Si a un contribuidor le conviene un régimen distinto para su parte, esa parte
debe estar en un componente separado y declararlo en su propio manifiesto. No se
mezclan licencias dentro de un mismo archivo.

---

## 5. Secretos y cadena de suministro

Que el repositorio sea público no relaja nada de esto: al contrario, un repo
abierto filtra secretos más rápido porque cualquiera lo clona.

1. **Nunca** claves, tokens ni certificados en el repo, en un `.env` commiteado
   ni en logs. `gitleaks` en *pre-commit* y en CI.
2. Secretos de CI en el *secret manager* de la plataforma; inyectados en tiempo
   de build y borrados al terminar.
3. Rotación de claves de firma; clave de release en HSM o *hardware token* con
   doble custodia.
4. Dependencias: *pinning* + `cargo vet` para crates críticos + revisión manual
   de cualquier `build.rs` nuevo (un `build.rs` es ejecución arbitraria en el CI).
5. **`unsafe` y FFI** revisados por dos personas; el código que cruza la ABI se
   considera superficie de ataque.
6. Modelo de amenaza: dependencia comprometida, *force push* a una tag,
   filtración de la clave de firma, exfiltración de datos BCI. Cada uno tiene
   control y dueño.

---

## 6. FullDive: datos personales y regulación

Las señales EEG/EMG son **datos personales** y, en muchos regímenes, datos de
categoría especial (salud/biometría). El diseño ya minimiza: procesamiento
*on-device*, crudo retenido 0 s, perfil cifrado (XChaCha20-Poly1305).

MIT licencia el código; **no** licencia el tratamiento de datos de nadie:

- **Base legal:** consentimiento explícito por sesión + evaluación de impacto
  (DPIA) antes de probar con personas.
- **Comité de empresa / works council:** obligatorio informar y negociar si se
  prueba con plantilla en la UE.
- **Prohibición de uso evaluativo:** los datos neuronales no se usan para
  evaluar desempeño, salud ni aptitud. Se documenta y se audita.
- **Retención:** crudo 0 s; *features* 24 h con consentimiento; modelos
  agregados sin identificadores.
- **Acceso:** *need to know* + registro de auditoría; ni el equipo de gameplay
  ve *waveforms*.
- **Si se comercializa como dispositivo médico:** IEC 62304, ISO 14971,
  IEC 60601-1 y el régimen de producto sanitario. Si es *wellness*, disclaimers
  claros, pero el diseño de seguridad se mantiene.
- **Export control:** el hardware BCI y la criptografía pueden estar sujetos a
  EAR/doble uso UE. La criptografía estándar (XChaCha20, TLS) suele caer en la
  excepción *mass-market*, pero se revisa antes de exportar hardware.
- **Responsabilidad:** la cláusula de ausencia de garantía de MIT protege al
  licenciante del código, no al fabricante del estimulador. Quien construya
  hardware sobre este diseño asume la certificación aplicable.

---

## 7. Upstream y comunidad

Ahora somos open source y usamos open source:

- **Consumir** es libre dentro de la licencia, con atribución en `NOTICE`.
- **Modificar** una dependencia exige registrar el *fork* (`vendor/`), mantener
  el aviso de licencia y no mezclar código con licencia incompatible en el mismo
  archivo si la dependencia es MPL/LGPL.
- **Devolver parches** (*upstreaming*) es la norma, no la excepción: un *bugfix*
  genérico se envía arriba. Un parche que revele una técnica concreta se revisa
  antes de publicarlo, porque publicarlo renuncia a patentarlo.
- **Se aceptan PRs externos** según [CONTRIBUTING.md](../CONTRIBUTING.md), con
  DCO y la matriz de CI en verde.
- **Publicación defensiva** (opcional): si se quiere evitar que un tercero
  patente una técnica, se publica un *technical disclosure* en la documentación.
  Es decisión de mantenimiento, no de un contribuidor de paso.

---

## 8. Marcas y nombres

- MIT **no** concede derechos de marca. "Tlaxtrelom Engine" identifica a este
  proyecto; un derivado debe dejar claro que no es el original y no debe
  presentarse como respaldado por él.
- Antes de cualquier uso comercial externo del nombre: búsqueda de marcas
  (EUIPO/USPTO), dominios y handles.
- No se usa el nombre de un middleware de terceros para describir el motor:
  "compatible con X" exige revisar la política de marca del proveedor.

---

## 9. Checklist de release

```text
[ ] Código con cabecera SPDX MIT en todos los archivos fuente
[ ] cargo deny + ScanCode + gitleaks en verde
[ ] NOTICE regenerado y revisado (atribuciones correctas)
[ ] SBOM CycloneDX adjunto y firmado
[ ] Build reproducible desde commit limpio en CI
[ ] Binario firmado; hash publicado junto al release
[ ] Sin símbolos de debug ni rutas absolutas del desarrollador
[ ] Sin secretos en el binario (strings/entropía)
[ ] Revisión de licencias de assets (texturas, audio, fuentes) — no solo código
[ ] Si toca FullDive: DPIA vigente y consentimiento documentado
[ ] CHANGELOG y documentación actualizados en el mismo release
```

---

## 10. Riesgos y controles

| Riesgo | Impacto | Control |
|---|---|---|
| Dependencia GPL entra por un `Cargo.lock` | Cambia el régimen legal del binario | `deny.toml` + revisión de PR |
| `build.rs` malicioso en una dependencia | Ejecución en CI, robo de secretos | `cargo vet`, lockfile, CI sin secretos de producción |
| Secreto commiteado en un repo público | Exposición inmediata e irreversible | `gitleaks` + rotación + historia limpia |
| Clave de firma comprometida | Suplantación de builds | HSM, doble custodia, rotación |
| Dependencia abandonada con CVE | Vulnerabilidad sin parche | `cargo deny advisories`, plan de *fork* |
| Fork que confunde al usuario | Daño reputacional | Política de marca (§ 8), aviso en README |
| Contribución con licencia incompatible | Reclamo de terceros | DCO + escaneo de similitud + revisión |
| Datos BCI usados para evaluación | Sanción legal y reputacional | Prohibición explícita + DPIA + auditoría |
| IA genera código copiado con otra licencia | Reclamo de terceros | Revisión humana + escaneo + política de prompts |

---

## 11. Resumen para ingeniería

1. Todo archivo nuevo: `SPDX-License-Identifier: MIT` y el aviso de copyright.
2. Toda dependencia nueva: allowlist + `deny.toml` + `NOTICE`.
3. Nada de secretos en el repo; el CI los inyecta.
4. Nada de `cargo publish`/`npm publish` sin una decisión explícita.
5. Todo binario sale firmado y reproducible del CI, nunca de una laptop.
6. FullDive: son datos de personas, no *telemetry*. Consentimiento,
   minimización, cifrado y prohibición de uso evaluativo.
7. Las marcas no se ceden con MIT: un derivado se presenta como derivado.
8. Ante la duda legal: se pregunta **antes** del merge, no después del release.
