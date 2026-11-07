/* SPDX-License-Identifier: LicenseRef-Tlaxtrelom-Proprietary
 * Copyright (c) 2026 Tlaxtrelom Engine. All rights reserved.
 * CONFIDENTIAL — internal use only.
 *
 * tlaxtrelom_hal.h — capa de plataforma en C11.
 *
 * La HAL es deliberadamente pequeña: consulta de capacidades, reloj monótono,
 * hash estable y utilidades de tiempo real. No conoce el motor, no asigna
 * memoria y todas sus funciones son seguras ante punteros nulos.
 *
 * Contrato de hilos: `query`, `monotonic_ns`, `thread_id`, `simd_level` y
 * `hash64` son thread-safe. `set_thread_affinity`, `set_thread_priority` y
 * `lock_memory` afectan únicamente al hilo llamador.
 */

#ifndef TLAXTRELOM_HAL_H
#define TLAXTRELOM_HAL_H

#include <stddef.h>
#include <stdint.h>

#define TLAXTRELOM_HAL_ABI_VERSION 1u

#if defined(_WIN32)
#  define TLAXTRELOM_HAL_EXPORT __declspec(dllexport)
#elif defined(__GNUC__) || defined(__clang__)
#  define TLAXTRELOM_HAL_EXPORT __attribute__((visibility("default")))
#else
#  define TLAXTRELOM_HAL_EXPORT
#endif

#ifdef __cplusplus
extern "C" {
#endif

typedef struct TlaxtrelomHalInfo {
    uint32_t abi_version;
    uint32_t page_size;
    uint64_t physical_memory_bytes;
    uint32_t logical_cores;
    uint32_t has_avx2;
    uint32_t has_avx512;
    uint32_t has_neon;
    uint32_t has_realtime;
    uint32_t simd_level; /* 0 = base, 1 = SSE2/NEON, 2 = AVX2, 3 = AVX-512 */
    char os[32];
    char arch[32];
} TlaxtrelomHalInfo;

TLAXTRELOM_HAL_EXPORT uint32_t tlaxtrelom_hal_abi_version(void);

/* Rellena `out`. Devuelve 0 en éxito, -1 si `out` es NULL. */
TLAXTRELOM_HAL_EXPORT int32_t tlaxtrelom_hal_query(TlaxtrelomHalInfo* out);

/* Reloj monótono en nanosegundos. 0 si la plataforma no lo soporta. */
TLAXTRELOM_HAL_EXPORT uint64_t tlaxtrelom_hal_monotonic_ns(void);

/* Identificador opaco del hilo llamador. */
TLAXTRELOM_HAL_EXPORT uint64_t tlaxtrelom_hal_thread_id(void);

/* Fija el hilo llamador a un core. 0 en éxito, -1 si no está soportado. */
TLAXTRELOM_HAL_EXPORT int32_t tlaxtrelom_hal_set_thread_affinity(uint32_t core);

/* SCHED_FIFO con prioridad [1, 99]. 0 en éxito, -1 si falta CAP_SYS_NICE. */
TLAXTRELOM_HAL_EXPORT int32_t tlaxtrelom_hal_set_thread_priority(int32_t priority);

/* mlockall(MCL_CURRENT|MCL_FUTURE). 0 en éxito, -1 si no está soportado. */
TLAXTRELOM_HAL_EXPORT int32_t tlaxtrelom_hal_lock_memory(void);

/* Nivel SIMD detectado en tiempo de ejecución. */
TLAXTRELOM_HAL_EXPORT uint32_t tlaxtrelom_hal_simd_level(void);

/* FNV-1a de 64 bits. Determinista entre plataformas. */
TLAXTRELOM_HAL_EXPORT uint64_t tlaxtrelom_hal_hash64(const void* data, size_t len, uint64_t seed);

/* Redondea `bytes` al siguiente múltiplo de `alignment` (potencia de dos).
 * Devuelve 0 si la operación desborda. */
TLAXTRELOM_HAL_EXPORT uint64_t tlaxtrelom_hal_align_up(uint64_t bytes, uint32_t alignment);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* TLAXTRELOM_HAL_H */
