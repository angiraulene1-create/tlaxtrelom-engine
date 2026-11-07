// SPDX-License-Identifier: LicenseRef-Tlaxtrelom-Proprietary
// Copyright (c) 2026 Tlaxtrelom Engine. All rights reserved.
// CONFIDENTIAL — internal use only.
/*
 * tlaxtrelom_geometry.h — ABI pública de la isla C++ de geometría virtual.
 *
 * ── Contrato de hilos ────────────────────────────────────────────────────────
 *  - `tlaxtrelom_geo_create` / `destroy` / `register_asset`: hilo principal.
 *  - `begin_frame` / `collect` / `drain_ready` / `ack_upload` / `get_stats`:
 *    SIEMPRE el mismo hilo (el de render). No son thread-safe entre sí.
 *  - Los hilos de IO internos son propiedad de la isla; el llamador no los ve.
 *
 * ── Contrato de memoria ──────────────────────────────────────────────────────
 *  - `TlaxtrelomGeoReadyPage::data` es válido hasta `tlaxtrelom_geo_ack_upload(page_id)`.
 *  - El llamador NO libera `data`; lo hace `ack_upload`.
 *  - `cache_dir` debe seguir vivo durante `tlaxtrelom_geo_create` (se copia dentro).
 *
 * ── Contrato de errores ──────────────────────────────────────────────────────
 *  - Ninguna excepción C++ cruza esta ABI. Todo se convierte en TlaxtrelomGeoResult.
 *  - `TlaxtrelomGeoResult` es estable; añadir valores al final es compatible.
 */

#pragma once

#include <stdint.h>

#if defined(_WIN32)
#  define TLAXTRELOM_GEO_EXPORT __declspec(dllexport)
#else
#  define TLAXTRELOM_GEO_EXPORT __attribute__((visibility("default")))
#endif

#ifdef __cplusplus
extern "C" {
#endif

typedef struct TlaxtrelomGeoStreamer TlaxtrelomGeoStreamer; /* handle opaco */

typedef enum TlaxtrelomGeoResult {
    TLAXTRELOM_GEO_OK = 0,
    TLAXTRELOM_GEO_ERR_INVALID = 1,
    TLAXTRELOM_GEO_ERR_OOM = 2,
    TLAXTRELOM_GEO_ERR_IO = 3,
    TLAXTRELOM_GEO_ERR_STATE = 4,
    TLAXTRELOM_GEO_ERR_FORMAT = 5
} TlaxtrelomGeoResult;

typedef struct TlaxtrelomGeoConfig {
    uint32_t max_resident_pages;   /* presupuesto de páginas en CPU/GPU */
    uint32_t io_threads;           /* 0 => 1; típico 2–4 */
    uint64_t vram_budget_bytes;    /* informativo para el residency manager */
    const char* cache_dir;         /* UTF-8, válido durante la llamada */
    uint32_t max_ready_pages;      /* cola de páginas listas para subir */
    uint32_t _pad;
} TlaxtrelomGeoConfig;

typedef struct TlaxtrelomGeoView {
    float view_proj[16];           /* column-major, para culling GPU-side */
    float camera[3];
    float fov_y_rad;
    uint32_t screen_width;
    uint32_t screen_height;
    float pixel_error_budget;      /* error máximo en píxeles (1.0 = AAA) */
} TlaxtrelomGeoView;

typedef struct TlaxtrelomGeoPageRequest {
    uint64_t page_id;              /* global: (asset_id << 32) | page_local */
    uint32_t asset_id;
    uint32_t lod;                  /* telemetría / prioridad */
    uint32_t priority;             /* mayor = más urgente */
    uint32_t _pad;
} TlaxtrelomGeoPageRequest;

typedef struct TlaxtrelomGeoReadyPage {
    uint64_t page_id;
    const void* data;              /* válido hasta ack_upload */
    uint32_t size;
    uint32_t format;               /* 0 = crudo; 1 = BC7; 2 = ... */
    uint32_t _pad;
} TlaxtrelomGeoReadyPage;

typedef struct TlaxtrelomGeoStats {
    uint64_t pages_resident;
    uint64_t pages_pending;
    uint64_t bytes_resident;
    uint64_t clusters_visible;
    uint64_t clusters_culled;
    uint64_t page_faults;
    uint64_t io_bytes;
    uint32_t io_queue_depth;
    uint32_t _pad;
} TlaxtrelomGeoStats;

/* Versión de ABI. Major distinto => el runtime rechaza cargar la isla. */
TLAXTRELOM_GEO_EXPORT uint32_t tlaxtrelom_geo_abi_version(void);

TLAXTRELOM_GEO_EXPORT TlaxtrelomGeoResult
tlaxtrelom_geo_create(const TlaxtrelomGeoConfig* cfg, TlaxtrelomGeoStreamer** out);

TLAXTRELOM_GEO_EXPORT void tlaxtrelom_geo_destroy(TlaxtrelomGeoStreamer* s);

/* Registra un asset ya cocinado (.tlaxtrelomgeo). `path` es UTF-8. */
TLAXTRELOM_GEO_EXPORT TlaxtrelomGeoResult
tlaxtrelom_geo_register_asset(TlaxtrelomGeoStreamer* s, uint32_t asset_id, const char* path);

/* Copia la vista del frame. Llamar antes de `collect`. */
TLAXTRELOM_GEO_EXPORT TlaxtrelomGeoResult
tlaxtrelom_geo_begin_frame(TlaxtrelomGeoStreamer* s, const TlaxtrelomGeoView* view, uint64_t frame_id);

/*
 * Recorre el DAG de clusters visible y devuelve las peticiones generadas.
 * Internamente ya encola las páginas faltantes a los hilos de IO. El array de
 * salida es para telemetría/priorización del lado Rust; puede ser pequeño.
 */
TLAXTRELOM_GEO_EXPORT uint32_t
tlaxtrelom_geo_collect(TlaxtrelomGeoStreamer* s, TlaxtrelomGeoPageRequest* out, uint32_t capacity);

/* Devuelve páginas descomprimidas listas para subir a GPU. */
TLAXTRELOM_GEO_EXPORT uint32_t
tlaxtrelom_geo_drain_ready(TlaxtrelomGeoStreamer* s, TlaxtrelomGeoReadyPage* out, uint32_t capacity);

/* Confirma que la página ya está en VRAM y libera el buffer de CPU. */
TLAXTRELOM_GEO_EXPORT TlaxtrelomGeoResult
tlaxtrelom_geo_ack_upload(TlaxtrelomGeoStreamer* s, uint64_t page_id);

TLAXTRELOM_GEO_EXPORT void
tlaxtrelom_geo_get_stats(const TlaxtrelomGeoStreamer* s, TlaxtrelomGeoStats* out);

#ifdef __cplusplus
} /* extern "C" */
#endif
