<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 2026 angiraulene1-create -->

# Linux

## Portable

`tlaxtrelom-<versión>-linux-x86_64.tar.gz` se descomprime y se ejecuta:

```sh
tar -xzf tlaxtrelom-0.3.1-linux-x86_64.tar.gz
cd tlaxtrelom-0.3.1-linux-x86_64
./bin/tlaxtrelom.sh --dimension 3.5d
```

El guion `bin/tlaxtrelom.sh` se sitúa junto al ejecutable para que el motor
encuentre `share/tlaxtrelom/assets` y `share/tlaxtrelom/shaders` sin importar
desde dónde se lance. No toca el sistema.

## Instalador `.deb`

`tlaxtrelom_<versión>_amd64.deb` instala FHS:
`/usr/bin/tlaxtrelom`, `/usr/share/tlaxtrelom/{assets,shaders}`,
`/usr/share/applications/tlaxtrelom.desktop
` y el icono hicolor. Requiere `libvulkan1 | mesa-vulkan-drivers` y
`libasound2`.

```sh
sudo dpkg -i tlaxtrelom_0.3.1_amd64.deb   # o: sudo apt install ./tlaxtrelom_0.3.1_amd64.deb
```

El `.deb` se construye con `dpkg-deb -b` si está instalado y, si no, con
`ar` + `tar` produciendo el mismo contenedor (`debian-binary`,
`control.tar.gz`, `data.tar.xz`). `postinst`/`prerm` sólo refrescan las
cachés de escritorio cuando las herramientas existen.

## Instalador genérico (sin dpkg)

En distros sin dpkg (CachyOS/Arch, Fedora, openSUSE) se puede usar el
instalador del árbol portable:

```sh
PREFIX=/usr/local ./install.sh tlaxtrelom-0.3.1-linux-x86_64
PREFIX=/usr/local ./install.sh -u tlaxtrelom-0.3.1-linux-x86_64   # desinstalar
```

Por defecto `PREFIX=$HOME/.local`, así que no necesita root.

## Ejecutable único

`make dist` (o `cargo xtask package`) también deja `dist/tlaxtrelom`, el
binario suelto que arranca con `--demo`, `--selftest` y `--protocol` sin
necesitar los recursos empaquetados.
