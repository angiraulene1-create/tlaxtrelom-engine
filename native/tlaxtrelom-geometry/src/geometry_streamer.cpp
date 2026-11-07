// SPDX-License-Identifier: LicenseRef-Tlaxtrelom-Proprietary
// Copyright (c) 2026 Tlaxtrelom Engine. All rights reserved.
// CONFIDENTIAL — internal use only.
/*
 * geometry_streamer.cpp — streaming de geometría de alta densidad (C++).
 *
 * Modelo:
 *   Malla → clusters de 128 triángulos → DAG jerárquico (LOD) → páginas.
 *   El runtime recorre el DAG con el error proyectado en píxeles y pide las
 *   páginas que faltan. La isla C++ hace IO + descompresión; Rust sube a GPU.
 *
 * Por qué C++ aquí:
 *   - `pread` + zstd + hilos de IO con control fino de afinidad.
 *   - Código de terceros (compresores, códecs de malla) ya existente.
 *   - El DAG y el recorrido son lógica pura, sin dependencias de wgpu.
 *
 * Lo que NO hace:
 *   - No toca Vulkan/DX12/wgpu.
 *   - No conoce entidades, materiales ni shaders.
 *   - No libera memoria de GPU (eso es del residency manager en Rust).
 */

#include "tlaxtrelom_geometry.h"

#include <zstd.h>

#include <algorithm>
#include <atomic>
#include <cmath>
#include <condition_variable>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <deque>
#include <memory>
#include <mutex>
#include <new>
#include <string>
#include <thread>
#include <unordered_map>
#include <utility>
#include <vector>

#if defined(_WIN32)
#  define WIN32_LEAN_AND_MEAN
#  include <windows.h>
#  include <io.h>
#else
#  include <fcntl.h>
#  include <sys/stat.h>
#  include <unistd.h>
#endif

namespace {

constexpr uint32_t kAssetMagic = 0x544C5847u; // "TLXG" — Tlaxtrelom Geometry
constexpr uint32_t kAssetVersion = 1;

// ─────────────────────────────────────────────────────────────────────────────
// Estado de página
// ─────────────────────────────────────────────────────────────────────────────

enum PageState : uint8_t {
    kUnloaded = 0,
    kLoading = 1,
    kReady = 2,
    kResident = 3,
    kEvicting = 4,
};

// 64 bytes: una línea de caché por página. Evita false sharing entre hilos.
struct alignas(64) PageSlot {
    std::atomic<uint8_t> state{kUnloaded};
    std::atomic<uint64_t> last_used{0};
    std::atomic<uint64_t> tag{0}; // page_id actual (detecta colisiones)
    uint32_t size{0};
    uint32_t _pad{0};
};

struct Cluster {
    float center[3];
    float radius;
    float lod_error;     // error en unidades de mundo al usar este cluster
    uint32_t page_local; // índice de página dentro del asset
    uint32_t first_child; // índice del primer hijo en el vector
    uint8_t child_count;
    uint8_t _pad[3];
};
static_assert(sizeof(Cluster) == 32, "layout estable para el cooker");

struct PageDesc {
    uint64_t offset;
    uint32_t compressed_size;
    uint32_t raw_size;
};
static_assert(sizeof(PageDesc) == 16, "layout estable para el cooker");

struct AssetHeader {
    uint32_t magic;
    uint16_t version;
    uint16_t flags;
    uint32_t cluster_count;
    uint32_t page_count;
    uint32_t root_count;
    uint32_t reserved;
};
static_assert(sizeof(AssetHeader) == 24, "layout estable para el cooker");

struct Asset {
    uint32_t id{0};
    int fd{-1};
    std::vector<Cluster> clusters;
    std::vector<uint32_t> roots;
    std::vector<PageDesc> pages;
};

struct ReadyPage {
    uint64_t page_id;
    std::vector<uint8_t> bytes;
};

inline void platform_close(int fd) {
    if (fd >= 0) {
#if defined(_WIN32)
        ::_close(fd);
#else
        ::close(fd);
#endif
    }
}

inline int platform_open_readonly(const char* path) {
#if defined(_WIN32)
    return ::_open(path, _O_RDONLY | _O_BINARY);
#elif defined(O_CLOEXEC)
    return ::open(path, O_RDONLY | O_CLOEXEC);
#else
    return ::open(path, O_RDONLY);
#endif
}

struct FdGuard {
    int fd{-1};
    ~FdGuard() {
        platform_close(fd);
    }
};

inline uint32_t local_of(uint64_t pid) { return uint32_t(pid & 0xFFFFFFFFu); }

bool pread_all(int fd, void* dst, size_t n, uint64_t offset) {
    auto* p = static_cast<uint8_t*>(dst);
    size_t done = 0;
    while (done < n) {
#if defined(_WIN32)
        HANDLE hFile = reinterpret_cast<HANDLE>(::_get_osfhandle(fd));
        if (hFile == INVALID_HANDLE_VALUE) return false;
        OVERLAPPED ov{};
        uint64_t cur_offset = offset + done;
        ov.Offset = static_cast<DWORD>(cur_offset & 0xFFFFFFFF);
        ov.OffsetHigh = static_cast<DWORD>((cur_offset >> 32) & 0xFFFFFFFF);
        DWORD bytesRead = 0;
        DWORD toRead = static_cast<DWORD>(std::min<size_t>(n - done, 0xFFFFFFFFu));
        if (!::ReadFile(hFile, p + done, toRead, &bytesRead, &ov) || bytesRead == 0) {
            return false;
        }
        done += static_cast<size_t>(bytesRead);
#else
        const ssize_t r = ::pread(fd, p + done, n - done, off_t(offset + done));
        if (r <= 0) return false;
        done += size_t(r);
#endif
    }
    return true;
}

} // namespace

// ─────────────────────────────────────────────────────────────────────────────
// Estado del streamer (definición del handle opaco)
// ─────────────────────────────────────────────────────────────────────────────

struct TlaxtrelomGeoStreamer {
    TlaxtrelomGeoConfig cfg{};
    std::string cache_dir;
    TlaxtrelomGeoView view{};
    uint64_t frame_id{0};

    std::unique_ptr<PageSlot[]> slots;
    uint32_t slot_count{0};

    std::vector<std::unique_ptr<Asset>> assets;

    std::atomic<uint64_t> stat_pages_resident{0};
    std::atomic<uint64_t> stat_page_faults{0};
    std::atomic<uint64_t> stat_clusters_visible{0};
    std::atomic<uint64_t> stat_clusters_culled{0};
    std::atomic<uint64_t> stat_io_bytes{0};
    std::atomic<uint32_t> stat_io_queue{0};

    // Cola de peticiones (productor: render; consumidor: IO). El mutex es
    // aceptable porque se toca una vez por página faltante, no por frame.
    std::mutex req_mu;
    std::condition_variable req_cv;
    std::deque<TlaxtrelomGeoPageRequest> requests;
    bool stop{false};

    std::vector<std::thread> io_threads;

    // Páginas descomprimidas + buffers en vuelo hasta ack_upload.
    std::mutex ready_mu;
    std::deque<ReadyPage> ready;
    std::unordered_map<uint64_t, std::vector<uint8_t>> in_flight;
};

namespace {

inline PageSlot& slot_for(TlaxtrelomGeoStreamer* s, uint64_t page_id) {
    return s->slots[page_id % s->slot_count];
}

Asset* find_asset(TlaxtrelomGeoStreamer* s, uint32_t id) {
    for (auto& a : s->assets) {
        if (a->id == id) return a.get();
    }
    return nullptr;
}

void mark_failed(TlaxtrelomGeoStreamer* s, uint64_t page_id) {
    slot_for(s, page_id).state.store(kUnloaded, std::memory_order_release);
}

// Error proyectado en píxeles: (error_mundo * alto_pantalla) /
// (2 * distancia * tan(fov/2)). Si supera el budget, hay que bajar de LOD.
float screen_error(const Cluster& c, const TlaxtrelomGeoView& v) {
    const float dx = c.center[0] - v.camera[0];
    const float dy = c.center[1] - v.camera[1];
    const float dz = c.center[2] - v.camera[2];
    const float dist = std::sqrt(dx * dx + dy * dy + dz * dz);
    const float half_fov = std::max(0.01f, std::tan(v.fov_y_rad * 0.5f));
    return (c.lod_error * float(v.screen_height)) /
           (2.0f * std::max(dist, 0.001f) * half_fov);
}

void request_page(TlaxtrelomGeoStreamer* s, uint32_t asset_id, uint64_t page_id,
                  uint32_t lod, uint32_t priority) {
    PageSlot& slot = slot_for(s, page_id);

    // CAS: solo el primer hilo que ve la página "Unloaded" la pide.
    uint8_t expected = kUnloaded;
    if (!slot.state.compare_exchange_strong(expected, kLoading,
                                            std::memory_order_acq_rel)) {
        return; // ya está cargando o residente
    }
    slot.tag.store(page_id, std::memory_order_relaxed);

    bool queued = false;
    {
        std::lock_guard<std::mutex> lk(s->req_mu);
        if (s->requests.size() < 8192) {
            s->requests.push_back(
                TlaxtrelomGeoPageRequest{page_id, asset_id, lod, priority, 0});
            queued = true;
        }
    }
    if (!queued) {
        // Backpressure: se revierte para reintentar el próximo frame.
        slot.state.store(kUnloaded, std::memory_order_release);
        return;
    }
    s->stat_io_queue.fetch_add(1, std::memory_order_relaxed);
    s->req_cv.notify_one();
}

void traverse(TlaxtrelomGeoStreamer* s, const Asset& a, uint32_t cluster_index,
              uint32_t& out_n, TlaxtrelomGeoPageRequest* out, uint32_t cap,
              uint32_t depth) {
    if (depth > 64 || cluster_index >= a.clusters.size()) return;
    const Cluster& c = a.clusters[cluster_index];

    const float err = screen_error(c, s->view);
    const bool leaf = c.child_count == 0;

    if (err <= s->view.pixel_error_budget || leaf) {
        s->stat_clusters_visible.fetch_add(1, std::memory_order_relaxed);

        const uint64_t pid = (uint64_t(a.id) << 32) | c.page_local;
        PageSlot& slot = slot_for(s, pid);
        const uint8_t st = slot.state.load(std::memory_order_acquire);

        if (st != kResident ||
            slot.tag.load(std::memory_order_relaxed) != pid) {
            const uint32_t prio = uint32_t(std::min(err, 1.0e6f)) + 1;
            request_page(s, a.id, pid, c.page_local, prio);
            s->stat_page_faults.fetch_add(1, std::memory_order_relaxed);
            if (out_n < cap) {
                out[out_n++] = TlaxtrelomGeoPageRequest{pid, a.id, c.page_local,
                                                  prio, 0};
            }
        } else {
            slot.last_used.store(s->frame_id, std::memory_order_relaxed);
        }
        return;
    }

    // Nodo interno visible pero con error alto: se baja a los hijos.
    s->stat_clusters_culled.fetch_add(1, std::memory_order_relaxed);
    for (uint32_t i = 0; i < c.child_count; ++i) {
        traverse(s, a, c.first_child + i, out_n, out, cap, depth + 1);
    }
}

void io_worker(TlaxtrelomGeoStreamer* s) {
    for (;;) {
        TlaxtrelomGeoPageRequest req{};
        {
            std::unique_lock<std::mutex> lk(s->req_mu);
            s->req_cv.wait(lk, [s] {
                return s->stop || !s->requests.empty();
            });
            if (s->stop && s->requests.empty()) return;
            req = s->requests.front();
            s->requests.pop_front();
            s->stat_io_queue.fetch_sub(1, std::memory_order_relaxed);
        }

        Asset* a = find_asset(s, req.asset_id);
        if (!a) {
            mark_failed(s, req.page_id);
            continue;
        }
        const uint32_t local = local_of(req.page_id);
        if (local >= a->pages.size()) {
            mark_failed(s, req.page_id);
            continue;
        }
        const PageDesc& pd = a->pages[local];
        if (pd.compressed_size == 0 || pd.raw_size == 0) {
            mark_failed(s, req.page_id);
            continue;
        }

        std::vector<uint8_t> compressed(pd.compressed_size);
        if (!pread_all(a->fd, compressed.data(), compressed.size(),
                       pd.offset)) {
            mark_failed(s, req.page_id);
            continue;
        }

        std::vector<uint8_t> raw(pd.raw_size);
        const size_t decoded =
            ZSTD_decompress(raw.data(), raw.size(), compressed.data(),
                            compressed.size());
        if (ZSTD_isError(decoded) || decoded != raw.size()) {
            mark_failed(s, req.page_id);
            continue;
        }

        s->stat_io_bytes.fetch_add(pd.compressed_size,
                                   std::memory_order_relaxed);

        {
            std::lock_guard<std::mutex> lk(s->ready_mu);
            if (s->ready.size() < s->cfg.max_ready_pages) {
                s->ready.push_back(ReadyPage{req.page_id, std::move(raw)});
            } else {
                // El render no está drenando: se descarta y se reintenta.
                mark_failed(s, req.page_id);
            }
        }
    }
}

} // namespace

// ─────────────────────────────────────────────────────────────────────────────
// ABI
// ─────────────────────────────────────────────────────────────────────────────

extern "C" {

TLAXTRELOM_GEO_EXPORT uint32_t tlaxtrelom_geo_abi_version(void) { return 1; }

TLAXTRELOM_GEO_EXPORT TlaxtrelomGeoResult
tlaxtrelom_geo_create(const TlaxtrelomGeoConfig* cfg, TlaxtrelomGeoStreamer** out) {
    if (!cfg || !out) return TLAXTRELOM_GEO_ERR_INVALID;
    *out = nullptr;

    TlaxtrelomGeoStreamer* s = nullptr;
    try {
        s = new TlaxtrelomGeoStreamer();
        s->cfg = *cfg;
        if (cfg->cache_dir) s->cache_dir = cfg->cache_dir;
        if (s->cfg.max_resident_pages == 0) s->cfg.max_resident_pages = 4096;
        if (s->cfg.io_threads == 0) s->cfg.io_threads = 1;
        if (s->cfg.max_ready_pages == 0) s->cfg.max_ready_pages = 256;

        s->slot_count = s->cfg.max_resident_pages;
        s->slots = std::make_unique<PageSlot[]>(s->slot_count);

        for (uint32_t i = 0; i < s->cfg.io_threads; ++i) {
            s->io_threads.emplace_back(io_worker, s);
        }
        *out = s;
        return TLAXTRELOM_GEO_OK;
    } catch (const std::bad_alloc&) {
        if (s) {
            {
                std::lock_guard<std::mutex> lk(s->req_mu);
                s->stop = true;
            }
            s->req_cv.notify_all();
            for (auto& t : s->io_threads) {
                if (t.joinable()) t.join();
            }
            delete s;
        }
        return TLAXTRELOM_GEO_ERR_OOM;
    } catch (...) {
        if (s) {
            {
                std::lock_guard<std::mutex> lk(s->req_mu);
                s->stop = true;
            }
            s->req_cv.notify_all();
            for (auto& t : s->io_threads) {
                if (t.joinable()) t.join();
            }
            delete s;
        }
        return TLAXTRELOM_GEO_ERR_STATE;
    }
}

TLAXTRELOM_GEO_EXPORT void tlaxtrelom_geo_destroy(TlaxtrelomGeoStreamer* s) {
    if (!s) return;
    {
        std::lock_guard<std::mutex> lk(s->req_mu);
        s->stop = true;
    }
    s->req_cv.notify_all();
    for (auto& t : s->io_threads) {
        if (t.joinable()) t.join();
    }
    for (auto& a : s->assets) {
        platform_close(a->fd);
    }
    delete s;
}

TLAXTRELOM_GEO_EXPORT TlaxtrelomGeoResult
tlaxtrelom_geo_register_asset(TlaxtrelomGeoStreamer* s, uint32_t asset_id,
                        const char* path) {
    if (!s || !path) return TLAXTRELOM_GEO_ERR_INVALID;
    try {
        FdGuard fd;
        fd.fd = platform_open_readonly(path);
        if (fd.fd < 0) return TLAXTRELOM_GEO_ERR_IO;

        AssetHeader h{};
        if (!pread_all(fd.fd, &h, sizeof(h), 0)) return TLAXTRELOM_GEO_ERR_IO;
        if (h.magic != kAssetMagic || h.version != kAssetVersion) {
            return TLAXTRELOM_GEO_ERR_FORMAT;
        }

        auto a = std::make_unique<Asset>();
        a->id = asset_id;
        a->clusters.resize(h.cluster_count);
        a->roots.resize(h.root_count);
        a->pages.resize(h.page_count);

        uint64_t off = sizeof(AssetHeader);
        if (!pread_all(fd.fd, a->clusters.data(),
                       a->clusters.size() * sizeof(Cluster), off)) {
            return TLAXTRELOM_GEO_ERR_IO;
        }
        off += a->clusters.size() * sizeof(Cluster);
        if (!pread_all(fd.fd, a->roots.data(),
                       a->roots.size() * sizeof(uint32_t), off)) {
            return TLAXTRELOM_GEO_ERR_IO;
        }
        off += a->roots.size() * sizeof(uint32_t);
        if (!pread_all(fd.fd, a->pages.data(),
                       a->pages.size() * sizeof(PageDesc), off)) {
            return TLAXTRELOM_GEO_ERR_IO;
        }

        a->fd = fd.fd;
        fd.fd = -1; // ownership transferido al asset
        s->assets.push_back(std::move(a));
        return TLAXTRELOM_GEO_OK;
    } catch (const std::bad_alloc&) {
        return TLAXTRELOM_GEO_ERR_OOM;
    } catch (...) {
        return TLAXTRELOM_GEO_ERR_STATE;
    }
}

TLAXTRELOM_GEO_EXPORT TlaxtrelomGeoResult
tlaxtrelom_geo_begin_frame(TlaxtrelomGeoStreamer* s, const TlaxtrelomGeoView* view,
                     uint64_t frame_id) {
    if (!s || !view) return TLAXTRELOM_GEO_ERR_INVALID;
    // Solo el hilo de render llama a esta función; no hay carrera con
    // `collect`, que se ejecuta inmediatamente después en el mismo hilo.
    s->view = *view;
    s->frame_id = frame_id;
    return TLAXTRELOM_GEO_OK;
}

TLAXTRELOM_GEO_EXPORT uint32_t
tlaxtrelom_geo_collect(TlaxtrelomGeoStreamer* s, TlaxtrelomGeoPageRequest* out,
                 uint32_t capacity) {
    if (!s || !out || capacity == 0) return 0;
    uint32_t n = 0;
    for (auto& a : s->assets) {
        for (uint32_t root : a->roots) {
            traverse(s, *a, root, n, out, capacity, 0);
        }
    }
    // Prioridad descendente: el residency manager de Rust sube primero lo
    // que más se va a ver. `std::sort` no asigna memoria.
    std::sort(out, out + n, [](const TlaxtrelomGeoPageRequest& a,
                               const TlaxtrelomGeoPageRequest& b) {
        return a.priority > b.priority;
    });
    return n;
}

TLAXTRELOM_GEO_EXPORT uint32_t
tlaxtrelom_geo_drain_ready(TlaxtrelomGeoStreamer* s, TlaxtrelomGeoReadyPage* out,
                     uint32_t capacity) {
    if (!s || !out || capacity == 0) return 0;
    try {
        std::lock_guard<std::mutex> lk(s->ready_mu);
        uint32_t n = 0;
        while (n < capacity && !s->ready.empty()) {
            ReadyPage rp = std::move(s->ready.front());
            s->ready.pop_front();

            // El buffer vive en `in_flight` hasta ack_upload. Mover el vector
            // al mapa no mueve su heap: `data()` queda estable.
            auto [it, inserted] = s->in_flight.try_emplace(
                rp.page_id, std::move(rp.bytes));
            (void)inserted;
            out[n++] = TlaxtrelomGeoReadyPage{rp.page_id, it->second.data(),
                                        uint32_t(it->second.size()), 0, 0};
        }
        return n;
    } catch (...) {
        return 0; // el render reintenta el próximo frame
    }
}

TLAXTRELOM_GEO_EXPORT TlaxtrelomGeoResult
tlaxtrelom_geo_ack_upload(TlaxtrelomGeoStreamer* s, uint64_t page_id) {
    if (!s) return TLAXTRELOM_GEO_ERR_INVALID;
    std::lock_guard<std::mutex> lk(s->ready_mu);
    auto it = s->in_flight.find(page_id);
    if (it == s->in_flight.end()) return TLAXTRELOM_GEO_ERR_STATE;
    s->in_flight.erase(it);

    PageSlot& slot = slot_for(s, page_id);
    slot.state.store(kResident, std::memory_order_release);
    slot.last_used.store(s->frame_id, std::memory_order_relaxed);
    s->stat_pages_resident.fetch_add(1, std::memory_order_relaxed);
    return TLAXTRELOM_GEO_OK;
}

TLAXTRELOM_GEO_EXPORT void
tlaxtrelom_geo_get_stats(const TlaxtrelomGeoStreamer* s, TlaxtrelomGeoStats* out) {
    if (!s || !out) return;
    out->pages_resident =
        s->stat_pages_resident.load(std::memory_order_relaxed);
    out->pages_pending = s->stat_io_queue.load(std::memory_order_relaxed);
    out->bytes_resident = 0; // lo mantiene el residency manager de Rust
    out->clusters_visible =
        s->stat_clusters_visible.load(std::memory_order_relaxed);
    out->clusters_culled =
        s->stat_clusters_culled.load(std::memory_order_relaxed);
    out->page_faults = s->stat_page_faults.load(std::memory_order_relaxed);
    out->io_bytes = s->stat_io_bytes.load(std::memory_order_relaxed);
    out->io_queue_depth = uint32_t(out->pages_pending);
    out->_pad = 0;
}

} // extern "C"
