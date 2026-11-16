// SPDX-License-Identifier: MIT
// Copyright (c) 2026 angiraulene1-create
//! Empaquetado portable y de instalación para Linux, Windows y HarmonyOS for PC.
//!
//! Dos entradas, ambas sin dependencias externas obligatorias salvo Rust y `tar`:
//!
//! - [`run`] construye el motor, monta el árbol de cada plataforma en `dist/` y
//!   lo archiva (`.tar.gz` en Linux, `.zip` en Windows y HarmonyOS). Además
//!   produce el `.deb`, ejecuta `makensis` cuando existe y compila la variante
//!   OHOS cuando `OHOS_NDK_HOME` apunta a un NDK.
//! - [`check`] valida las definiciones de `packaging/` sin compilar nada: campos
//!   obligatorios, recursos referenciados que deben existir, marcadores de
//!   plantilla y que las rutas que declara el instalador de Windows son rutas que
//!   el empaquetado realmente entrega.
//!
//! La regla de diseño es **degradar, no fingir**: si falta una herramienta, se
//! omite ese artefacto concreto con un aviso que dice cómo obtenerlo, y ningún
//! archivo se presenta como construido cuando no lo está.

use std::collections::BTreeSet;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Error de empaquetado: el mensaje tal cual lo ve quien corre el comando.
type Result<T> = std::result::Result<T, String>;

/// `fs::*` devuelve `io::Result`, cuyo error no se puede convertir en `String`
/// con `?`; `.io()` hace esa conversión en el sitio de la llamada.
trait Io<T> {
    fn io(self) -> Result<T>;
}

impl<T> Io<T> for std::io::Result<T> {
    fn io(self) -> Result<T> {
        self.map_err(|e| e.to_string())
    }
}

/// Plataformas conocidas; el orden es el de `--all`.
pub const PLATFORMS: [&str; 3] = ["linux", "windows", "harmonyos"];

/// Ficheros de documentación que van en la raíz de cualquier paquete. Todos son
/// de nivel raíz: `stage` los copia por su nombre, sin colisiones.
const DOCS: [&str; 5] = [
    "LICENSE",
    "README.md",
    "CHANGELOG.md",
    "CONTRIBUTING.md",
    "SECURITY.md",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Profile {
    Debug,
    Release,
}

impl Profile {
    fn dir(self) -> &'static str {
        match self {
            Self::Debug => "debug",
            Self::Release => "release",
        }
    }

    fn flag(self) -> &'static str {
        match self {
            Self::Debug => "--debug",
            Self::Release => "--release",
        }
    }
}

#[derive(Debug)]
struct Options {
    platforms: Vec<String>,
    profile: Profile,
    skip_build: bool,
    cross: bool,
    out: PathBuf,
}

fn host_os() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(windows) {
        "windows"
    } else {
        "linux"
    }
}

fn exe(os: &str) -> &'static str {
    if os == "windows" {
        "tlaxtrelom.exe"
    } else {
        "tlaxtrelom"
    }
}

/// Tripletas por defecto de cada plataforma; se pueden sustituir con variables
/// de entorno para paquetes de 32 bits, ARM de Linux o ABI futuras.
fn triple(platform: &str) -> String {
    let key = match platform {
        "linux" => "TLAXTRELOM_LINUX_TARGET",
        "windows" => "TLAXTRELOM_WINDOWS_TARGET",
        _ => "TLAXTRELOM_OHOS_TARGET",
    };
    let defect = match platform {
        "linux" => "x86_64-unknown-linux-gnu",
        "windows" => "x86_64-pc-windows-msvc",
        _ => "aarch64-unknown-linux-ohos",
    };
    env::var(key).unwrap_or_else(|_| defect.to_string())
}

/// Nombre de arquitectura dpkg para una tripleta de Linux.
fn deb_arch(target: &str) -> Result<String> {
    let arch = match target.split('-').next().unwrap_or_default() {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        "arm" => "armhf",
        "i686" | "i586" => "i386",
        "riscv64" => "riscv64",
        otro => return Err(format!("arquitectura dpkg desconocida para {otro}")),
    };
    Ok(arch.to_string())
}

/// Carpeta `bin/` del HAP para la ABI de la tripleta OHOS.
fn hap_abi(target: &str) -> Result<String> {
    let abi = match target.split('-').next().unwrap_or_default() {
        "aarch64" => "arm64-v8a",
        "x86_64" => "x86_64",
        otro => return Err(format!("ABI de HarmonyOS desconocida para {otro}")),
    };
    Ok(abi.to_string())
}

fn suffix(target: &str) -> String {
    target.replace('_', "-")
}

fn version() -> String {
    let dir = workspace();
    if let Ok(manifiesto) = fs::read_to_string(dir.join("Cargo.toml")) {
        for seccion in ["[workspace.package]", "[package]"] {
            if let Some(v) = version_of(&manifiesto, seccion) {
                return v;
            }
        }
    }
    if let Ok(v) = fs::read_to_string(dir.join("VERSION")) {
        let v = v.trim().to_string();
        if !v.is_empty() {
            return v;
        }
    }
    "0.0.0-dev".to_string()
}

/// `version = "x.y.z"` dentro de una sección concreta, ignorando las claves
/// `version.workspace = true` de los manifests hijos.
fn version_of(manifiesto: &str, seccion: &str) -> Option<String> {
    let despues = manifiesto.split_once(seccion)?.1;
    for linea in despues
        .lines()
        .take_while(|l| !l.trim_start().starts_with('['))
    {
        let Some(resto) = linea.trim().strip_prefix("version") else {
            continue;
        };
        let Some(valor) = resto
            .split('=')
            .nth(1)
            .and_then(|v| v.trim().strip_prefix('"'))
            .and_then(|v| v.split('"').next())
        else {
            continue;
        };
        return Some(valor.to_string());
    }
    None
}

fn workspace() -> PathBuf {
    let manifiesto = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifiesto
        .ancestors()
        .nth(2)
        .unwrap_or(manifiesto)
        .to_path_buf()
}

fn parse(args: &[String]) -> Result<Options> {
    let dir = workspace();
    let mut opts = Options {
        platforms: vec![host_os().to_string()],
        profile: Profile::Release,
        skip_build: false,
        cross: false,
        out: dir.join("dist"),
    };
    let mut explicit = false;
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        match arg {
            "--all" | "-a" => {
                opts.platforms = PLATFORMS.iter().map(|p| p.to_string()).collect();
                explicit = true;
            }
            "--platform" | "-p" => {
                let valor = args
                    .get(i + 1)
                    .ok_or("--platform pide un nombre".to_string())?;
                i += 1;
                if !PLATFORMS.contains(&valor.as_str()) {
                    return Err(format!(
                        "plataforma {valor} desconocida (linux, windows, harmonyos)"
                    ));
                }
                if !explicit {
                    opts.platforms.clear();
                    explicit = true;
                }
                if !opts.platforms.contains(valor) {
                    opts.platforms.push(valor.clone());
                }
            }
            "--debug" => opts.profile = Profile::Debug,
            "--skip-build" => opts.skip_build = true,
            "--cross" => opts.cross = true,
            "--out" => {
                let valor = args
                    .get(i + 1)
                    .ok_or("--out pide un directorio".to_string())?;
                i += 1;
                opts.out = if Path::new(valor).is_absolute() {
                    PathBuf::from(valor)
                } else {
                    dir.join(valor)
                };
            }
            "--help" | "-h" => {
                print_help();
                std::process::exit(0);
            }
            otro => return Err(format!("argumento {otro} desconocido")),
        }
        i += 1;
    }
    Ok(opts)
}

pub fn print_help() {
    println!(
        "cargo xtask package [--all | --platform <linux|windows|harmonyos>] [--debug] [--skip-build] [--cross] [--out <dir>]\n\
        \t--all: las tres plataformas; por defecto sólo la del host\n\
        \t--skip-build: reempaqueta lo que ya está en target/<profile>\n\
        \t--cross: construye además las tripletas de destino (pide rustup target add y su linker)\n\
        \t--out: directorio de salida (defecto: dist/)\n\
        cargo xtask package-check [archivo...]\n\
        \tvalida packaging/ sin compilar; sin argumentos revisa todo el arbol"
    );
}

// ── entradas ────────────────────────────────────────────────────────────────

/// Construye, ordena y archiva los paquetes pedidos.
pub fn run(args: &[String]) -> Result<()> {
    let opts = parse(args)?;
    let dir = workspace();
    let pkg = definitions(&dir)?;
    fs::create_dir_all(&opts.out)
        .map_err(|e| format!("no se pudo crear {}: {e}", opts.out.display()))?;

    println!(
        "empaquetando tlaxtrelom v{} para {} → {}",
        version(),
        opts.platforms.join(", "),
        opts.out.display()
    );

    let mut artefactos = Vec::new();
    let mut avisos = Vec::new();
    for platform in &opts.platforms {
        let binario = build(&dir, platform, &opts, &mut avisos)?;
        let raiz = stage(&dir, &pkg, platform, &binario, &opts, &mut avisos)?;
        artefactos.extend(archive(&raiz, platform, &opts, &mut avisos)?);
    }

    println!("\nartefactos:");
    for ruta in &artefactos {
        println!("  {} ({})", ruta.display(), human_size(path_len(ruta)));
    }
    for aviso in &avisos {
        println!("aviso: {aviso}");
    }
    if avisos.is_empty() {
        println!("sin avisos: todos los instaladores se construyeron");
    }
    Ok(())
}

/// Sin compilar: revisa que `packaging/` sea coherente con lo que el empaquetado
/// entrega y con la versión del workspace.
pub fn check(args: &[String]) -> Result<()> {
    let dir = workspace();
    let pkg = definitions(&dir)?;
    let explicit: Vec<String> = args
        .iter()
        .filter(|a| !a.starts_with('-'))
        .cloned()
        .collect();
    for flag in args {
        if flag == "--help" || flag == "-h" {
            print_help();
            return Ok(());
        }
    }

    let mut problemas: Vec<String> = Vec::new();
    if explicit.is_empty() {
        for platform in PLATFORMS {
            let sub = pkg.join(platform);
            if !sub.is_dir() {
                problemas.push(format!("falta {platform}"));
                continue;
            }
            check_platform(&sub, platform, &dir, &mut problemas);
        }
    } else {
        for ruta in &explicit {
            let abs = if Path::new(ruta).is_absolute() {
                PathBuf::from(ruta)
            } else {
                dir.join(ruta)
            };
            let texto = fs::read_to_string(&abs)
                .map_err(|e| format!("no se puede leer {}: {e}", abs.display()))?;
            let nombre = abs.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if let Err(e) = check_text(nombre, &texto, &abs, &dir) {
                problemas.push(e);
            }
        }
    }

    if !problemas.is_empty() {
        return Err(format!(
            "package-check: {} problema(s):\n  - {}",
            problemas.len(),
            problemas.join("\n  - ")
        ));
    }
    println!(
        "package-check: definiciones de linux, windows y harmonyos coherentes con tlaxtrelom v{}",
        version()
    );
    Ok(())
}

// ── construcción ────────────────────────────────────────────────────────────

/// Deja el ejecutable listo y devuelve la ruta del binario de la plataforma.
///
/// Un paquete de otra tripleta exige o un toolchain de compilación cruzada
/// (`--cross`, más `rustup target add` y su linker) o que el binario ya esté
/// construido en `target/<tripleta>/<perfil>/` o en el sitio que indique
/// `TLAXTRELOM_<PLATAFORMA>_TARGET`.
fn build(dir: &Path, platform: &str, opts: &Options, avisos: &mut Vec<String>) -> Result<PathBuf> {
    if opts.skip_build {
        return prebuilt(dir, platform, opts);
    }
    let destino = triple(platform);
    if destino != host_target() && !opts.cross {
        if let Ok(candidato) = prebuilt(dir, platform, opts) {
            avisos.push(format!(
                "usando binario existente de {platform} en {}",
                candidato.display()
            ));
            return Ok(candidato);
        }
        return Err(format!(
            "el paquete de {platform} pide la tripleta {destino} (la del host es {}); \
             instalala con `rustup target add {destino}` y su linker, relanzá con --cross, \
             o apontá un binario ya construido con TLAXTRELOM_{}_TARGET=<tripleta>",
            host_target(),
            platform_env(platform)
        ));
    }
    let mut cmd = Command::new(cargo());
    cmd.args(["build", "-p", "tlaxtrelom-core", "--target", &destino]);
    cmd.arg(opts.profile.flag());
    cmd.current_dir(dir);
    exec(&mut cmd, &format!("cargo build --target {destino}"))?;

    if platform == "linux" {
        exec(
            Command::new("make")
                .args(["native", "shaders"])
                .current_dir(dir),
            "make native shaders",
        )?;
    }
    if platform == "harmonyos" {
        bridge(dir, avisos)?;
    }
    prebuilt(dir, platform, opts)
}

/// El binario que ya está en el disco, sin compilar nada. Busca en la carpeta
/// de la tripleta y, si esa tripleta es la del host, también en
/// `target/<perfil>/`, que es donde cargo deja las compilaciones nativas.
fn prebuilt(dir: &Path, platform: &str, opts: &Options) -> Result<PathBuf> {
    let nombre = exe(platform);
    let perfiles = [
        opts.profile.dir(),
        if opts.profile == Profile::Debug {
            "release"
        } else {
            "debug"
        },
    ];
    let tripleta = triple(platform);
    for perfil in perfiles {
        let candidato = dir.join("target").join(&tripleta).join(perfil).join(nombre);
        if candidato.is_file() {
            return Ok(candidato);
        }
    }
    if tripleta == host_target() {
        for perfil in perfiles {
            let candidato = dir.join("target").join(perfil).join(nombre);
            if candidato.is_file() {
                return Ok(candidato);
            }
        }
    }
    Err(format!(
        "no hay binario de {platform}: se buscó {nombre} en target/{tripleta}/{{{}}} \
         y en target/{{{}}}/; quitá --skip-build para que se construya",
        perfiles.join(", "),
        perfiles.join(", ")
    ))
}

/// `e_machine` de una cabecera ELF: 62 es x86-64, 183 es aarch64.
fn elf_machine(path: &Path) -> Option<u16> {
    let mut cabecera = [0u8; 20];
    fs::File::open(path).ok()?.read_exact(&mut cabecera).ok()?;
    if &cabecera[0..4] != b"\x7fELF" {
        return None;
    }
    Some(u16::from_le_bytes([cabecera[18], cabecera[19]]))
}

/// `e_machine` que exige la ABI de un HAP.
fn abi_machine(abi: &str) -> Option<u16> {
    match abi {
        "arm64-v8a" => Some(183),
        "x86_64" => Some(62),
        _ => None,
    }
}

fn platform_env(platform: &str) -> &'static str {
    match platform {
        "linux" => "LINUX",
        "windows" => "WINDOWS",
        _ => "OHOS",
    }
}

fn host_target() -> String {
    let Ok(salida) = Command::new(rustc()).arg("-vV").output() else {
        return String::new();
    };
    let texto = String::from_utf8_lossy(&salida.stdout);
    for linea in texto.lines() {
        if let Some(rest) = linea.strip_prefix("host: ") {
            return rest.trim().to_string();
        }
    }
    String::new()
}

fn cargo() -> OsString {
    env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"))
}

fn rustc() -> OsString {
    env::var_os("RUSTC").unwrap_or_else(|| OsString::from("rustc"))
}

fn definitions(dir: &Path) -> Result<PathBuf> {
    let pkg = dir.join("packaging");
    if !pkg.is_dir() {
        return Err(format!("no existe el directorio {}", pkg.display()));
    }
    Ok(pkg)
}

fn require_file(path: &Path, que: &str, problemas: &mut Vec<String>) -> Option<String> {
    match fs::read_to_string(path) {
        Ok(texto) => Some(texto),
        Err(e) => {
            problemas.push(format!("falta {}: {e} ({que})", path.display()));
            None
        }
    }
}

// ── ordenación del árbol ────────────────────────────────────────────────────

/// Copia el ejecutable, los shaders, los assets y la documentación a
/// `dist/tlaxtrelom-<versión>-<sufijo>` y devuelve esa raíz.
fn stage(
    dir: &Path,
    pkg: &Path,
    platform: &str,
    binario: &Path,
    opts: &Options,
    avisos: &mut Vec<String>,
) -> Result<PathBuf> {
    let nombre = format!("tlaxtrelom-{}-{}", version(), suffix(&triple(platform)));
    let raiz = opts.out.join(&nombre);
    if raiz.exists() {
        fs::remove_dir_all(&raiz)
            .map_err(|e| format!("no se puede limpiar {}: {e}", raiz.display()))?;
    }
    fs::create_dir_all(&raiz).map_err(|e| format!("no se puede crear {}: {e}", raiz.display()))?;

    copy_dir(&dir.join("shaders"), &raiz.join("shaders"), false)?;
    copy_dir(&dir.join("assets"), &raiz.join("assets"), false)?;
    // Los documentos referencian los de su propia carpeta y, con `../`, los de la
    // raíz del paquete: copiar `docs/` entero deja los enlaces internos intactos.
    copy_dir(&dir.join("docs"), &raiz.join("docs"), false)?;
    for documento in DOCS {
        let origen = dir.join(documento);
        if origen.is_file() {
            let nombre = documento.rsplit('/').next().unwrap_or(documento);
            fs::copy(&origen, raiz.join(nombre))
                .map_err(|e| format!("no se puede copiar {}: {e}", origen.display()))?;
        } else if matches!(documento, "LICENSE" | "README.md") {
            return Err(format!("falta {} en el workspace", origen.display()));
        }
    }
    write_file(&raiz.join("VERSION"), &format!("{}\n", version()))?;
    write_source_stamp(dir, &raiz.join("BUILDINFO"))?;

    match platform {
        "windows" => {
            fs::copy(binario, raiz.join(exe("windows")))
                .map_err(|e| format!("no se puede copiar {}: {e}", binario.display()))?;
            copy_from(pkg, "windows/tlaxtrelom.cmd", &raiz.join("tlaxtrelom.cmd"))?;
            copy_from(pkg, "windows/tlaxtrelom.nsi", &raiz.join("tlaxtrelom.nsi"))?;
            let _ = copy_from(pkg, "windows/install.ps1", &raiz.join("install.ps1"));
        }
        "linux" => {
            let bin = raiz.join("bin");
            fs::create_dir_all(&bin).io()?;
            let destino = bin.join(exe("linux"));
            fs::copy(binario, &destino)
                .map_err(|e| format!("no se puede copiar {}: {e}", binario.display()))?;
            make_executable(&destino)?;
            // Un solo layout para el portable, el .deb y la instalación: los datos
            // viven en `share/tlaxtrelom/`, que es también la ruta relativa desde
            // `bin/` por la que el motor busca `demo_cluster.tlxg`.
            let datos = raiz.join("share/tlaxtrelom");
            fs::create_dir_all(&datos).io()?;
            for carpeta in ["assets", "shaders"] {
                fs::rename(raiz.join(carpeta), datos.join(carpeta))
                    .map_err(|e| format!("no se puede mover {carpeta} a share: {e}"))?;
            }
            copy_from(pkg, "linux/install.sh", &raiz.join("install.sh"))?;
            write_file(&bin.join("tlaxtrelom.sh"), &linux_launcher())?;
            make_executable(&bin.join("tlaxtrelom.sh"))?;
            make_executable(&raiz.join("install.sh"))?;
            let share = raiz.join("share");
            fs::create_dir_all(share.join("applications")).io()?;
            fs::create_dir_all(share.join("icons/hicolor/scalable/apps")).io()?;
            copy_from(
                pkg,
                "linux/tlaxtrelom.desktop",
                &share.join("applications/tlaxtrelom.desktop"),
            )?;
            copy_from(
                pkg,
                "linux/tlaxtrelom.svg",
                &share.join("icons/hicolor/scalable/apps/tlaxtrelom.svg"),
            )?;
            fs::create_dir_all(share.join("metainfo")).io()?;
            copy_from(
                pkg,
                "linux/org.tlaxtrelom.Engine.metainfo.xml",
                &share.join("metainfo/org.tlaxtrelom.Engine.metainfo.xml"),
            )?;
        }
        _ => {
            let bin = raiz.join("bin");
            fs::create_dir_all(&bin).io()?;
            let destino = bin.join(exe("linux"));
            if binario.exists() {
                fs::copy(binario, &destino).io()?;
                make_executable(&destino)?;
            }
            copy_from(
                pkg,
                "harmonyos/install-harmonyos.sh",
                &raiz.join("install.sh"),
            )?;
            make_executable(&raiz.join("install.sh"))?;
            let hap = raiz.join("hap");
            copy_dir(&pkg.join("harmonyos"), &hap, true)?;
            let abi = hap_abi(&triple("harmonyos"))?;
            let libs = hap.join("entry/libs").join(&abi);
            fs::create_dir_all(&libs).io()?;
            if let Some(puente) = puente_para_abi(dir, &abi, avisos) {
                fs::copy(&puente, libs.join("libtlaxtrelom_harmonyos.so")).io()?;
            }
        }
    }
    println!("  árbol de {platform}: {}", raiz.display());
    Ok(raiz)
}

fn bridge_lib(dir: &Path) -> Option<PathBuf> {
    let isla = dir.join("native/tlaxtrelom-harmonyos");
    for carpeta in ["build-ohos", "build"] {
        let candidato = isla.join(carpeta).join("libtlaxtrelom_harmonyos.so");
        if candidato.is_file() {
            return Some(candidato);
        }
    }
    None
}

/// La `.so` del puente que corresponde meter en `entry/libs/<abi>/`, o `None`
/// con un aviso si no existe o si es de otra arquitectura. Un HAP con una `.so`
/// de la ABI equivocada instala mal y el fallo aparece en el dispositivo, no
/// acá: es más barato descartarla ahora.
fn puente_para_abi(dir: &Path, abi: &str, avisos: &mut Vec<String>) -> Option<PathBuf> {
    let Some(puente) = bridge_lib(dir) else {
        avisos.push(
            "el HAP va sin libtlaxtrelom_harmonyos.so: corre `make harmonyos` (host) o \
             definí OHOS_NDK_HOME y construyla antes de empaquetar"
                .to_string(),
        );
        return None;
    };
    match (elf_machine(&puente), abi_machine(abi)) {
        (Some(actual), Some(esperada)) if actual != esperada => {
            avisos.push(format!(
                "se descarta la puente del HAP: {} es ELF e_machine {actual} y la ABI {abi} \
                 pide {esperada}; construyla con el NDK de HarmonyOS \
                 (OHOS_NDK_HOME=<ndk> cargo xtask package --platform harmonyos)",
                puente.display()
            ));
            None
        }
        _ => Some(puente),
    }
}

/// Compila la isla de HarmonyOS con el NDK de OHOS si está disponible.
fn bridge(dir: &Path, avisos: &mut Vec<String>) -> Result<()> {
    let isla = dir.join("native/tlaxtrelom-harmonyos");
    let clang = env::var("OHOS_NDK_HOME")
        .ok()
        .map(|n| Path::new(&n).join("llvm/bin/clang++"))
        .filter(|p| p.is_file());
    match clang {
        Some(compilador) => exec(
            Command::new("make")
                .current_dir(&isla)
                .arg("BUILD_DIR=build-ohos")
                .arg(format!("CXX={}", compilador.display()))
                .arg("libtlaxtrelom_harmonyos.so"),
            &format!(
                "make -C native/tlaxtrelom-harmonyos BUILD_DIR=build-ohos ({} del NDK)",
                compilador.display()
            ),
        ),
        None => {
            if bridge_lib(dir).is_some() {
                avisos.push(
                    "sin OHOS_NDK_HOME no se cruzó el puente con el NDK; el HAP sólo lleva \
                     la .so de la carpeta build-ohos o build si su ABI coincide con la del \
                     paquete. Para la variante de arm64: instala el NDK de HarmonyOS y \
                     definí OHOS_NDK_HOME"
                        .to_string(),
                );
                Ok(())
            } else {
                Err(
                    "no hay libtlaxtrelom_harmonyos.so: define OHOS_NDK_HOME (con llvm/bin/clang++) \
                     o corre antes `make harmonyos`"
                        .to_string(),
                )
            }
        }
    }
}

fn linux_launcher() -> String {
    "#!/bin/sh\n\
     # SPDX-License-Identifier: MIT\n\
     # Copyright (c) 2026 angiraulene1-create\n\
     # Arranque portable: ejecuta el binario que está junto a este guion.\n\
     # El motor ya busca assets/ y shaders/ respecto al ejecutable, así que no\n\
     # hace falta cambiar de directorio ni pasar rutas de recursos.\n\
     set -e\n\
     here=$(CDPATH= cd -- \"$(dirname -- \"$0\")\" && pwd)\n\
     exec \"$here/tlaxtrelom\" \"$@\"\n"
        .to_string()
}

fn copy_from(pkg: &Path, relativo: &str, destino: &Path) -> Result<()> {
    let origen = pkg.join(relativo);
    if let Some(padre) = destino.parent() {
        fs::create_dir_all(padre).io()?;
    }
    fs::copy(&origen, destino)
        .map_err(|e| format!("no se puede copiar {}: {e}", origen.display()))?;
    Ok(())
}

fn write_file(destino: &Path, contenido: &str) -> Result<()> {
    if let Some(padre) = destino.parent() {
        fs::create_dir_all(padre).io()?;
    }
    fs::write(destino, contenido)
        .map_err(|e| format!("no se puede escribir {}: {e}", destino.display()))
}

fn make_executable(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let permisos = fs::metadata(path)
            .map_err(|e| format!("no se puede inspeccionar {}: {e}", path.display()))?;
        let mut modo = permisos.permissions();
        modo.set_mode(0o755);
        fs::set_permissions(path, modo)
            .map_err(|e| format!("no se puede marcar {} como ejecutable: {e}", path.display()))?;
    }
    Ok(())
}

/// Ficha de origen del paquete: sin commit (el arbol descargado como zip no lo
/// tiene) queda `commit=desconocido` en vez de un valor falso.
fn write_source_stamp(dir: &Path, destino: &Path) -> Result<()> {
    let commit = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .current_dir(dir)
        .output()
        .ok()
        .filter(|s| s.status.success())
        .map(|s| String::from_utf8_lossy(&s.stdout).trim().to_string())
        .unwrap_or_else(|| "desconocido".to_string());
    write_file(
        destino,
        &format!(
            "version={}\ncommit={commit}\nempaquetado={}\n",
            version(),
            now()
        ),
    )
}

fn now() -> String {
    Command::new("date")
        .arg("-u")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|_| "desconocida".to_string())
}

fn copy_dir(origen: &Path, destino: &Path, vaciar_estilo: bool) -> Result<()> {
    let entrada =
        fs::read_dir(origen).map_err(|e| format!("no se puede leer {}: {e}", origen.display()))?;
    fs::create_dir_all(destino).io()?;
    for item in entrada {
        let item = item.map_err(|e| e.to_string())?;
        let nombre = item.file_name();
        let nombre = nombre.to_string_lossy().to_string();
        if vaciar_estilo && (nombre == "build" || nombre == "build-ohos" || nombre.ends_with(".md"))
        {
            continue;
        }
        let ruta = item.path();
        if ruta.is_dir() {
            copy_dir(&ruta, &destino.join(nombre), vaciar_estilo)?;
        } else {
            fs::copy(&ruta, destino.join(nombre))
                .map_err(|e| format!("no se puede copiar {}: {e}", ruta.display()))?;
        }
    }
    Ok(())
}

/// Nombre del árbol con un sufijo, sin romper los puntos de la versión.
fn con_sufijo(raiz: &Path, sufijo: &str) -> PathBuf {
    let nombre = raiz
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    raiz.with_file_name(format!("{nombre}-{sufijo}"))
}

fn path_len(path: &Path) -> u64 {
    fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

fn human_size(bytes: u64) -> String {
    const UNIDADES: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut valor = bytes as f64;
    let mut i = 0;
    while valor >= 1024.0 && i + 1 < UNIDADES.len() {
        valor /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{bytes} {}", UNIDADES[0])
    } else {
        format!("{:.1} {}", valor, UNIDADES[i])
    }
}

fn exec(cmd: &mut Command, etiqueta: &str) -> Result<()> {
    println!("==> {etiqueta}");
    let estado = cmd
        .status()
        .map_err(|e| format!("no se puede ejecutar {etiqueta}: {e}"))?;
    if !estado.success() {
        return Err(format!("{etiqueta} salió con {estado}"));
    }
    Ok(())
}

fn which(herramienta: &str) -> Option<OsString> {
    env::var_os("PATH").and_then(|caminos| {
        env::split_paths(&caminos).find_map(|mut carpeta| {
            carpeta.push(format!(
                "{herramienta}{}",
                if cfg!(windows) { ".exe" } else { "" }
            ));
            carpeta.is_file().then_some(carpeta.into_os_string())
        })
    })
}

// ── artefactos ──────────────────────────────────────────────────────────────

/// Comprime el árbol ya ordenado y devuelve los archivos generados.
fn archive(
    raiz: &Path,
    platform: &str,
    opts: &Options,
    avisos: &mut Vec<String>,
) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    match platform {
        "linux" => {
            out.push(tar_gz(raiz, opts)?);
            match make_deb(raiz, opts, avisos) {
                Ok(deb) => out.push(deb),
                Err(e) => avisos.push(e),
            }
        }
        "windows" => {
            out.push(zip_dir(raiz, opts)?);
            match make_setup_exe(raiz, opts) {
                Ok(setup) => out.push(setup),
                Err(e) => avisos.push(e),
            }
        }
        _ => out.push(zip_dir(raiz, opts)?),
    }
    Ok(out)
}

/// `dist/<árbol>.tar.gz`, con permisos Unix dentro (por eso no se hace zip).
fn tar_gz(raiz: &Path, opts: &Options) -> Result<PathBuf> {
    let nombre = raiz
        .file_name()
        .ok_or("el árbol no tiene nombre".to_string())?
        .to_string_lossy()
        .into_owned();
    let archivo = raiz.with_file_name(format!("{nombre}.tar.gz"));
    let nombre = raiz
        .file_name()
        .ok_or("el árbol no tiene nombre".to_string())?;
    exec(
        Command::new("tar")
            .args(["-czf"])
            .arg(&archivo)
            .arg("-C")
            .arg(&opts.out)
            .arg(nombre),
        &format!("tar -czf {}", archivo.display()),
    )?;
    Ok(archivo)
}

/// Zip determinista escrito con la librería estándar de Python 3, que está en
/// todas las plataformas objetivo salvo sistemas mínimos.
fn zip_dir(raiz: &Path, opts: &Options) -> Result<PathBuf> {
    if which("python3").is_none() {
        return Err(format!(
            "no hay python3 para comprimir {}: instala python3 o distribuye el árbol suelto",
            raiz.display()
        ));
    }
    let nombre = raiz
        .file_name()
        .ok_or("el árbol no tiene nombre".to_string())?
        .to_string_lossy()
        .into_owned();
    let archivo = opts.out.join(format!("{nombre}.zip"));
    let guion = "
import os, sys, zipfile
raiz, destino = sys.argv[1], sys.argv[2]
with zipfile.ZipFile(destino, 'w', zipfile.ZIP_DEFLATED, compresslevel=9) as z:
    for carpeta, _, archivos in sorted(os.walk(raiz)):
        for archivo in sorted(archivos):
            origen = os.path.join(carpeta, archivo)
            z.write(origen, os.path.relpath(origen, os.path.dirname(raiz)))
";
    exec(
        Command::new("python3")
            .args(["-c", guion])
            .arg(raiz)
            .arg(&archivo),
        &format!("zip → {}", archivo.display()),
    )?;
    Ok(archivo)
}

/// Envuelve el árbol portable en FHS y lo empaqueta como `.deb`.
///
/// Usa `dpkg-deb` si está; si no, escribe el contenedor `ar` a mano
/// (`debian-binary` + `control.tar.gz` + `data.tar.xz`), que es lo mismo que
/// produce dpkg.
fn make_deb(raiz: &Path, opts: &Options, avisos: &mut Vec<String>) -> Result<PathBuf> {
    if cfg!(windows) {
        return Err("el .deb se construye desde Linux o macOS".to_string());
    }
    let target = triple("linux");
    let arch = deb_arch(&target)?;
    let plantilla = fs::read_to_string(definitions(&workspace())?.join("linux/control"))
        .map_err(|e| format!("no se puede leer packaging/linux/control: {e}"))?;

    let raiz_deb = con_sufijo(raiz, "deb-root");
    if raiz_deb.exists() {
        fs::remove_dir_all(&raiz_deb).io()?;
    }
    let usr = raiz_deb.join("usr");
    fs::create_dir_all(usr.join("bin")).io()?;
    let doc = usr.join("share/doc/tlaxtrelom");
    fs::create_dir_all(&doc).io()?;
    fs::copy(raiz.join("bin/tlaxtrelom"), usr.join("bin/tlaxtrelom")).io()?;
    make_executable(&usr.join("bin/tlaxtrelom"))?;
    // El árbol portable de Linux ya es FHS, así que `share/` se copia tal cual.
    // Se copia, no se mueve: el árbol de `dist/` tiene que quedar entero después.
    copy_dir(&raiz.join("share"), &usr.join("share"), false)?;
    for documento in DOCS {
        if raiz.join(documento).is_file() {
            fs::copy(raiz.join(documento), doc.join(documento)).io()?;
        }
    }
    // Debian espera un `copyright` con la licencia en texto claro.
    if raiz.join("LICENSE").is_file() {
        fs::copy(raiz.join("LICENSE"), doc.join("copyright")).io()?;
    }
    fs::copy(raiz.join("VERSION"), doc.join("VERSION")).io()?;
    copy_dir(&raiz.join("docs"), &doc.join("docs"), false)?;

    let size = install_size(&raiz_deb);
    let control = fill_placeholders(
        &plantilla,
        &[
            ("VERSION", &version()),
            ("ARCH", &arch),
            ("SIZE", &size.to_string()),
        ],
    );
    let control_dir = con_sufijo(&raiz_deb, "control");
    fs::create_dir_all(&control_dir).io()?;
    write_file(&control_dir.join("control"), &control)?;
    let pkg = definitions(&workspace())?;
    for script in ["postinst", "prerm"] {
        copy_from(&pkg, &format!("linux/{script}"), &control_dir.join(script))?;
        make_executable(&control_dir.join(script))?;
    }
    let md5s = md5sums(&raiz_deb);
    write_file(&control_dir.join("md5sums"), &md5s)?;

    let archivo = opts
        .out
        .join(format!("tlaxtrelom_{}_{}.deb", version(), arch));
    if let Some(dpkg) = which("dpkg-deb") {
        exec(
            Command::new(dpkg).args(["-b"]).arg(&raiz_deb).arg(&archivo),
            &format!("dpkg-deb -b {} → {}", raiz_deb.display(), archivo.display()),
        )?;
        let _ = fs::remove_dir_all(&control_dir);
    } else {
        make_deb_ar(&raiz_deb, &control_dir, &archivo)?;
        avisos.push(
            "el .deb se armó con ar+tar: dpkg-deb no está instalado, pero es el mismo contenedor"
                .to_string(),
        );
    }
    let _ = fs::remove_dir_all(&control_dir);
    // El árbol FHS es intermedio: el .deb ya lo contiene.
    let _ = fs::remove_dir_all(&raiz_deb);
    Ok(archivo)
}

/// Contenedor `.deb` sin dpkg: `debian-binary`, `control.tar.gz`, `data.tar.xz`.
fn make_deb_ar(data: &Path, control: &Path, archivo: &Path) -> Result<()> {
    if archivo.exists() {
        fs::remove_file(archivo).io()?;
    }
    let temporal = archivo.with_extension("parts");
    fs::create_dir_all(&temporal).io()?;
    let resultado = (|| -> Result<()> {
        write_file(&temporal.join("debian-binary"), "2.0\n")?;
        exec(
            Command::new("tar")
                .args(["-czf"])
                .arg(temporal.join("control.tar.gz"))
                .arg("-C")
                .arg(control)
                .args(["./control", "./md5sums", "./postinst", "./prerm"]),
            "tar control.tar.gz",
        )?;
        exec(
            Command::new("tar")
                .args(["-cJf"])
                .arg(temporal.join("data.tar.xz"))
                .arg("-C")
                .arg(data)
                .arg("./usr"),
            "tar data.tar.xz",
        )?;
        let ar =
            which("ar").ok_or("no hay ni dpkg-deb ni ar para construir el .deb".to_string())?;
        let mut cmd = Command::new(ar);
        cmd.args(["rcs"]).arg(archivo).args([
            temporal.join("debian-binary"),
            temporal.join("control.tar.gz"),
            temporal.join("data.tar.xz"),
        ]);
        exec(&mut cmd, "ar rcs (contenedor .deb)")?;
        Ok(())
    })();
    let _ = fs::remove_dir_all(&temporal);
    resultado
}

/// Instalador de Windows: sólo si NSIS está en el PATH.
fn make_setup_exe(raiz: &Path, opts: &Options) -> Result<PathBuf> {
    let makensis = which("makensis").ok_or_else(|| {
        format!(
            "sin makensis no se genera el instalador de Windows; el guion \
             tlaxtrelom.nsi va dentro del portable y se puede correr con \
             `makensis -DVERSION={} tlaxtrelom.nsi` (winget install NSIS.NSIS)",
            version()
        )
    })?;
    let guion = raiz.join("tlaxtrelom.nsi");
    exec(
        Command::new(makensis)
            .arg(format!("-DVERSION={}", version()))
            .arg(&guion),
        &format!("makensis {}", guion.display()),
    )?;
    let generado = raiz.join(format!("tlaxtrelom-setup-{}.exe", version()));
    if !generado.is_file() {
        return Err(format!(
            "makensis terminó sin producir {}",
            generado.display()
        ));
    }
    let destino = opts.out.join(
        generado
            .file_name()
            .ok_or("el instalador no tiene nombre".to_string())?,
    );
    if destino != generado {
        fs::rename(&generado, &destino).io()?;
    }
    Ok(destino)
}

/// Suma de `Installed-Size` en KiB, como dpkg: sólo ficheros regulares.
fn install_size(raiz: &Path) -> u64 {
    fn andar(carpeta: &Path, total: &mut u64) {
        if let Ok(entradas) = fs::read_dir(carpeta) {
            for item in entries_ok(entradas) {
                let ruta = item.path();
                if ruta.is_dir() {
                    andar(&ruta, total);
                } else if let Ok(m) = fs::metadata(&ruta) {
                    *total += m.len().div_ceil(1024);
                }
            }
        }
    }
    let mut total = 0;
    andar(raiz, &mut total);
    total
}

fn entries_ok(lectura: fs::ReadDir) -> Vec<fs::DirEntry> {
    lectura.flatten().collect()
}

/// Lista `md5sums` con rutas relativas a la raíz de datos, como dpkg-deb.
fn md5sums(raiz: &Path) -> String {
    let mut lineas = Vec::new();
    let mut archivos = Vec::new();
    recolectar(raiz, &mut archivos);
    archivos.sort();
    for ruta in archivos {
        if let Ok(sumas) = md5_file(&ruta) {
            let relativo = ruta
                .strip_prefix(raiz)
                .unwrap_or(&ruta)
                .to_string_lossy()
                .replace('\\', "/");
            lineas.push(format!("{sumas}  {relativo}"));
        }
    }
    lineas.join("\n") + "\n"
}

fn md5_file(path: &Path) -> Result<String> {
    // Sin crate de hashes: se delega en coreutils/md5sum, y si no está se omite
    // la línea (los md5sums de un paquete no firmado son informativos).
    let bin = which("md5sum")
        .or_else(|| which("md5"))
        .ok_or("sin md5sum".to_string())?;
    let salida = Command::new(bin)
        .arg(path)
        .output()
        .map_err(|e| e.to_string())?;
    if !salida.status.success() {
        return Err("md5 falló".to_string());
    }
    String::from_utf8_lossy(&salida.stdout)
        .split_whitespace()
        .next()
        .map(str::to_string)
        .ok_or("salida de md5 vacía".to_string())
}

fn recolectar(carpeta: &Path, out: &mut Vec<PathBuf>) {
    if let Ok(lectura) = fs::read_dir(carpeta) {
        for item in entries_ok(lectura) {
            let ruta = item.path();
            if ruta.is_dir() {
                recolectar(&ruta, out);
            } else {
                out.push(ruta);
            }
        }
    }
}

// ── validación de definiciones ──────────────────────────────────────────────

/// Ficheros que `stage` pone en cualquier árbol, sea cual sea la plataforma.
const COMUNES: [&str; 8] = [
    "LICENSE",
    "README.md",
    "CHANGELOG.md",
    "CONTRIBUTING.md",
    "SECURITY.md",
    "VERSION",
    "BUILDINFO",
    "docs",
];

/// Lo que existe de verdad en `dist/tlaxtrelom-<versión>-<sufijo>/`.
fn staged_paths(platform: &str) -> BTreeSet<String> {
    let especifico: &[&str] = match platform {
        "linux" => &[
            "bin/tlaxtrelom",
            "bin/tlaxtrelom.sh",
            "install.sh",
            "share/tlaxtrelom/assets",
            "share/tlaxtrelom/shaders",
            "share/applications",
            "share/icons",
            "share/metainfo",
        ],
        "windows" => &[
            "tlaxtrelom.exe",
            "tlaxtrelom.cmd",
            "tlaxtrelom.nsi",
            "assets",
            "shaders",
        ],
        _ => &["bin/tlaxtrelom", "install.sh", "hap", "assets", "shaders"],
    };
    especifico
        .iter()
        .chain(COMUNES.iter())
        .map(|p| (*p).to_string())
        .collect()
}

/// Rutas relativas que el instalador de Windows espera encontrar donde se lance
/// `makensis` (la raíz del árbol empaquetado o su carpeta de artefactos).
fn nsi_file_paths(texto: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for (indice, linea) in texto.lines().enumerate() {
        if indice < 2 && linea.starts_with(";") {
            continue; // cabecera SPDX y comentario de uso
        }
        let recortada = linea.trim_start();
        if !(recortada.starts_with("File") || recortada.starts_with("!system")) {
            continue;
        }
        for trozo in recortada.split('"').skip(1).step_by(2) {
            let limpio = trozo
                .trim_start_matches("/r ")
                .trim_start_matches("/r")
                .trim()
                .to_string();
            if limpio.is_empty() || limpio.starts_with('$') {
                continue;
            }
            let raiz = limpio
                .split(['\\', '/'])
                .next()
                .unwrap_or_default()
                .to_string();
            let raiz = raiz
                .trim_end_matches("*.*")
                .trim_end_matches('*')
                .to_string();
            if !raiz.is_empty() {
                out.insert(raiz);
            }
        }
    }
    out
}

/// Qué rutas del guion faltan en la lista de lo que se empaqueta.
fn missing_staged(rutas: &BTreeSet<String>, platform: &str) -> Vec<String> {
    let esperadas = staged_paths(platform);
    let mut faltan = Vec::new();
    for ruta in rutas {
        let clave = ruta.trim_end_matches('\\').trim_end_matches('/');
        let cubierta = esperadas
            .iter()
            .any(|e| e == clave || e.starts_with(&format!("{clave}/")));
        if !cubierta {
            faltan.push(clave.to_string());
        }
    }
    faltan
}

/// Sustituciones de plantilla estilo CMake: `${NOMBRE}`.
fn fill_placeholders(texto: &str, valores: &[(&str, &str)]) -> String {
    let mut out = texto.to_string();
    for (clave, valor) in valores {
        out = out.replace(&format!("${{{clave}}}"), valor);
    }
    out
}

/// Quita los comentarios `//` de un JSON5 para poder parsearlo con serde_json.
/// Sólo se permiten a nivel de línea, que es como están escritos en `packaging/`.
fn strip_json5_comments(texto: &str) -> String {
    texto
        .lines()
        .filter(|linea| !linea.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn json5_parse(texto: &str) -> Result<()> {
    if !json5_balanced(&strip_json5_comments(texto)) {
        return Err("llaves o corchetes desbalanceados".to_string());
    }
    Ok(())
}

/// Saltar espacios en blanco a partir de un índice de bytes (claves ASCII).
fn skip_ws(texto: &str, desde: usize) -> usize {
    let bytes = texto.as_bytes();
    let mut i = desde;
    while i < bytes.len() && matches!(bytes[i], b' ' | b'\t' | b'\n' | b'\r') {
        i += 1;
    }
    i
}

/// Todos los valores de cadena de `"clave": "valor"` en un JSON5.
///
/// A cambio de no traer un parser, sólo entiende cadenas simples: suficiente
/// para los manifiestos de `packaging/harmonyos`, que se escriben a mano.
fn json5_strings(texto: &str, clave: &str) -> Vec<String> {
    let patron = format!("\"{clave}\"");
    let mut out = Vec::new();
    let mut desde = 0;
    while let Some(pos) = texto[desde..].find(&patron) {
        let mut i = skip_ws(texto, desde + pos + patron.len());
        if !starts_at(texto, i, ':') {
            desde += pos + patron.len();
            continue;
        }
        i = skip_ws(texto, i + 1);
        if !starts_at(texto, i, '"') {
            desde = i;
            continue;
        }
        i += 1;
        match texto[i..].find('"') {
            Some(fin) => {
                out.push(texto[i..i + fin].to_string());
                desde = i + fin + 1;
            }
            None => break,
        }
    }
    out
}

/// Primer valor de cadena de una clave, si existe.
fn json5_string(texto: &str, clave: &str) -> Option<String> {
    json5_strings(texto, clave).into_iter().next()
}

/// Subcadena balanceada que empieza en el `{` o `[` tras `"clave":`.
fn json5_block(texto: &str, clave: &str) -> Option<String> {
    let patron = format!("\"{clave}\"");
    let pos = texto.find(&patron)?;
    let mut i = skip_ws(texto, pos + patron.len());
    if !starts_at(texto, i, ':') {
        return None;
    }
    i = skip_ws(texto, i + 1);
    let cierre = find_matching(texto, i)?;
    texto.get(i..=cierre).map(str::to_string)
}

/// Elementos de cadena del array que sigue a `"clave":`.
fn json5_array(texto: &str, clave: &str) -> Vec<String> {
    let Some(bloque) = json5_block(texto, clave) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut desde = 0;
    while let Some(dentro) = bloque[desde..].find('"') {
        let inicio = desde + dentro + 1;
        match bloque[inicio..].find('"') {
            Some(fin) => {
                out.push(bloque[inicio..inicio + fin].to_string());
                desde = inicio + fin + 1;
            }
            None => break,
        }
    }
    out
}

fn starts_at(texto: &str, i: usize, c: char) -> bool {
    texto.get(i..).is_some_and(|resto| resto.starts_with(c))
}

/// Índice del `}` o `]` que cierra el que abre en `apertura`, ignorando
/// el contenido de las cadenas.
fn find_matching(texto: &str, apertura: usize) -> Option<usize> {
    let abierto = *texto.as_bytes().get(apertura)?;
    let esperado = match abierto {
        b'{' => b'}',
        b'[' => b']',
        _ => return None,
    };
    let mut profundidad = 0usize;
    let mut en_cadena = false;
    let mut escapado = false;
    for (i, byte) in texto.as_bytes().iter().enumerate().skip(apertura) {
        if en_cadena {
            if escapado {
                escapado = false;
            } else if *byte == b'\\' {
                escapado = true;
            } else if *byte == b'"' {
                en_cadena = false;
            }
            continue;
        }
        match byte {
            b'"' => en_cadena = true,
            b'{' | b'[' => profundidad += 1,
            b'}' | b']' => {
                profundidad = profundidad.checked_sub(1)?;
                if profundidad == 0 {
                    return (*byte == esperado).then_some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// Balance de llaves y corchetes fuera de cadenas y comentarios de línea.
fn json5_balanced(texto: &str) -> bool {
    let mut pila: Vec<u8> = Vec::new();
    let mut en_cadena = false;
    let mut escapado = false;
    let mut comentario = false;
    let bytes = texto.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let byte = bytes[i];
        if comentario {
            if byte == b'\n' {
                comentario = false;
            }
            i += 1;
            continue;
        }
        if en_cadena {
            if escapado {
                escapado = false;
            } else if byte == b'\\' {
                escapado = true;
            } else if byte == b'"' {
                en_cadena = false;
            }
            i += 1;
            continue;
        }
        match byte {
            b'"' => en_cadena = true,
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                comentario = true;
                i += 2;
                continue;
            }
            b'{' | b'[' => pila.push(byte),
            b'}' | b']' => {
                let abre = match byte {
                    b'}' => b'{',
                    _ => b'[',
                };
                if pila.pop() != Some(abre) {
                    return false;
                }
            }
            _ => {}
        }
        i += 1;
    }
    pila.is_empty() && !en_cadena
}

/// Referencias de recursos de HarmonyOS: `"$media:app_icon"`, `"$string:x"`, etc.
fn extract_resource_refs(texto: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut desde = 0;
    while let Some(dentro) = texto[desde..].find("\"$") {
        let inicio = desde + dentro + 2;
        let Some(fin) = texto[inicio..].find(':') else {
            break;
        };
        let tipo = &texto[inicio..inicio + fin];
        let despues = inicio + fin + 1;
        let Some(cierre) = texto[despues..].find('"') else {
            break;
        };
        out.insert(format!("{tipo}:{}", &texto[despues..despues + cierre]));
        desde = despues + cierre + 1;
    }
    out
}

fn check_platform(sub: &Path, platform: &str, dir: &Path, problemas: &mut Vec<String>) {
    require_file(
        &sub.join("README.md"),
        "documentación de la plataforma",
        problemas,
    );
    match platform {
        "linux" => check_linux(sub, problemas),
        "windows" => check_windows(sub, problemas),
        _ => check_harmonyos(sub, dir, problemas),
    }
}

fn check_linux(sub: &Path, problemas: &mut Vec<String>) {
    let clave = [
        "Package:",
        "Version:",
        "Architecture:",
        "Maintainer:",
        "Description:",
    ];
    if let Some(texto) = require_file(&sub.join("control"), "campo de control del .deb", problemas)
    {
        for k in clave {
            if !texto.contains(k) {
                problemas.push(format!("control: falta el campo {k}"));
            }
        }
        for marcador in ["${VERSION}", "${ARCH}", "${SIZE}"] {
            if !texto.contains(marcador) {
                problemas.push(format!(
                    "control: {marcador} no está, el .deb saldría incompleto"
                ));
            }
        }
    }
    for script in ["postinst", "prerm", "install.sh"] {
        if let Some(texto) = require_file(&sub.join(script), "guion del instalador", problemas) {
            if !texto.starts_with("#!/bin/sh") {
                problemas.push(format!("{script}: debe empezar con #!/bin/sh"));
            }
            if script == "install.sh" && !texto.contains("share/tlaxtrelom") {
                problemas.push("install.sh: no instala share/tlaxtrelom".to_string());
            }
        }
    }
    if let Some(texto) = require_file(
        &sub.join("tlaxtrelom.desktop"),
        "entrada de menú",
        problemas,
    ) {
        if let Err(e) = check_text("tlaxtrelom.desktop", &texto, sub, &workspace()) {
            problemas.push(e);
        }
    }
    if let Some(texto) = require_file(
        &sub.join("org.tlaxtrelom.Engine.metainfo.xml"),
        "metainfo AppStream",
        problemas,
    ) {
        if !texto.contains("<id>org.tlaxtrelom.Engine</id>") {
            problemas.push("metainfo: el <id> debe ser org.tlaxtrelom.Engine".to_string());
        }
        if !texto.contains(&format!("<release version=\"{}\"", version())) {
            problemas.push(format!(
                "metainfo: no anuncia la versión {} del workspace",
                version()
            ));
        }
    }
    if let Some(texto) = require_file(&sub.join("tlaxtrelom.svg"), "icono", problemas) {
        if !texto.contains("<svg") {
            problemas.push("tlaxtrelom.svg: no es un SVG".to_string());
        }
    }
}

fn check_windows(sub: &Path, problemas: &mut Vec<String>) {
    if let Some(nsi) = require_file(&sub.join("tlaxtrelom.nsi"), "instalador NSIS", problemas) {
        if let Err(e) = check_text("tlaxtrelom.nsi", &nsi, sub, &workspace()) {
            problemas.push(e);
        }
        for ruta in nsi_file_paths(&nsi) {
            if missing_staged(&[ruta.clone()].into_iter().collect(), "windows").contains(&ruta) {
                problemas.push(format!(
                    "tlaxtrelom.nsi: instala {ruta}, que el empaquetado de windows no entrega"
                ));
            }
        }
        if !nsi.contains("Section \"Uninstall\"") {
            problemas
                .push("tlaxtrelom.nsi: sin sección Uninstall no hay desinstalación".to_string());
        }
    }
    if let Some(cmd) = require_file(&sub.join("tlaxtrelom.cmd"), "arranque portable", problemas) {
        if !cmd.starts_with("@echo off") {
            problemas.push("tlaxtrelom.cmd: debe empezar con @echo off".to_string());
        }
    }
}

fn check_harmonyos(sub: &Path, raiz: &Path, problemas: &mut Vec<String>) {
    let app = match require_file(
        &sub.join("app.json5"),
        "manifiesto de aplicación",
        problemas,
    ) {
        Some(texto) => texto,
        None => return,
    };
    if let Err(e) = json5_parse(&app) {
        problemas.push(format!("app.json5: {e}"));
        return;
    }
    let cotejar = |clave: &str, esperado: &str, problemas: &mut Vec<String>| {
        let actual = json5_string(&app, clave).unwrap_or_default();
        if actual != esperado {
            problemas.push(format!(
                "app.json5: {clave} = \"{actual}\", debería ser \"{esperado}\""
            ));
        }
    };
    cotejar("versionName", &version(), problemas);
    cotejar("bundleName", "com.tlaxtrelom.engine", problemas);

    let ruta_modulo = sub.join("entry/src/main/module.json5");
    let modulo = match require_file(&ruta_modulo, "manifiesto del módulo", problemas) {
        Some(texto) => texto,
        None => return,
    };
    if let Err(e) = json5_parse(&modulo) {
        problemas.push(format!("module.json5: {e}"));
        return;
    }
    let main = json5_string(&modulo, "mainElement").unwrap_or_default();
    let abilities = json5_block(&modulo, "abilities").unwrap_or_default();
    if !json5_strings(&abilities, "name").contains(&main) {
        problemas.push(format!(
            "module.json5: mainElement \"{main}\" no corresponde a ninguna ability"
        ));
    }
    for entrada in json5_strings(&abilities, "srcEntry") {
        let archivo = sub
            .join("entry/src/main")
            .join(entrada.trim_start_matches("./"));
        if !archivo.is_file() {
            problemas.push(format!(
                "module.json5: srcEntry {} no existe",
                archivo.display()
            ));
        }
    }

    let recursos = sub.join("entry/src/main/resources/base");
    for referencia in extract_resource_refs(&modulo) {
        let (tipo, nombre) = referencia.split_once(':').unwrap_or(("", ""));
        let existe = match tipo {
            "string" | "color" => json5_strings(
                &fs::read_to_string(recursos.join(format!("element/{tipo}.json")))
                    .unwrap_or_default(),
                "name",
            )
            .contains(&nombre.to_string()),
            "profile" => recursos
                .join("profile")
                .join(format!("{nombre}.json"))
                .is_file(),
            "media" => ["png", "svg", "webp"].iter().any(|ext| {
                recursos
                    .join("media")
                    .join(format!("{nombre}.{ext}"))
                    .is_file()
            }),
            otro => {
                problemas.push(format!("module.json5: recurso \"{otro}\" desconocido"));
                false
            }
        };
        if !existe {
            problemas.push(format!(
                "module.json5: referencia ${tipo}:{nombre} sin recurso en la aplicación"
            ));
        }
    }

    let perfil = sub.join("entry/src/main/resources/base/profile/main_pages.json");
    if let Some(texto) = require_file(&perfil, "páginas del módulo", problemas) {
        for pagina in json5_array(&texto, "src") {
            let archivo = sub.join("entry/src/main/ets").join(format!("{pagina}.ets"));
            if !archivo.is_file() {
                problemas.push(format!(
                    "main_pages.json: página {pagina} sin {}",
                    archivo.display()
                ));
            }
        }
    }

    // La isla de HarmonyOS no tiene capa ArkTS en el workspace; el HAP es la que
    // se empaqueta, así que sólo se comprueba que el puente existe.
    let puente = raiz.join("native/tlaxtrelom-harmonyos/include/tlaxtrelom_harmonyos.h");
    if !puente.is_file() {
        problemas.push(format!(
            "falta el encabezado del puente {}",
            puente.display()
        ));
    }
}

/// Prefijo de comentario que admite cada formato de fichero de empaquetado.
/// `None` para binarios y para JSON puro, que no tiene comentarios.
fn comentario_de(nombre: &str) -> Option<&'static str> {
    if nombre.ends_with(".png")
        || nombre.ends_with(".ico")
        || nombre.ends_with(".jpg")
        || (nombre.ends_with(".json") && !nombre.ends_with(".json5"))
    {
        return None;
    }
    if nombre.ends_with(".json5") || nombre.ends_with(".ets") || nombre.ends_with(".ts") {
        return Some("//");
    }
    if nombre.ends_with(".svg") || nombre.ends_with(".xml") {
        return Some("<!--");
    }
    if nombre.ends_with(".nsi") {
        return Some(";");
    }
    if nombre.ends_with(".cmd") {
        return Some("REM");
    }
    Some("#")
}

/// Reglas que dependen sólo del contenido de un fichero, para que las cubran
/// tanto el recorrido completo como la lista explícita de `package-check`.
fn check_text(nombre: &str, texto: &str, donde: &Path, _dir: &Path) -> Result<()> {
    let cabecera = "SPDX-License-Identifier: MIT";
    let derechos = "Copyright (c) 2026 angiraulene1-create";
    if let Some(prefijo) = comentario_de(nombre) {
        for esperada in [cabecera, derechos] {
            let presente = texto.lines().any(|linea| {
                linea
                    .trim()
                    .strip_prefix(prefijo)
                    .map(|resto| resto.trim_start().trim_end_matches("-->").trim_end())
                    == Some(esperada)
            });
            if !presente {
                return Err(format!("{nombre}: falta la línea {prefijo} {esperada}"));
            }
        }
    }
    match nombre {
        "tlaxtrelom.desktop" => {
            for clave in ["[Desktop Entry]", "Type=Application", "Exec=", "Icon="] {
                if !texto.contains(clave) {
                    return Err(format!("tlaxtrelom.desktop: falta {clave}"));
                }
            }
            if texto.contains("TryExec=") && !texto.contains("TryExec=tlaxtrelom") {
                return Err("el TryExec debe apuntar al binario empaquetado".to_string());
            }
        }
        "control" => {
            if !texto.contains("Package: tlaxtrelom") {
                return Err("control: Package != tlaxtrelom".to_string());
            }
        }
        "tlaxtrelom.nsi" => {
            for clave in [
                "!include \"MUI2.nsh\"",
                "OutFile \"tlaxtrelom-setup-${VERSION}.exe\"",
                "RequestExecutionLevel admin",
                "WriteUninstaller",
            ] {
                if !texto.contains(clave) {
                    return Err(format!("tlaxtrelom.nsi: falta {clave}"));
                }
            }
        }
        "app.json5" | "module.json5" | "build-profile.json5" => {
            json5_parse(texto).map_err(|e| format!("{nombre}: no parsea ({e})"))?;
        }
        _ if nombre.ends_with(".sh") => {
            // `\$` escapado no es un estilo: es una expansión que el shell nunca
            // hace. `install-harmonyos.sh` tuvo `SRC="\${1:-}"`, que dejaba el
            // argumento como la cadena literal `${1:-}` y el guion fallaba tarde.
            for escapado in [r"\${", r"\$("] {
                if texto.contains(escapado) {
                    return Err(format!(
                        "{nombre}: contiene {escapado}; esa expansión escapada no la \
                         evalúa ningún shell"
                    ));
                }
            }
            if !texto.contains("set -e") {
                return Err(format!(
                    "{nombre}: un guion de instalación necesita `set -e`"
                ));
            }
            if let Some(sh) = which("sh") {
                exec(
                    Command::new(sh).args(["-n", &donde.to_string_lossy()]),
                    &format!("sh -n {nombre}"),
                )?;
            }
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arg(v: &str) -> String {
        v.to_string()
    }

    #[test]
    fn fill_placeholders_sustituye_los_marcadores_de_control() {
        let plantila = "Version: ${VERSION}\nArchitecture: ${ARCH}\nInstalled-Size: ${SIZE}\n";
        let lleno = fill_placeholders(
            plantila,
            &[("VERSION", "1.2.3"), ("ARCH", "amd64"), ("SIZE", "4242")],
        );
        assert_eq!(
            lleno,
            "Version: 1.2.3\nArchitecture: amd64\nInstalled-Size: 4242\n"
        );
        assert!(!lleno.contains("${"));
    }

    #[test]
    fn el_escaner_de_json5_lee_claves_sin_parser() {
        let texto = "// SPDX-License-Identifier: MIT\n{\n  \"module\": { \"mainElement\": \"EntryAbility\", \"abilities\": [ { \"name\": \"EntryAbility\", \"srcEntry\": \"./ets/a.ets\" } ] } }\n";
        assert!(json5_balanced(&strip_json5_comments(texto)));
        assert_eq!(
            json5_string(texto, "mainElement").as_deref(),
            Some("EntryAbility")
        );
        let bloque = json5_block(texto, "abilities").expect("bloque de abilities");
        assert_eq!(
            json5_strings(&bloque, "name"),
            vec!["EntryAbility".to_string()]
        );
        assert_eq!(
            json5_strings(&bloque, "srcEntry"),
            vec!["./ets/a.ets".to_string()]
        );
        assert_eq!(json5_string(texto, "inexistente"), None);
    }

    #[test]
    fn el_escaner_detecta_desbalance_y_cadenas_con_llaves() {
        assert!(!json5_balanced("{ \"a\": [1, 2]"));
        assert!(!json5_balanced("{ \"a\": ]1[ }"));
        assert!(!json5_balanced("{ \"a\": 1 }}"));
        assert!(json5_balanced("{ \"a\": \"llave { dentro de cadena\" }"));
        assert!(json5_balanced("// comentario con }\n{ \"a\": 1 }\n"));
    }

    #[test]
    fn json5_array_extrae_elementos_de_cadena() {
        let texto = "{ \"src\": [ \"pages/Index\", \"pages/Otro\" ] }";
        assert_eq!(
            json5_array(texto, "src"),
            vec!["pages/Index".to_string(), "pages/Otro".to_string()]
        );
    }

    #[test]
    fn extract_resource_refs_recolecta_tipo_y_nombre() {
        let texto = "\"mainElement\": \"EntryAbility\",\n\"icon\": \"$media:app_icon\",\n\"label\": \"$string:app_name\",\n\"pages\": \"$profile:main_pages\",\n";
        let refs = extract_resource_refs(texto);
        assert!(refs.contains("media:app_icon"), "{refs:?}");
        assert!(refs.contains("string:app_name"), "{refs:?}");
        assert!(refs.contains("profile:main_pages"), "{refs:?}");
        assert!(!refs.iter().any(|r| r.contains("EntryAbility")));
    }

    #[test]
    fn nsi_instala_solo_rutas_que_el_empaquetado_entrega() {
        let texto = fs::read_to_string(workspace().join("packaging/windows/tlaxtrelom.nsi"))
            .expect("el guion NSIS debe estar en packaging/windows");
        let rutas = nsi_file_paths(&texto);
        assert!(
            rutas.contains("tlaxtrelom.exe")
                && rutas.contains("assets")
                && rutas.contains("shaders"),
            "rutas detectadas: {rutas:?}"
        );
        assert!(
            missing_staged(&rutas, "windows").is_empty(),
            "el NSIS pide cosas que package no entrega: {:?}",
            missing_staged(&rutas, "windows")
        );
    }

    #[test]
    fn rutas_de_instalacion_inventadas_se_detectan() {
        let rutas = nsi_file_paths(
            "\nFile \"tlaxtrelom.exe\"\nFile \"$INSTDIR\\uninstall.exe\"\nFile /r \"otra\\*.*\"\n",
        );
        assert!(rutas.contains("otra"));
        assert!(!rutas.contains("$INSTDIR"));
        let faltan = missing_staged(&rutas, "windows");
        assert_eq!(faltan, vec!["otra".to_string()]);
    }

    #[test]
    fn tripletas_arquitecturas_y_nombres_de_archivo() {
        assert_eq!(deb_arch("x86_64-unknown-linux-gnu").unwrap(), "amd64");
        assert_eq!(deb_arch("aarch64-unknown-linux-ohos").unwrap(), "arm64");
        assert!(deb_arch("mips-unknown-linux").is_err());
        assert_eq!(hap_abi("aarch64-unknown-linux-ohos").unwrap(), "arm64-v8a");
        assert!(hap_abi("riscv64-unknown-linux-ohos").is_err());
        assert_eq!(suffix("x86_64-pc-windows-msvc"), "x86-64-pc-windows-msvc");
        assert_eq!(exe("windows"), "tlaxtrelom.exe");
        assert_eq!(exe("linux"), "tlaxtrelom");
    }

    #[test]
    fn version_of_lee_workspace_package_y_ignora_la_heredada() {
        let raiz =
            "[workspace]\n[workspace.package]\nversion = \"9.8.7\"\nrust-version = \"1.83\"\n";
        assert_eq!(
            version_of(raiz, "[workspace.package]").as_deref(),
            Some("9.8.7")
        );
        let hijo = "[package]\nversion.workspace = true\nedition.workspace = true\n";
        assert_eq!(version_of(hijo, "[package]"), None);
    }

    #[test]
    fn la_version_del_workspace_se_lee_del_manifest() {
        let v = version();
        assert_ne!(v, "0.0.0-dev", "no se pudo leer la versión de Cargo.toml");
        assert_eq!(v.matches('.').count(), 2, "{v} no parece x.y.z");
    }

    #[test]
    fn parse_valida_plataformas_y_flags() {
        let opts = parse(&[arg("--platform"), arg("windows"), arg("--skip-build")]).unwrap();
        assert_eq!(opts.platforms, vec!["windows".to_string()]);
        assert!(opts.skip_build);
        let todos = parse(&[arg("--all")]).unwrap();
        assert_eq!(todos.platforms, PLATFORMS.to_vec());
        assert!(parse(&[arg("--platform"), arg("maemo")]).is_err());
        assert!(parse(&[arg("--nope")]).is_err());
    }

    #[test]
    fn install_size_suma_kib_de_ficheros_regulares() {
        let temporal = env::temp_dir().join(format!("tlx-size-{}", std::process::id()));
        let _ = fs::remove_dir_all(&temporal);
        fs::create_dir_all(temporal.join("usr/bin")).unwrap();
        fs::write(temporal.join("usr/bin/x"), vec![7u8; 2048]).unwrap();
        fs::create_dir_all(temporal.join("usr/vacio")).unwrap();
        assert_eq!(install_size(&temporal), 2);
        let _ = fs::remove_dir_all(&temporal);
    }

    #[test]
    fn el_arbol_de_packaging_del_repositorio_pasa_check() {
        check(&[]).expect("package-check debe pasar en el árbol limpio");
    }

    #[test]
    fn los_guiones_shell_no_escapan_sus_expansiones() {
        let cabecera = "# SPDX-License-Identifier: MIT\n# Copyright (c) 2026 angiraulene1-create\n";
        let donde = workspace().join("packaging/linux/install.sh");
        let bueno = format!("{cabecera}set -eu\nSRC=\"${{1:-}}\"\n");
        check_text("install.sh", &bueno, &donde, Path::new(".")).expect("un guion correcto pasa");
        let escapado = format!("{cabecera}set -eu\nSRC=\"\\${{1:-}}\"\n");
        let error = check_text("install.sh", &escapado, &donde, Path::new(".")).unwrap_err();
        assert!(error.contains(r"\${"), "{error}");
        let descuidado = format!("{cabecera}SRC=\"${{1:-}}\"\n");
        let error = check_text("install.sh", &descuidado, &donde, Path::new(".")).unwrap_err();
        assert!(error.contains("set -e"), "{error}");
    }

    fn cabecera_elf(maquina: u16) -> Vec<u8> {
        let mut bytes = vec![0u8; 20];
        bytes[0..4].copy_from_slice(b"\x7fELF");
        bytes[18..20].copy_from_slice(&maquina.to_le_bytes());
        bytes
    }

    #[test]
    fn elf_machine_lee_la_arquitectura_de_la_cabecera() {
        let temporal = env::temp_dir().join(format!("tlx-elf-{}", std::process::id()));
        let _ = fs::remove_dir_all(&temporal);
        fs::create_dir_all(&temporal).unwrap();
        fs::write(temporal.join("x86.so"), cabecera_elf(62)).unwrap();
        fs::write(temporal.join("arm.so"), cabecera_elf(183)).unwrap();
        fs::write(temporal.join("otro.so"), b"MZ\x90\x01\x03\x04").unwrap();
        assert_eq!(elf_machine(&temporal.join("x86.so")), Some(62));
        assert_eq!(elf_machine(&temporal.join("arm.so")), Some(183));
        assert_eq!(elf_machine(&temporal.join("otro.so")), None);
        assert_eq!(elf_machine(&temporal.join("no-esta.so")), None);
        assert_eq!(abi_machine("arm64-v8a"), Some(183));
        assert_eq!(abi_machine("x86_64"), Some(62));
        assert_eq!(abi_machine("armeabi-v7a"), None);
        let _ = fs::remove_dir_all(&temporal);
    }

    #[test]
    fn la_puente_de_otra_abi_se_descarta_en_vez_de_empaquetarse() {
        let temporal = env::temp_dir().join(format!("tlx-puente-{}", std::process::id()));
        let _ = fs::remove_dir_all(&temporal);
        let host = temporal.join("native/tlaxtrelom-harmonyos/build");
        fs::create_dir_all(&host).unwrap();
        let so = host.join("libtlaxtrelom_harmonyos.so");
        fs::write(&so, cabecera_elf(62)).unwrap();

        let mut avisos = Vec::new();
        assert_eq!(puente_para_abi(&temporal, "arm64-v8a", &mut avisos), None);
        assert!(avisos[0].contains("e_machine 62"), "{avisos:?}");

        let mut sin_avisos = Vec::new();
        assert_eq!(
            puente_para_abi(&temporal, "x86_64", &mut sin_avisos).as_deref(),
            Some(so.as_path())
        );
        assert!(sin_avisos.is_empty(), "{sin_avisos:?}");

        // build-ohos tiene prioridad sobre build.
        let ohos = temporal.join("native/tlaxtrelom-harmonyos/build-ohos");
        fs::create_dir_all(&ohos).unwrap();
        let so_ohos = ohos.join("libtlaxtrelom_harmonyos.so");
        fs::write(&so_ohos, cabecera_elf(183)).unwrap();
        let mut otros = Vec::new();
        assert_eq!(
            puente_para_abi(&temporal, "arm64-v8a", &mut otros).as_deref(),
            Some(so_ohos.as_path())
        );
        assert!(otros.is_empty(), "{otros:?}");

        let _ = fs::remove_dir_all(temporal.join("native"));
        let mut falta = Vec::new();
        assert_eq!(puente_para_abi(&temporal, "arm64-v8a", &mut falta), None);
        assert!(falta[0].contains("make harmonyos"), "{falta:?}");
        let _ = fs::remove_dir_all(&temporal);
    }

    #[test]
    fn stage_de_windows_entrega_todas_las_rutas_que_promete() {
        let dir = workspace();
        let pkg = definitions(&dir).expect("packaging/");
        let temporal = env::temp_dir().join(format!("tlx-stage-{}", std::process::id()));
        let _ = fs::remove_dir_all(&temporal);
        fs::create_dir_all(&temporal).unwrap();
        let binario = temporal.join("tlaxtrelom.exe");
        fs::write(&binario, b"stub de prueba").unwrap();
        let opts = Options {
            platforms: vec!["windows".to_string()],
            profile: Profile::Debug,
            skip_build: true,
            cross: false,
            out: temporal.join("dist"),
        };
        let mut avisos = Vec::new();
        let raiz =
            stage(&dir, &pkg, "windows", &binario, &opts, &mut avisos).expect("stage de windows");
        let esperadas = staged_paths("windows");
        for ruta in &esperadas {
            assert!(raiz.join(ruta).exists(), "falta {ruta} en el árbol");
        }
        let nsi = fs::read_to_string(raiz.join("tlaxtrelom.nsi")).unwrap();
        assert!(
            missing_staged(&nsi_file_paths(&nsi), "windows").is_empty(),
            "el .nsi empaquetado pide rutas que no están en el árbol"
        );
        assert_eq!(
            fs::read_to_string(raiz.join("VERSION")).unwrap().trim(),
            version()
        );
        assert!(!fs::read_to_string(raiz.join("BUILDINFO"))
            .unwrap()
            .is_empty());
        assert!(
            fs::read_to_string(raiz.join("tlaxtrelom.exe")).is_ok_and(|b| b == "stub de prueba")
        );
        let _ = fs::remove_dir_all(&temporal);
    }

    #[test]
    fn prebuilt_pide_quitar_skip_build_cuando_no_hay_binario() {
        let vacio = env::temp_dir().join(format!("tlx-sin-bin-{}", std::process::id()));
        let _ = fs::remove_dir_all(&vacio);
        fs::create_dir_all(&vacio).unwrap();
        let opts = Options {
            platforms: vec!["harmonyos".to_string()],
            profile: Profile::Release,
            skip_build: true,
            cross: false,
            out: PathBuf::from("dist"),
        };
        let error = prebuilt(&vacio, "harmonyos", &opts).expect_err("no hay binario OHOS ahí");
        assert!(error.contains("--skip-build"), "{error}");
        assert!(error.contains("aarch64"), "{error}");
        fs::create_dir_all(vacio.join("target/aarch64-unknown-linux-ohos/debug")).unwrap();
        let so = vacio.join("target/aarch64-unknown-linux-ohos/debug/tlaxtrelom");
        fs::write(&so, "x").unwrap();
        assert_eq!(prebuilt(&vacio, "harmonyos", &opts).ok(), Some(so));
        let _ = fs::remove_dir_all(&vacio);
    }
}
