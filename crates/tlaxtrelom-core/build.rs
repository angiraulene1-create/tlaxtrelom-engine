// SPDX-License-Identifier: LicenseRef-Tlaxtrelom-Proprietary
// Copyright (c) 2026 Tlaxtrelom Engine. All rights reserved.
// CONFIDENTIAL — internal use only.
//! Build script del runtime.
//!
//! - `native-geometry`: aporta flags de enlace contra la isla C++ ya construida
//!   (`cmake` o `make -C native/tlaxtrelom-geometry`).
//! - `native-hal`: compila la HAL C11 con `cc` y la enlaza estáticamente.
//! - Siempre: invalida el build si cambian shaders o cabeceras.

use std::env;
use std::path::{Path, PathBuf};

fn main() {
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap_or_default());
    let root = manifest.join("../..");

    for path in [
        root.join("shaders/stereo.wgsl"),
        root.join("native/tlaxtrelom-hal/include/tlaxtrelom_hal.h"),
        root.join("native/tlaxtrelom-hal/src/tlaxtrelom_hal.c"),
    ] {
        println!("cargo:rerun-if-changed={}", path.display());
    }

    build_hal(&root);
    link_geometry(&root);
}

fn build_hal(root: &Path) {
    if env::var_os("CARGO_FEATURE_NATIVE_HAL").is_none() {
        return;
    }

    let src = root.join("native/tlaxtrelom-hal/src/tlaxtrelom_hal.c");
    let include = root.join("native/tlaxtrelom-hal/include");
    if !src.exists() {
        panic!(
            "feature native-hal activa pero falta {}; restaurá la isla C",
            src.display()
        );
    }

    let mut build = cc::Build::new();
    build
        .file(&src)
        .include(&include)
        .warnings(true)
        .extra_warnings(true)
        .flag_if_supported("-std=c11")
        .flag_if_supported("-Wall")
        .flag_if_supported("-Wextra")
        .flag_if_supported("-Wpedantic")
        .flag_if_supported("-Wconversion")
        .flag_if_supported("-Wshadow")
        .compile("tlaxtrelom_hal");

    println!("cargo:rustc-link-lib=static=tlaxtrelom_hal");
}

fn link_geometry(root: &Path) {
    if env::var_os("CARGO_FEATURE_NATIVE_GEOMETRY").is_none() {
        return;
    }

    println!("cargo:rerun-if-env-changed=TLAXTRELOM_GEOMETRY_DIR");

    let dir = env::var_os("TLAXTRELOM_GEOMETRY_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("native/tlaxtrelom-geometry/build"));

    if !dir.exists() {
        panic!(
            "feature native-geometry activa pero no existe {}; \
             ejecutá `make -C native/tlaxtrelom-geometry` o \
             `cmake -S native/tlaxtrelom-geometry -B native/tlaxtrelom-geometry/build`",
            dir.display()
        );
    }

    let candidates = [
        dir.join("libtlaxtrelom_geometry.so"),
        dir.join("libtlaxtrelom_geometry.dylib"),
        dir.join("tlaxtrelom_geometry.dll"),
    ];
    if !candidates.iter().any(|p| p.exists()) {
        panic!(
            "no se encontró la biblioteca de geometría en {}; \
             revisá el build nativo antes de activar la feature",
            dir.display()
        );
    }

    println!("cargo:rustc-link-search=native={}", dir.display());
    println!("cargo:rustc-link-lib=dylib=tlaxtrelom_geometry");

    // RPATH para que el binario encuentre la biblioteca sin LD_LIBRARY_PATH.
    if cfg!(target_os = "linux") {
        println!("cargo:rustc-link-arg=-Wl,-rpath,{}", dir.display());
    }
}
