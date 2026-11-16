<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 2026 angiraulene1-create -->

# Windows

## Requisitos de compilación

- Rust con `rustup target add x86_64-pc-windows-msvc` y su enlace (MSVC Build
  Tools, o `rustup component add llvm-tools` y `-C linker=clang`), o bien
  compilar nativamente en un agente `windows-latest`.
- `wgpu` usa backends Vulkan y DX12: no hace falta instalar nada más para
  renderizar. La isla `native/tlaxtrelom-hal` (VLDK) y `native-geometry` sólo
  se activan con sus features.

## Portable

`tlaxtrelom-<versión>-windows-x86_64.zip`: descomprimir y ejecutar
`tlaxtrelom.cmd` (acepta los mismos argumentos que el motor:
`--dimension 3.5d`, `--xr`, `--msaa 4`, `--selftest`…). No escribe en el
registro ni fuera de su carpeta, salvo la caché de sombreadores compilados.

## Instalador

`tlaxtrelom.nsi` se incluye dentro del propio zip. Para generar el ejecutable
de instalación:

```powershell
winget install NSIS.NSIS      # o: choco install nsis
cd tlaxtrelom-<version>-windows-x86_64
makensis -DVERSION=<version> tlaxtrelom.nsi
# -> tlaxtrelom-setup-<version>.exe
```

El instalador ofrece tres secciones (motor, accesos directos, asociación
`.tlxg`) y se quita desde *Aplicaciones y características*.

`cargo xtask package --platform windows` invoca `makensis` cuando está en el
`PATH` y, cuando no, deja el guion dentro del portable y lo dice en vez de
fingir un `.exe` que no se pudo construir.
