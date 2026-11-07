/* SPDX-License-Identifier: LicenseRef-Tlaxtrelom-Proprietary
 * Copyright (c) 2026 Tlaxtrelom Engine. All rights reserved.
 * CONFIDENTIAL — internal use only.
 *
 * tlaxtrelom_harmonyos.h — Bridge de integración para HarmonyOS for PC (OpenHarmony NEXT PC).
 *
 * Proporciona el contrato de ciclo de vida entre el subsistema ArkUI / XComponent
 * de HarmonyOS PC y el core determinista de Tlaxtrelom Engine.
 */

#ifndef TLAXTRELOM_HARMONYOS_H
#define TLAXTRELOM_HARMONYOS_H

#include <stdint.h>
#include <stdbool.h>

#define TLAXTRELOM_HARMONYOS_ABI_VERSION 1u

#if defined(_WIN32)
#  define TLAXTRELOM_OHOS_EXPORT __declspec(dllexport)
#elif defined(__GNUC__) || defined(__clang__)
#  define TLAXTRELOM_OHOS_EXPORT __attribute__((visibility("default")))
#else
#  define TLAXTRELOM_OHOS_EXPORT
#endif

#ifdef __cplusplus
extern "C" {
#endif

typedef struct TlaxtrelomOhosSurfaceInfo {
    uint64_t window_id;
    uint32_t width;
    uint32_t height;
    float refresh_rate_hz;
    bool is_hdr_capable;
    char display_name[64];
} TlaxtrelomOhosSurfaceInfo;

TLAXTRELOM_OHOS_EXPORT uint32_t tlaxtrelom_harmonyos_abi_version(void);

/* Inicializa el contexto de render de ventana nativa de HarmonyOS PC (XComponent). */
TLAXTRELOM_OHOS_EXPORT int32_t tlaxtrelom_harmonyos_init_surface(
    const TlaxtrelomOhosSurfaceInfo* info, void** out_handle);

/* Procesa eventos de foco y ciclo de vida de HarmonyOS PC. */
TLAXTRELOM_OHOS_EXPORT void tlaxtrelom_harmonyos_on_window_focus(
    void* handle, bool has_focus);

/* Notifica redimensionamiento de ventana de escritorio HarmonyOS. */
TLAXTRELOM_OHOS_EXPORT void tlaxtrelom_harmonyos_on_window_resize(
    void* handle, uint32_t new_width, uint32_t new_height);

/* Destruye el handle de superficie HarmonyOS. */
TLAXTRELOM_OHOS_EXPORT void tlaxtrelom_harmonyos_destroy_surface(void* handle);

#ifdef __cplusplus
}
#endif

#endif /* TLAXTRELOM_HARMONYOS_H */
