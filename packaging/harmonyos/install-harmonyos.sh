#!/bin/sh
# SPDX-License-Identifier: MIT
# Copyright (c) 2026 angiraulene1-create
# Vuelca el arbol nativo de Tlaxtrelom a HarmonyOS for PC por hdc y comprueba
# que el motor arranca en el dispositivo. No sustituye al HAP firmado: ver README.
#
# Uso:
#   ./install-harmonyos.sh <dir-del-paquete>     # volcado + selftest
#   ./install-harmonyos.sh <file.hap>            # instalacion del HAP firmado
#   REMOTE=/data/app/el2/100/base/com.tlaxtrelom.engine ./install-harmonyos.sh ...
set -eu

DEST="${REMOTE:-/data/local/tmp/tlaxtrelom}"
SRC="${1:-}"

if [ -z "$SRC" ]; then
  echo "uso: $0 <dir-del-paquete|file.hap>" >&2
  exit 2
fi

HDC="${HDC:-hdc}"
command -v "$HDC" >/dev/null 2>&1 || {
  echo "no encuentro '$HDC'; instala el toolchain de HarmonyOS o define HDC=/ruta/hdc" >&2
  exit 1
}
"$HDC" list targets 2>/dev/null | grep -q . || {
  echo "no hay dispositivo HarmonyOS conectado (hdc list targets vacio)" >&2
  exit 1
}

case "$SRC" in
  *.hap)
    echo "==> instalando HAP $SRC"
    "$HDC" install -r "$SRC"
    "$HDC" shell aa start -a EntryAbility -b com.tlaxtrelom.engine
    exit 0
    ;;
esac

[ -d "$SRC" ] || { echo "$SRC no es un arbol de paquete" >&2; exit 1; }

echo "==> volcando $SRC a $DEST"
"$HDC" shell mkdir -p "$DEST"
for item in bin libs assets shaders; do
  [ -e "$SRC/$item" ] || continue
  "$HDC" file send "$SRC/$item" "$DEST/"
done

"$HDC" shell "chmod -R u+rwX $DEST"

echo "==> prueba de humo en el dispositivo"
"$HDC" shell "cd $DEST && LD_LIBRARY_PATH=$DEST/libs ./bin/tlaxtrelom --selftest --dimension 3.5d" \
  || { echo "el selftest fallo en el dispositivo" >&2; exit 1; }

echo "listo: arbol en $DEST"
