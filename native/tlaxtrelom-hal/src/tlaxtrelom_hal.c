/* SPDX-License-Identifier: LicenseRef-Tlaxtrelom-Proprietary
 * Copyright (c) 2026 Tlaxtrelom Engine. All rights reserved.
 * CONFIDENTIAL — internal use only.
 *
 * tlaxtrelom_hal.c — implementación multiplataforma C11 para:
 *   - Linux (POSIX, glibc/musl, sched_setaffinity, mlockall)
 *   - Windows (Win32, QueryPerformanceCounter, SetThreadAffinityMask)
 *   - macOS (Darwin, mach_absolute_time, thread_policy_set)
 *   - HarmonyOS for PC (OpenHarmony NEXT PC, HongMeng POSIX NDK)
 */

#if defined(__linux__) && !defined(_GNU_SOURCE)
#  define _GNU_SOURCE 1
#endif
#if defined(__linux__) && !defined(_DEFAULT_SOURCE)
#  define _DEFAULT_SOURCE 1
#endif

#include "tlaxtrelom_hal.h"

#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <time.h>

#if defined(_WIN32)
#  define WIN32_LEAN_AND_MEAN
#  include <windows.h>
#  include <intrin.h>
#elif defined(__APPLE__)
#  include <mach/mach_time.h>
#  include <mach/thread_policy.h>
#  include <mach/thread_act.h>
#  include <pthread.h>
#  include <sys/mman.h>
#  include <sys/sysctl.h>
#  include <sys/types.h>
#  include <unistd.h>
#elif defined(__OHOS__) || defined(__HARMONYOS__) || defined(__LITEOS__)
#  define TLAXTRELOM_OS_HARMONYOS 1
#  include <pthread.h>
#  include <sched.h>
#  include <sys/mman.h>
#  include <sys/types.h>
#  include <unistd.h>
#elif defined(__linux__)
#  include <pthread.h>
#  include <sched.h>
#  include <sys/mman.h>
#  include <sys/types.h>
#  include <unistd.h>
#endif

static void copy_str(char* dst, size_t cap, const char* src) {
    if (dst == NULL || cap == 0u) {
        return;
    }
    if (src == NULL) {
        dst[0] = '\0';
        return;
    }
    (void)snprintf(dst, cap, "%s", src);
}

static uint32_t detect_simd(void) {
    uint32_t level = 0u;
#if (defined(__x86_64__) || defined(__i386__) || defined(_M_X64) || defined(_M_IX86))
#  if defined(_MSC_VER)
    int info[4];
    __cpuid(info, 1);
    if ((info[3] & (1 << 26)) != 0) level = 1u; // SSE2
    __cpuidex(info, 7, 0);
    if ((info[1] & (1 << 5)) != 0) level = 2u;  // AVX2
    if ((info[1] & (1 << 16)) != 0) level = 3u; // AVX-512F
#  elif defined(__GNUC__) || defined(__clang__)
    __builtin_cpu_init();
    if (__builtin_cpu_supports("sse2")) {
        level = 1u;
    }
    if (__builtin_cpu_supports("avx2")) {
        level = 2u;
    }
    if (__builtin_cpu_supports("avx512f")) {
        level = 3u;
    }
#  endif
#elif defined(__aarch64__) || defined(__ARM_NEON) || defined(_M_ARM64)
    level = 1u;
#endif
    return level;
}

uint32_t tlaxtrelom_hal_abi_version(void) {
    return TLAXTRELOM_HAL_ABI_VERSION;
}

int32_t tlaxtrelom_hal_query(TlaxtrelomHalInfo* out) {
    if (out == NULL) {
        return -1;
    }

    memset(out, 0, sizeof(*out));
    out->abi_version = TLAXTRELOM_HAL_ABI_VERSION;
    out->page_size = 4096u;
    out->logical_cores = 1u;
    out->simd_level = tlaxtrelom_hal_simd_level();
    out->has_avx2 = (out->simd_level >= 2u) ? 1u : 0u;
    out->has_avx512 = (out->simd_level >= 3u) ? 1u : 0u;

#if defined(__x86_64__) || defined(_M_X64)
    copy_str(out->arch, sizeof(out->arch), "x86_64");
#elif defined(__i386__) || defined(_M_IX86)
    copy_str(out->arch, sizeof(out->arch), "x86");
#elif defined(__aarch64__) || defined(_M_ARM64)
    copy_str(out->arch, sizeof(out->arch), "aarch64");
    out->has_neon = 1u;
#elif defined(__arm__) || defined(_M_ARM)
    copy_str(out->arch, sizeof(out->arch), "arm");
#else
    copy_str(out->arch, sizeof(out->arch), "unknown");
#endif

#if defined(TLAXTRELOM_OS_HARMONYOS)
    copy_str(out->os, sizeof(out->os), "harmonyos");
    {
        const long page = sysconf(_SC_PAGESIZE);
        const long pages = sysconf(_SC_PHYS_PAGES);
        const long cores = sysconf(_SC_NPROCESSORS_ONLN);
        if (page > 0) out->page_size = (uint32_t)page;
        if (page > 0 && pages > 0) out->physical_memory_bytes = (uint64_t)page * (uint64_t)pages;
        if (cores > 0) out->logical_cores = (uint32_t)cores;
        out->has_realtime = 1u;
    }
#elif defined(_WIN32)
    copy_str(out->os, sizeof(out->os), "windows");
    {
        SYSTEM_INFO sys_info;
        GetSystemInfo(&sys_info);
        out->page_size = (uint32_t)sys_info.dwPageSize;
        out->logical_cores = (uint32_t)sys_info.dwNumberOfProcessors;

        MEMORYSTATUSEX mem_status;
        mem_status.dwLength = sizeof(mem_status);
        if (GlobalMemoryStatusEx(&mem_status)) {
            out->physical_memory_bytes = (uint64_t)mem_status.ullTotalPhys;
        }
        out->has_realtime = 1u;
    }
#elif defined(__APPLE__)
    copy_str(out->os, sizeof(out->os), "macos");
    {
        int64_t mem = 0;
        size_t len = sizeof(mem);
        if (sysctlbyname("hw.memsize", &mem, &len, NULL, 0) == 0 && mem > 0) {
            out->physical_memory_bytes = (uint64_t)mem;
        }
        int32_t cores = 0;
        len = sizeof(cores);
        if (sysctlbyname("hw.logicalcpu", &cores, &len, NULL, 0) == 0 && cores > 0) {
            out->logical_cores = (uint32_t)cores;
        }
        out->page_size = (uint32_t)getpagesize();
        out->has_realtime = 1u;
    }
#elif defined(__linux__)
    copy_str(out->os, sizeof(out->os), "linux");
    {
        const long page = sysconf(_SC_PAGESIZE);
        const long pages = sysconf(_SC_PHYS_PAGES);
        const long cores = sysconf(_SC_NPROCESSORS_ONLN);

        if (page > 0) {
            out->page_size = (uint32_t)page;
        }
        if (page > 0 && pages > 0) {
            out->physical_memory_bytes = (uint64_t)page * (uint64_t)pages;
        }
        if (cores > 0) {
            out->logical_cores = (uint32_t)cores;
        }
        out->has_realtime = (geteuid() == 0) ? 1u : 0u;
    }
#else
    copy_str(out->os, sizeof(out->os), "unknown");
#endif

    return 0;
}

uint64_t tlaxtrelom_hal_monotonic_ns(void) {
#if defined(_WIN32)
    LARGE_INTEGER counter, freq;
    if (QueryPerformanceCounter(&counter) && QueryPerformanceFrequency(&freq) && freq.QuadPart > 0) {
        return (uint64_t)((counter.QuadPart * 1000000000ull) / freq.QuadPart);
    }
    return 0u;
#elif defined(__APPLE__)
    static mach_timebase_info_data_t tb;
    if (tb.denom == 0) {
        (void)mach_timebase_info(&tb);
    }
    uint64_t mach_now = mach_absolute_time();
    return (mach_now * (uint64_t)tb.numer) / (uint64_t)tb.denom;
#elif defined(CLOCK_MONOTONIC)
    struct timespec ts;
    if (clock_gettime(CLOCK_MONOTONIC, &ts) != 0) {
        return 0u;
    }
    return (uint64_t)ts.tv_sec * 1000000000ull + (uint64_t)ts.tv_nsec;
#else
    return 0u;
#endif
}

uint64_t tlaxtrelom_hal_thread_id(void) {
#if defined(_WIN32)
    return (uint64_t)GetCurrentThreadId();
#elif defined(__APPLE__)
    uint64_t tid = 0;
    pthread_threadid_np(NULL, &tid);
    return tid;
#elif defined(__linux__) || defined(TLAXTRELOM_OS_HARMONYOS)
    return (uint64_t)(uintptr_t)pthread_self();
#else
    return 0u;
#endif
}

int32_t tlaxtrelom_hal_set_thread_affinity(uint32_t core) {
#if defined(_WIN32)
    if (core >= 64u) {
        return -1;
    }
    DWORD_PTR mask = ((DWORD_PTR)1) << core;
    return (SetThreadAffinityMask(GetCurrentThread(), mask) != 0) ? 0 : -1;
#elif defined(__APPLE__)
    thread_affinity_policy_data_t policy = { (integer_t)core };
    return (thread_policy_set(pthread_mach_thread_np(pthread_self()),
                              THREAD_AFFINITY_POLICY,
                              (thread_policy_t)&policy,
                              THREAD_AFFINITY_POLICY_COUNT) == KERN_SUCCESS) ? 0 : -1;
#elif defined(__linux__) || defined(TLAXTRELOM_OS_HARMONYOS)
    cpu_set_t set;
    CPU_ZERO(&set);
    if (core >= (uint32_t)CPU_SETSIZE) {
        return -1;
    }
    CPU_SET((size_t)core, &set);
    return (sched_setaffinity(0, sizeof(set), &set) == 0) ? 0 : -1;
#else
    (void)core;
    return -1;
#endif
}

int32_t tlaxtrelom_hal_set_thread_priority(int32_t priority) {
#if defined(_WIN32)
    int win_prio = THREAD_PRIORITY_NORMAL;
    if (priority >= 80) win_prio = THREAD_PRIORITY_TIME_CRITICAL;
    else if (priority >= 60) win_prio = THREAD_PRIORITY_HIGHEST;
    else if (priority >= 40) win_prio = THREAD_PRIORITY_ABOVE_NORMAL;
    else if (priority >= 20) win_prio = THREAD_PRIORITY_NORMAL;
    else win_prio = THREAD_PRIORITY_BELOW_NORMAL;
    return (SetThreadPriority(GetCurrentThread(), win_prio) != 0) ? 0 : -1;
#elif defined(__APPLE__)
    struct sched_param param;
    memset(&param, 0, sizeof(param));
    param.sched_priority = (priority > 50) ? 45 : 31;
    return (pthread_setschedparam(pthread_self(), SCHED_RR, &param) == 0) ? 0 : -1;
#elif (defined(__linux__) || defined(TLAXTRELOM_OS_HARMONYOS)) && defined(SCHED_FIFO)
    struct sched_param param;
    if (priority < 1 || priority > 99) {
        return -1;
    }
    memset(&param, 0, sizeof(param));
    param.sched_priority = (int)priority;
    return (sched_setscheduler(0, SCHED_FIFO, &param) == 0) ? 0 : -1;
#else
    (void)priority;
    return -1;
#endif
}

int32_t tlaxtrelom_hal_lock_memory(void) {
#if defined(_WIN32)
    HANDLE hProcess = GetCurrentProcess();
    return (SetProcessWorkingSetSize(hProcess, (SIZE_T)64 * 1024 * 1024, (SIZE_T)512 * 1024 * 1024) != 0) ? 0 : -1;
#elif (defined(__linux__) || defined(__APPLE__) || defined(TLAXTRELOM_OS_HARMONYOS)) && defined(MCL_CURRENT) && defined(MCL_FUTURE)
    return (mlockall(MCL_CURRENT | MCL_FUTURE) == 0) ? 0 : -1;
#else
    return -1;
#endif
}

uint32_t tlaxtrelom_hal_simd_level(void) {
    static uint32_t cached = 0u;
    static int initialized = 0;
    if (initialized == 0) {
        cached = detect_simd();
        initialized = 1;
    }
    return cached;
}

uint64_t tlaxtrelom_hal_hash64(const void* data, size_t len, uint64_t seed) {
    const unsigned char* bytes = (const unsigned char*)data;
    uint64_t hash = (seed == 0u) ? 1469598103934665603ull : seed;

    if (bytes == NULL) {
        return hash;
    }

    for (size_t i = 0u; i < len; ++i) {
        hash ^= (uint64_t)bytes[i];
        hash *= 1099511628211ull;
    }
    return hash;
}

uint64_t tlaxtrelom_hal_align_up(uint64_t bytes, uint32_t alignment) {
    uint64_t mask;
    if (alignment == 0u) {
        return 0u;
    }
    if ((alignment & (alignment - 1u)) != 0u) {
        return 0u;
    }
    mask = (uint64_t)alignment - 1ull;
    if (bytes > (UINT64_MAX - mask)) {
        return 0u;
    }
    return (bytes + mask) & ~mask;
}
