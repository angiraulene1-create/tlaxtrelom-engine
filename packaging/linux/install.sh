#!/bin/sh
# SPDX-License-Identifier: MIT
# Copyright (c) 2026 angiraulene1-create
# Instalador portable para cualquier distribucion con glibc (sin dpkg).
# Uso: PREFIX=/usr/local ./install.sh <dir-desempaquetado>
set -eu

usage() {
  echo "uso: $0 [-u|--uninstall] <dir-del-paquete>" >&2
  exit 2
}

PREFIX="${PREFIX:-$HOME/.local}"
UNINSTALL=0
SRC=""

while [ $# -gt 0 ]; do
  case "$1" in
    -u|--uninstall) UNINSTALL=1 ;;
    -h|--help) usage ;;
    *) SRC="$1" ;;
  esac
  shift
done

[ -n "$SRC" ] || usage
[ -d "$SRC/bin" ] && [ -d "$SRC/share/tlaxtrelom/assets" ] || {
  echo "$SRC no tiene la estructura del paquete portable (bin/ + share/tlaxtrelom/)" >&2
  exit 1
}

if [ "$UNINSTALL" = 1 ]; then
  rm -f "$PREFIX/bin/tlaxtrelom" \
        "$PREFIX/bin/tlaxtrelom.sh" \
        "$PREFIX/share/applications/tlaxtrelom.desktop" \
        "$PREFIX/share/icons/hicolor/scalable/apps/tlaxtrelom.svg" \
        "$PREFIX/share/metainfo/org.tlaxtrelom.Engine.metainfo.xml"
  rm -rf "$PREFIX/share/tlaxtrelom"
  echo "desinstalado de $PREFIX"
  exit 0
fi

mkdir -p "$PREFIX/bin" "$PREFIX/share"
# El arbol portable ya trae la jerarquia FHS (share/tlaxtrelom, applications,
# icons, metainfo): se copia entera, no carpeta por carpeta.
cp -Rf "$SRC/share/." "$PREFIX/share/"
cp -f "$SRC/bin/tlaxtrelom" "$PREFIX/bin/tlaxtrelom"
cp -f "$SRC/bin/tlaxtrelom.sh" "$PREFIX/bin/tlaxtrelom.sh"
chmod 755 "$PREFIX/bin/tlaxtrelom" "$PREFIX/bin/tlaxtrelom.sh"
command -v update-desktop-database >/dev/null 2>&1 && \
  update-desktop-database "$PREFIX/share/applications" || true

echo "instalado en $PREFIX/bin/tlaxtrelom"
case ":$PATH:" in
  *":$PREFIX/bin:"*) ;;
  *) echo "anade $PREFIX/bin al PATH para lanzar 'tlaxtrelom' directamente" ;;
esac
