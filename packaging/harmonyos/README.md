<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 2026 angiraulene1-create -->

# HarmonyOS for PC

Este directorio es el **proyecto HAP** (modelo Stage, API 12) que envuelve el
puente C++ de `native/tlaxtrelom-harmonyos`. `cargo xtask package --platform
harmonyos` copia el proyecto a `dist/` junto a la biblioteca nativa ya
compilada y los recursos del motor.

## Qué se empaqueta aquí

```text
app.json5                 paquete: com.tlaxtrelom.engine
build-profile.json5       productos y modos de hvigor
entry/src/main/module.json5   ability de entrada, dispositivos 2in1/tablet
entry/src/main/ets/       EntryAbility + pagina Index con el XComponent
entry/libs/arm64-v8a/     libtlaxtrelom_harmonyos.so (rellena xtask package)
assets/ shaders/          claster de demostracion y sombreadores WGSL
install-harmonyos.sh      Volcado por hdc del arbol nativo y prueba de humo
```

## Dos rutas de instalacion

1. **Nativa (funciona hoy, sin SDK)**: `install-harmonyos.sh` envia por `hdc`
   el ejecutable `aarch64-unknown-linux-ohos`, la biblioteca del puente y los
   recursos a `/data/local/tmp/tlaxtrelom`, y corre `--selftest` en el
   dispositivo. Sirve para validar el HAL y el render en HarmonyOS sin firma.
2. **HAP (requiere DevEco Studio)**: abrir `packaging/harmonyos` como proyecto,
   compilar `entry` y firmar con un certificado de desarrollador;
   `hdc install entry-default-signed.hap` lo instala como aplicacion con
   acceso directo e icono.

## Limitacion honesta

El puente expone funciones C (`tlaxtrelom_harmonyos_init_surface`,
`on_window_focus`, `on_window_resize`, `destroy_surface`), no un modulo NAPI, asi que
la capa ArkTS aun **no puede llamarlo** desde el HAP. Antes de publicar la
aplicacion hace falta un envoltorio `napi_init.cpp` que registre
`initSurface`/`onWindowResize`/`destroySurface` y repare la superficie del XComponent;
por eso `Index.ets` solo reserva el `surfaceId`. La ruta nativa (1) no tiene
esa limitacion porque ejecuta el binario directamente.

Tampoco hay SDK de OHOS ni NDK en el repositorio ni en la CI publica:
el binario `aarch64-unknown-linux-ohos` y el `.so` del puente se construyen
con `OHOS_NDK_HOME` definido (`make harmonyos-ohos` /
`xtask package --platform harmonyos`), y el `.hap` final se produce en
DevEco. `xtask package-check` valida los manifiestos y que existan los
recursos referenciados, no que el HAP compile.
