// SPDX-License-Identifier: LicenseRef-Tlaxtrelom-Proprietary
// Copyright (c) 2026 Tlaxtrelom Engine. All rights reserved.
// CONFIDENTIAL — internal use only.
//! `xtask`: orquestador de build, test y CI sin dependencias externas.
//!
//! Cada comando es explícito y reproducible. `xtask ci` es exactamente lo que
//! corre el workflow de `.github/workflows/ci.yml`, de modo que un fallo local
//! y un fallo de CI no puedan divergir.

use std::env;
use std::path::Path;
use std::process::{Command, ExitStatus};

type Result<T> = std::result::Result<T, String>;

fn main() {
    if let Err(err) = run() {
        eprintln!("xtask: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let mut args = env::args().skip(1);
    let command = args.next().unwrap_or_else(|| "help".to_string());
    let rest: Vec<String> = args.collect();

    match command.as_str() {
        "help" | "-h" | "--help" => {
            print_help();
            Ok(())
        }
        "fmt" => exec("cargo", &["fmt", "--all"], None),
        "fmt-check" => exec("cargo", &["fmt", "--all", "--", "--check"], None),
        "clippy" => {
            exec("make", &["-C", "native/tlaxtrelom-geometry", "all"], None)?;
            exec(
                "cargo",
                &[
                    "clippy",
                    "--workspace",
                    "--all-targets",
                    "--all-features",
                    "--",
                    "-D",
                    "warnings",
                ],
                None,
            )
        }
        "check" => exec("cargo", &["check", "--workspace", "--all-targets"], None),
        "test" => exec("cargo", &["test", "--workspace"], None),
        "test-all" => {
            exec("make", &["-C", "native/tlaxtrelom-geometry", "all"], None)?;
            exec("cargo", &["test", "--workspace", "--features", "native-geometry"], None)
        }
        "native" => exec("make", &["-C", "native/tlaxtrelom-geometry", "all"], None),
        "native-test" => exec("make", &["-C", "native/tlaxtrelom-geometry", "test"], None),
        "native-clean" => exec("make", &["-C", "native/tlaxtrelom-geometry", "clean"], None),
        "spatial" => exec("make", &["-C", "native/tlaxtrelom-spatial", "test"], None),
        "harmonyos" => exec("make", &["-C", "native/tlaxtrelom-harmonyos", "test"], None),
        "shaders" => check_shaders(),
        "dotnet" => exec("python3", &["dotnet/test_csharp.py"], None),
        "relay" => exec(
            "python3",
            &["services/tlaxtrelom-relay/test_relay.py"],
            None,
        ),
        "python" => exec(
            "python3",
            &["-m", "unittest", "discover", "-s", "tests", "-v"],
            Some(Path::new("python")),
        ),
        "lua" => exec("lua", &["script/tests/run.lua"], None),
        "node" => exec(
            "node",
            &["--test", "tests"],
            Some(Path::new("tools/tlaxtrelom-ts")),
        ),
        "protocol" => exec(
            "python3",
            &["-m", "tlaxtrelom_tools.cli", "selftest"],
            Some(Path::new("python")),
        ),
        "docs" => check_docs(),
        "ci" => ci(),
        "all" => {
            run()?;
            // El comando `all` se expande a la matriz completa.
            ci()
        }
        "clean" => {
            let _ = exec("cargo", &["clean"], None);
            let _ = exec("make", &["-C", "native/tlaxtrelom-geometry", "clean"], None);
            let _ = std::fs::remove_dir_all("native/tlaxtrelom-geometry/build");
            Ok(())
        }
        "run" => {
            let mut argv = vec!["run", "-p", "tlaxtrelom-core", "--bin", "tlaxtrelom", "--"];
            argv.extend(rest.iter().map(String::as_str));
            exec("cargo", &argv, None)
        }
        other => Err(format!("comando desconocido: {other} (usá `xtask help`)")),
    }
}

fn ci() -> Result<()> {
    println!("==> fmt");
    exec("cargo", &["fmt", "--all", "--", "--check"], None)?;

    println!("==> C++ (geometría)");
    exec("make", &["-C", "native/tlaxtrelom-geometry", "all"], None)?;
    exec("make", &["-C", "native/tlaxtrelom-geometry", "test"], None)?;

    println!("==> clippy");
    exec(
        "cargo",
        &[
            "clippy",
            "--workspace",
            "--all-targets",
            "--all-features",
            "--",
            "-D",
            "warnings",
        ],
        None,
    )?;

    println!("==> C/Zig (cinemática espacial SIMD)");
    exec("make", &["-C", "native/tlaxtrelom-spatial", "test"], None)?;

    println!("==> HarmonyOS for PC (OpenHarmony NDK bridge)");
    exec("make", &["-C", "native/tlaxtrelom-harmonyos", "test"], None)?;

    println!("==> GLSL & SPIR-V (shaders estéreo y compute)");
    check_shaders()?;

    println!("==> Rust (default + isla C++)");
    exec("cargo", &["test", "--workspace"], None)?;
    exec(
        "cargo",
        &["test", "--workspace", "--features", "native-geometry"],
        None,
    )?;

    println!("==> C# (.NET SDK Tlaxtrelom.Engine)");
    exec("python3", &["dotnet/test_csharp.py"], None)?;

    println!("==> Go (Protocolo de red FullDive y Relay)");
    exec(
        "python3",
        &["services/tlaxtrelom-relay/test_relay.py"],
        None,
    )?;

    println!("==> Python");
    exec(
        "python3",
        &["-m", "unittest", "discover", "-s", "tests", "-v"],
        Some(Path::new("python")),
    )?;

    println!("==> Lua");
    exec("lua", &["script/tests/run.lua"], None)?;

    println!("==> TypeScript/Node");
    exec(
        "node",
        &["--test", "tests"],
        Some(Path::new("tools/tlaxtrelom-ts")),
    )?;

    println!("==> selftest headless");
    exec(
        "cargo",
        &[
            "run",
            "-q",
            "-p",
            "tlaxtrelom-core",
            "--bin",
            "tlaxtrelom",
            "--",
            "--selftest",
            "--json",
        ],
        None,
    )?;

    println!("==> docs");
    check_docs()?;

    println!("\nCI completo: todo verde.");
    Ok(())
}

fn check_shaders() -> Result<()> {
    let shaders = [
        (
            "shaders/glsl/stereo_multiview.vert",
            "shaders/spv/stereo_multiview.vert.spv",
        ),
        (
            "shaders/glsl/stereo_multiview.frag",
            "shaders/spv/stereo_multiview.frag.spv",
        ),
        (
            "shaders/glsl/fulldive_foveated.comp",
            "shaders/spv/fulldive_foveated.comp.spv",
        ),
    ];
    let _ = std::fs::create_dir_all("shaders/spv");
    for (src, spv) in shaders {
        exec("glslc", &[src, "-o", spv], None)?;
        exec("spirv-val", &[spv], None)?;
    }
    println!("    3 shaders GLSL compilados a SPIR-V y validados");
    Ok(())
}

fn exec(program: &str, args: &[&str], cwd: Option<&Path>) -> Result<()> {
    let display = format!("{program} {}", args.join(" "));
    println!("$ {display}");
    let mut command = Command::new(program);
    command.args(args);
    if let Some(dir) = cwd {
        command.current_dir(dir);
    }
    let status = command
        .status()
        .map_err(|e| format!("no se pudo ejecutar {display}: {e}"))?;
    check_status(&display, status)
}

fn check_status(display: &str, status: ExitStatus) -> Result<()> {
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "{display} falló con estado {}",
            status
                .code()
                .map_or_else(|| "señal".into(), |c| c.to_string())
        ))
    }
}

/// Verifica que los enlaces Markdown locales apunten a archivos existentes.
/// No valida URLs externas (eso requiere red y un crawler).
fn check_docs() -> Result<()> {
    println!("==> enlaces locales en docs/ y README");
    let mut checked = 0usize;
    for path in [
        "README.md",
        "docs/01-architecture.md",
        "docs/02-fulldive.md",
        "docs/03-tlaxtrelom-ir.md",
        "docs/04-propiedad-intelectual.md",
        "docs/05-vr-fulldive.md",
        "docs/06-lenguajes.md",
        "docs/07-estabilidad.md",
        "docs/08-operaciones.md",
    ] {
        let file = Path::new(path);
        if !file.exists() {
            return Err(format!("documento esperado no existe: {path}"));
        }
        let text =
            std::fs::read_to_string(file).map_err(|e| format!("no se pudo leer {path}: {e}"))?;
        for target in local_links(&text) {
            let resolved = file.parent().unwrap_or(Path::new(".")).join(&target);
            if !resolved.exists() {
                return Err(format!(
                    "enlace roto en {path}: {target} (resuelve a {})",
                    resolved.display()
                ));
            }
            checked += 1;
        }
    }
    println!("    {checked} enlaces locales OK");
    Ok(())
}

fn local_links(markdown: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = markdown.as_bytes();
    let mut i = 0usize;
    while i + 1 < bytes.len() {
        if bytes[i] == b']' && bytes[i + 1] == b'(' {
            let start = i + 2;
            if let Some(end) = markdown[start..].find(')') {
                let raw = &markdown[start..start + end];
                let target = raw.split('#').next().unwrap_or("").trim();
                if !target.is_empty()
                    && !target.starts_with("http://")
                    && !target.starts_with("https://")
                    && !target.starts_with("mailto:")
                {
                    out.push(target.to_string());
                }
            }
            i = start;
        }
        i += 1;
    }
    out
}

fn print_help() {
    println!(
        "\
 Tlaxtrelom Engine — xtask

 USO:
     cargo xtask <comando>

 COMANDOS:
     fmt             formatea todo el workspace
     fmt-check       verifica formato sin escribir
     clippy          clippy con -D warnings
     check           cargo check --workspace --all-targets
     test            tests Rust con features por defecto
     test-all        tests Rust con la isla C++ enlazada
     native          compila la isla C++ (Makefile)
     native-test     ejecuta el smoke test de la ABI C++
     native-clean    limpia el build C++
     spatial         tests de la cinemática espacial C/Zig
     harmonyos       tests del puente HarmonyOS for PC (ArkUI/XComponent)
     shaders         compila y valida shaders GLSL a SPIR-V
     dotnet          valida el SDK C# (Tlaxtrelom.Engine)
     relay           valida el protocolo Go de telemetría FullDive/VR
     python          tests de las herramientas Python
     lua             tests del runtime de scripting Lua
     node            tests de las herramientas TypeScript
     protocol        valida el encoder HIR desde Python
     docs            verifica enlaces locales de la documentación
     ci              matriz completa políglota y determinista
     run [args...]   ejecuta el host (pasa los args al binario)
     clean           limpia artefactos
 "
    );
}
