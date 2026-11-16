<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 2026 angiraulene1-create -->

# Empaquetado

`cargo xtask package` construye, ordena y archiva los artefactos. Este directorio
sólo contiene las **definiciones** que el empaquetador copia o invoca; no hay
que editarlas a mano en el flujo normal.

```text
packaging/linux/       instalador .deb + arranque portable + entrada de menú
packaging/windows/     guion NSIS (setup.exe) + arranque portable .cmd
packaging/harmonyos/   proyecto HAP (ArkTS + ability) y arranque por hdc
```

## Qué se produce por plataforma

| Plataforma | Portable | Instalador |
| --- | --- | --- |
| Linux (`x86_64-unknown-linux-gnu`) | `tlaxtrelom-<ver>-linux-x86_64.tar.gz` | `.deb` (dpkg/ar) + `install.sh` |
| Windows (`x86_64-pc-windows-msvc`) | `tlaxtrelom-<ver>-windows-x86_64.zip` | `tlaxtrelom-setup-<ver>.exe` (NSIS) |
| HarmonyOS for PC (`aarch64-unknown-linux-ohos`) | `tlaxtrelom-<ver>-harmonyos-aarch64.zip` | proyecto `.hap` + `install-harmonyos.sh` (`hdc`) |

Todo sale en `dist/`, que está en `.gitignore`.

## Degradación honesta

El empaquetador **no finge** lo que la máquina no puede hacer:

- Sin `makensis` no se genera el `.exe` de instalación de Windows: se emite el
  guion `tlaxtrelom.nsi` dentro del portable y se avisa cómo correrlo
  (`makensis tlaxtrelom.nsi`). NSIS viene con `choco install nsis`.
- Sin `dpkg-deb` el `.deb` se arma con `ar` + `tar` (es el mismo contenedor
  `debian-binary` / `control.tar.gz` / `data.tar.xz`); si tampoco hay `ar`, se
  deja el árbol `deb/` listo para `dpkg-deb -b`.
- El `.hap` de HarmonyOS necesita el SDK de DevEco (hvigor + `hap-sign-tool`),
  que no existe en el repositorio ni en CI. `xtask package --platform harmonyos`
  compila el puente C++ del NDK si el NDK está disponible, monta el proyecto
  HAP con la biblioteca dentro y **valida** los manifiestos; el empaquetado
  final del `.hap` y su firma son un paso de DevEco. `install-harmonyos.sh`
  instala el resultado con `hdc`.
- Cruzar a Windows u OHOS exige `rustup target add <triple>` y su enlace
  (`x86_64-w64-mingw32-clang` / NDK de OHOS). Si faltan, `xtask package` falla
  con el comando exacto que falta en lugar de emitir un artefacto roto.

## Comandos

```sh
cargo xtask package                    # plataforma del host
cargo xtask package --platform windows # portable Windows (requiere target)
cargo xtask package --all              # linux + windows + harmonyos
cargo xtask package --skip-build       # reempaqueta target/release
cargo xtask package --debug            # usa target/debug (pruebas rápidas)
cargo xtask package-check              # valida definiciones sin compilar
```

`xtask ci` corre `package-check` siempre, y `.github/workflows/release.yml`
corre `package` en las tres plataformas y sube los artefactos.
