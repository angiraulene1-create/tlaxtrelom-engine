# Política de seguridad — Tlaxtrelom Engine

> **PROPIETARIO Y CONFIDENCIAL.** No abrir incidencias de seguridad en foros
> públicos, redes sociales ni trackers externos.

---

## 1. Versiones soportadas

| Versión | Soporte | Notas |
|---|---|---|
| `0.2.x` | Activo | runtime, HAL, geometría, tooling |
| `0.1.x` | Solo lectura | sin parches salvo riesgo crítico |

Una versión deja de recibir parches cuando se publica la siguiente minor y se
confirma la migración interna.

---

## 2. Cómo reportar

1. Usa el **canal interno de seguridad** de la organización (gestor de
   incidencias restringido o contacto directo del responsable de seguridad).
2. Incluye:
   - versión y commit exactos;
   - plataforma, driver y hardware;
   - pasos mínimos de reproducción;
   - impacto esperado (corrupción, fuga, DoS, seguridad física);
   - si el fallo afecta a FullDive o a un actuador, márcalo como **crítico**.
3. No publiques detalles hasta que el equipo confirme el parche y la ventana de
   divulgación.

No hay recompensas públicas ni programa de bug bounty externo.

---

## 3. Alcance

### En alcance

- Runtime Rust: memoria, FFI, recuperación de superficie, gate y watchdog.
- Isla C++: parsing de assets, descompresión, contratos de ownership.
- HAL C11: afinidad, memoria bloqueada, hashing.
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

- Confirmación interna en un plazo objetivo de 2 días hábiles.
- Evaluación y mitigación según severidad; los críticos tienen prioridad.
- Se documenta el fallo y el parche en el changelog interno cuando la
  mitigación está desplegada.
- No se publica código fuente ni detalles fuera de la organización.

---

## 6. Buenas prácticas para contribuidores

- No incluir secretos, claves, tokens ni datos personales en commits, tests,
  fixtures o logs.
- No desactivar el sandbox Lua ni el gate para "probar rápido".
- No añadir dependencias fuera de la allowlist de `deny.toml` sin revisión.
- Ejecutar `cargo xtask ci` antes de pedir revisión.
- Reportar cualquier `unsafe` nuevo con su invariante y su test.
