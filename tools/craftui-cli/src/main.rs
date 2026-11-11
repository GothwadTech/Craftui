use clap::{Parser, Subcommand, ValueEnum};
use craftui_core::{CraftError, Result, Size, WindowMetrics};
use craftui_dsl::{Severity, generate_rust, resolve_file};
use craftui_reactive::Scope;
use craftui_render::{
    BackendKind, PaintScene, TextureRegistry, render_scene_to_rgba, write_rgba_png,
};
use craftui_runtime::HotReloadSession;
use craftui_widgets::WidgetTree;
use notify::{Event, RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc;
use std::time::Instant;

#[derive(Parser, Debug)]
#[command(
    name = "craftui",
    version,
    about = "CraftUI CLI — scaffolding, dev hot-reload, .cui validation, headless snapshot rendering, and cross-platform builds"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum PlatformTarget {
    Desktop,
    Web,
    Android,
    Ios,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Scaffold a new CraftUI project with build.rs and a reactive counter template
    New {
        name: String,
        #[arg(long)]
        path: Option<PathBuf>,
    },
    /// Watch .cui files and print fast sub-100ms hot-reload validation feedback
    Dev {
        #[arg(long, default_value = "ui/main.cui")]
        entry: PathBuf,
        #[arg(long, default_value = "ui")]
        dir: PathBuf,
    },
    /// Open a live hot-reloading desktop preview window for a .cui file
    Preview {
        file: PathBuf,
        #[arg(long)]
        component: Option<String>,
        #[arg(long)]
        cpu: bool,
    },
    /// Parse, validate, and optionally emit AOT Rust code for .cui files
    Check {
        #[arg(default_value = ".")]
        path: PathBuf,
        #[arg(long)]
        emit_rust: bool,
    },
    /// Headlessly render a .cui file to an RGBA PNG snapshot via vello_cpu
    Render {
        file: PathBuf,
        #[arg(short, long)]
        out: PathBuf,
        #[arg(long)]
        component: Option<String>,
        #[arg(long, default_value_t = 800)]
        width: u32,
        #[arg(long, default_value_t = 600)]
        height: u32,
    },
    /// Render or validate cross-platform toolchain readiness (Rust targets, NDK, wasm-bindgen, Xcode, .cui files)
    Doctor {
        #[arg(default_value = ".")]
        path: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Build & package a CraftUI application for desktop, web, android, or ios
    Build {
        #[arg(long, value_enum, default_value_t = PlatformTarget::Desktop)]
        platform: PlatformTarget,
        #[arg(long)]
        release: bool,
        #[arg(short, long)]
        package: Option<String>,
        #[arg(short, long)]
        out: Option<PathBuf>,
        /// Generate platform packaging templates/bundles without invoking cargo cross-compilation
        #[arg(long)]
        skip_cargo: bool,
    },
}

fn main() {
    if let Err(err) = run() {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::New { name, path } => {
            let target_dir = path.unwrap_or_else(|| PathBuf::from(&name));
            scaffold_project(&name, &target_dir)?;
            println!(
                "Created CraftUI project '{}' at {}",
                name,
                target_dir.display()
            );
            println!(
                "Next steps:\n  cd {}\n  craftui check ui/main.cui\n  cargo run",
                target_dir.display()
            );
            Ok(())
        }
        Commands::Dev { entry, dir } => run_dev_watcher(&entry, &dir),
        Commands::Preview {
            file,
            component,
            cpu,
        } => craftui_platform_desktop::run_hot_reload_cui(
            craftui_platform_desktop::WindowConfig {
                title: format!("CraftUI Preview — {}", file.display()),
                force_cpu: cpu,
                ..Default::default()
            },
            file,
            component,
        ),
        Commands::Check { path, emit_rust } => {
            let stats = check_cui_paths(&path, emit_rust)?;
            println!(
                "Checked {} .cui file(s): {} warning(s), {} error(s)",
                stats.files, stats.warnings, stats.errors
            );
            if stats.errors > 0 {
                return Err(CraftError::Dsl(format!(
                    "{} .cui validation error(s) found",
                    stats.errors
                )));
            }
            Ok(())
        }
        Commands::Render {
            file,
            out,
            component,
            width,
            height,
        } => {
            render_cui_file_to_png(&file, component.as_deref(), width, height, &out)?;
            println!(
                "Rendered {}x{} snapshot to {}",
                width,
                height,
                out.display()
            );
            Ok(())
        }
        Commands::Doctor { path, json } => run_doctor(&path, json),
        Commands::Build {
            platform,
            release,
            package,
            out,
            skip_cargo,
        } => run_build(
            platform,
            release,
            package.as_deref(),
            out.as_deref(),
            skip_cargo,
        ),
    }
}

pub fn scaffold_project(name: &str, root: &Path) -> Result<()> {
    let src_dir = root.join("src");
    let ui_dir = root.join("ui");
    std::fs::create_dir_all(&src_dir)
        .and_then(|_| std::fs::create_dir_all(&ui_dir))
        .map_err(|e| CraftError::Platform(format!("failed creating directories: {e}")))?;

    let pkg_name = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>();

    let cargo_toml = format!(
        r#"[package]
name = "{pkg_name}"
version = "0.1.0"
edition = "2024"

[dependencies]
craftui = "0.1.0"

[build-dependencies]
craftui-dsl = "0.1.0"
"#
    );

    let build_rs = r#"fn main() {
    println!("cargo:rerun-if-changed=ui");
    let out_dir = std::path::PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    craftui_dsl::compile_cui_dir("ui", &out_dir).expect("failed compiling .cui templates");
}
"#;

    let main_cui = r#"export component MainView {
    in-out property <int> count: 0;

    Column {
        width: 100%;
        height: 100%;
        background: Theme.background;
        padding: 24dp;
        gap: 16dp;

        Row {
            align: Center;
            gap: 10dp;
            Text {
                text: "CraftUI App";
                font_size: 24;
                color: Theme.text_primary;
            }
            Badge {
                label: "Ready";
                icon: "check";
                background: Theme.primary;
                color: Theme.on_primary;
            }
        }
        Text {
            text: count;
            font_size: 20;
            color: Theme.primary;
        }
        Row {
            gap: 12dp;
            Button {
                text: "Increment";
                background: Theme.primary;
                cursor: Pointer;
                on_tap => { count = count + 1; }
            }
            Button {
                text: "Reset";
                cursor: Pointer;
                on_tap => { count = 0; }
            }
        }
    }
}
"#;

    let main_rs = r#"use craftui::prelude::*;

mod generated {
    include!(concat!(env!("OUT_DIR"), "/main.rs"));
}

fn main() -> craftui::core::Result<()> {
    run_app(
        WindowConfig {
            title: "CraftUI Starter".to_string(),
            ..Default::default()
        },
        |cx, tree, _ctx| {
            let ui = generated::MainView::new(cx);
            ui.build(cx, tree)
        },
    )
}
"#;

    std::fs::write(root.join("Cargo.toml"), cargo_toml)
        .and_then(|_| std::fs::write(root.join("build.rs"), build_rs))
        .and_then(|_| std::fs::write(ui_dir.join("main.cui"), main_cui))
        .and_then(|_| std::fs::write(src_dir.join("main.rs"), main_rs))
        .map_err(|e| CraftError::Platform(format!("failed writing template files: {e}")))?;
    Ok(())
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct CheckStats {
    pub files: usize,
    pub warnings: usize,
    pub errors: usize,
}

pub fn check_cui_paths(path: &Path, emit_rust: bool) -> Result<CheckStats> {
    let mut files = Vec::new();
    collect_cui_files(path, &mut files)?;
    let mut stats = CheckStats::default();

    for f in files {
        stats.files += 1;
        let resolved = resolve_file(&f);
        for d in &resolved.diagnostics {
            match d.severity {
                Severity::Warning => {
                    stats.warnings += 1;
                    eprintln!(
                        "warning [{} @ {}..{}]: {}",
                        f.display(),
                        d.span.start,
                        d.span.end,
                        d.message
                    );
                }
                Severity::Error => {
                    stats.errors += 1;
                    eprintln!(
                        "error [{} @ {}..{}]: {}",
                        f.display(),
                        d.span.start,
                        d.span.end,
                        d.message
                    );
                }
            }
        }
        if emit_rust {
            let code = generate_rust(&resolved.merged);
            println!("// === {} ===\n{}", f.display(), code);
        }
    }
    Ok(stats)
}

fn collect_cui_files(path: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    if path.is_file() {
        if path.extension().and_then(|e| e.to_str()) == Some("cui") {
            out.push(path.to_path_buf());
        }
        return Ok(());
    }
    if path.is_dir() {
        for entry in std::fs::read_dir(path)
            .map_err(|e| CraftError::Platform(format!("read_dir {} failed: {e}", path.display())))?
        {
            let entry = entry.map_err(|e| CraftError::Platform(format!("dir entry error: {e}")))?;
            let p = entry.path();
            if p.file_name().and_then(|n| n.to_str()) == Some("target") {
                continue;
            }
            if p.is_dir() {
                collect_cui_files(&p, out)?;
            } else if p.extension().and_then(|e| e.to_str()) == Some("cui") {
                out.push(p);
            }
        }
    }
    Ok(())
}

pub fn render_cui_to_png(
    source: &str,
    component: Option<&str>,
    width: u32,
    height: u32,
    out_png: &Path,
) -> Result<()> {
    let cx = Scope::root();
    let mut tree = WidgetTree::new(cx);
    let mut session = HotReloadSession::new(cx);
    session.reload(source, component, &mut tree)?;
    render_loaded_tree(&mut tree, width, height, out_png)
}

pub fn render_cui_file_to_png(
    file: &Path,
    component: Option<&str>,
    width: u32,
    height: u32,
    out_png: &Path,
) -> Result<()> {
    let cx = Scope::root();
    let mut tree = WidgetTree::new(cx);
    let mut session = HotReloadSession::new(cx);
    session.reload_file(file, component, &mut tree)?;
    render_loaded_tree(&mut tree, width, height, out_png)
}

fn render_loaded_tree(
    tree: &mut WidgetTree,
    width: u32,
    height: u32,
    out_png: &Path,
) -> Result<()> {
    let metrics = WindowMetrics {
        logical_size: Size::new(width.max(1) as f32, height.max(1) as f32),
        scale_factor: 1.0,
        ..Default::default()
    };
    tree.layout(metrics);
    let mut scene = PaintScene::new();
    tree.paint(&mut scene);

    let textures = TextureRegistry::new();
    let rgba = render_scene_to_rgba(
        BackendKind::VelloCpu,
        &scene,
        width.max(1),
        height.max(1),
        1.0,
        tree.theme().background,
        &textures,
        &mut tree.text_engine,
    );
    write_rgba_png(out_png, width.max(1), height.max(1), &rgba)?;
    Ok(())
}

fn run_dev_watcher(entry: &Path, dir: &Path) -> Result<()> {
    let watch_dir = if dir.exists() {
        dir
    } else if entry.exists() {
        entry.parent().unwrap_or_else(|| Path::new("."))
    } else {
        Path::new(".")
    };

    if entry.exists() {
        let _ = check_cui_paths(entry, false)?;
    }
    println!(
        "Watching {} for .cui hot-reload (Ctrl+C to stop)...",
        watch_dir.display()
    );

    let (tx, rx) = mpsc::channel::<notify::Result<Event>>();
    let mut watcher = notify::recommended_watcher(tx)
        .map_err(|e| CraftError::Platform(format!("watcher init failed: {e}")))?;
    watcher
        .watch(watch_dir, RecursiveMode::Recursive)
        .map_err(|e| CraftError::Platform(format!("watch failed: {e}")))?;

    let mut session = HotReloadSession::new(Scope::root());
    let mut tree = WidgetTree::new(session.state_scope);

    for event in rx.into_iter().flatten() {
        let touches_cui = event
            .paths
            .iter()
            .any(|p| p.extension().and_then(|e| e.to_str()) == Some("cui"));
        if !touches_cui {
            continue;
        }
        let target_path = if entry.exists() {
            entry.to_path_buf()
        } else if let Some(p) = event
            .paths
            .iter()
            .find(|p| p.extension().and_then(|e| e.to_str()) == Some("cui"))
        {
            p.clone()
        } else {
            continue;
        };
        let t0 = Instant::now();
        match session.reload_file(&target_path, None, &mut tree) {
            Ok(_) => println!(
                "[hot-reload] {} synced in {:.2}ms",
                target_path.display(),
                t0.elapsed().as_secs_f64() * 1000.0
            ),
            Err(e) => eprintln!("[hot-reload] {}: {e}", target_path.display()),
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoctorReport {
    pub rustc_version: Option<String>,
    pub cargo_version: Option<String>,
    pub installed_targets: Vec<String>,
    pub wasm_target: bool,
    pub android_target: bool,
    pub ios_target: bool,
    pub wasm_bindgen: bool,
    pub wasm_opt: bool,
    pub trunk: bool,
    pub adb: Option<PathBuf>,
    pub android_ndk: Option<PathBuf>,
    pub clang: Option<PathBuf>,
    pub xcodebuild: bool,
    pub cui_stats: CheckStats,
}

fn command_stdout(bin: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(bin).args(args).output().ok()?;
    if out.status.success() {
        Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        None
    }
}

fn find_in_path(name: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    #[cfg(windows)]
    let candidates = [format!("{name}.exe"), format!("{name}.cmd"), name.to_string()];
    #[cfg(not(windows))]
    let candidates = [name.to_string()];

    for dir in std::env::split_paths(&path_var) {
        for cand in &candidates {
            let full = dir.join(cand);
            if full.is_file() {
                return Some(full);
            }
        }
    }
    None
}

fn is_valid_ndk_root(dir: &Path) -> bool {
    dir.join("toolchains").join("llvm").join("prebuilt").is_dir()
}

pub fn discover_android_ndk() -> Option<PathBuf> {
    for var in ["ANDROID_NDK_HOME", "ANDROID_NDK_ROOT"] {
        if let Ok(val) = std::env::var(var) {
            let p = PathBuf::from(val);
            if is_valid_ndk_root(&p) {
                return Some(p);
            }
        }
    }

    let mut sdk_roots = Vec::new();
    for var in ["ANDROID_HOME", "ANDROID_SDK_ROOT"] {
        if let Ok(val) = std::env::var(var) {
            sdk_roots.push(PathBuf::from(val));
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        let h = PathBuf::from(home);
        sdk_roots.push(h.join("Android").join("Sdk"));
        sdk_roots.push(h.join("Library").join("Android").join("sdk"));
    }

    let mut ndk_dirs = Vec::new();
    for sdk in sdk_roots {
        let ndk_parent = sdk.join("ndk");
        if let Ok(entries) = std::fs::read_dir(&ndk_parent) {
            for entry in entries.flatten() {
                let p = entry.path();
                if is_valid_ndk_root(&p) {
                    ndk_dirs.push(p);
                }
            }
        }
    }
    ndk_dirs.sort();
    ndk_dirs.pop()
}

fn discover_adb() -> Option<PathBuf> {
    if let Some(p) = find_in_path("adb") {
        return Some(p);
    }
    if let Ok(home) = std::env::var("HOME") {
        let h = PathBuf::from(home);
        for cand in [
            h.join("Android/Sdk/platform-tools/adb"),
            h.join("Library/Android/sdk/platform-tools/adb"),
        ] {
            if cand.is_file() {
                return Some(cand);
            }
        }
    }
    None
}

fn ndk_toolchain_bin(ndk_root: &Path) -> Option<PathBuf> {
    let prebuilt = ndk_root.join("toolchains").join("llvm").join("prebuilt");
    for host in ["linux-x86_64", "darwin-arm64", "darwin-x86_64", "windows-x86_64"] {
        let bin = prebuilt.join(host).join("bin");
        if bin.is_dir() {
            return Some(bin);
        }
    }
    None
}

fn configure_android_ndk_env(cmd: &mut Command, ndk_root: &Path) -> Option<(PathBuf, PathBuf)> {
    let bin = ndk_toolchain_bin(ndk_root)?;
    let mut best_api: Option<(u32, PathBuf, PathBuf)> = None;
    if let Ok(entries) = std::fs::read_dir(&bin) {
        for entry in entries.flatten() {
            let p = entry.path();
            let Some(fname) = p.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            let Some(rest) = fname.strip_prefix("aarch64-linux-android") else {
                continue;
            };
            let Some(api_str) = rest.strip_suffix("-clang++") else {
                continue;
            };
            if let Ok(api) = api_str.parse::<u32>() {
                let cc = bin.join(format!("aarch64-linux-android{api}-clang"));
                if cc.is_file() && best_api.as_ref().is_none_or(|(b, _, _)| api > *b && *b < 24 || (api >= 24 && (*b < 24 || api < *b))) {
                    // Prefer lowest API >= 24 (matches minSdk 24), or highest available if < 24.
                    best_api = Some((api, cc, p));
                }
            }
        }
    }
    let (_, cc, cxx) = best_api?;
    let ar = bin.join("llvm-ar");
    if std::env::var_os("CC_aarch64_linux_android").is_none() {
        cmd.env("CC_aarch64_linux_android", &cc);
    }
    if std::env::var_os("CXX_aarch64_linux_android").is_none() {
        cmd.env("CXX_aarch64_linux_android", &cxx);
    }
    if ar.is_file() && std::env::var_os("AR_aarch64_linux_android").is_none() {
        cmd.env("AR_aarch64_linux_android", &ar);
    }
    Some((cc, cxx))
}

pub fn collect_doctor_report(project_dir: &Path) -> Result<DoctorReport> {
    let rustc_version = command_stdout("rustc", &["--version"]);
    let cargo_version = command_stdout("cargo", &["--version"]);
    let installed_targets: Vec<String> = command_stdout("rustup", &["target", "list", "--installed"])
        .map(|s| s.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect())
        .unwrap_or_default();

    let has_target = |t: &str| installed_targets.iter().any(|x| x == t);
    let cui_stats = if project_dir.exists() {
        check_cui_paths(project_dir, false)?
    } else {
        CheckStats::default()
    };

    Ok(DoctorReport {
        rustc_version,
        cargo_version,
        wasm_target: has_target("wasm32-unknown-unknown"),
        android_target: has_target("aarch64-linux-android"),
        ios_target: has_target("aarch64-apple-ios"),
        installed_targets,
        wasm_bindgen: find_in_path("wasm-bindgen").is_some(),
        wasm_opt: find_in_path("wasm-opt").is_some(),
        trunk: find_in_path("trunk").is_some(),
        adb: discover_adb(),
        android_ndk: discover_android_ndk(),
        clang: find_in_path("clang"),
        xcodebuild: cfg!(target_os = "macos") && find_in_path("xcodebuild").is_some(),
        cui_stats,
    })
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

pub fn format_doctor_json(r: &DoctorReport) -> String {
    let opt_str = |o: &Option<String>| {
        o.as_ref()
            .map_or_else(|| "null".to_string(), |s| json_escape(s))
    };
    let opt_path = |o: &Option<PathBuf>| {
        o.as_ref()
            .map_or_else(|| "null".to_string(), |p| json_escape(&p.display().to_string()))
    };
    format!(
        "{{\n  \"rustc\": {},\n  \"cargo\": {},\n  \"targets\": {{\n    \"wasm32_unknown_unknown\": {},\n    \"aarch64_linux_android\": {},\n    \"aarch64_apple_ios\": {}\n  }},\n  \"tools\": {{\n    \"wasm_bindgen\": {},\n    \"wasm_opt\": {},\n    \"trunk\": {},\n    \"adb\": {},\n    \"android_ndk\": {},\n    \"clang\": {},\n    \"xcodebuild\": {}\n  }},\n  \"cui\": {{\n    \"files\": {},\n    \"warnings\": {},\n    \"errors\": {}\n  }}\n}}",
        opt_str(&r.rustc_version),
        opt_str(&r.cargo_version),
        r.wasm_target,
        r.android_target,
        r.ios_target,
        r.wasm_bindgen,
        r.wasm_opt,
        r.trunk,
        opt_path(&r.adb),
        opt_path(&r.android_ndk),
        opt_path(&r.clang),
        r.xcodebuild,
        r.cui_stats.files,
        r.cui_stats.warnings,
        r.cui_stats.errors,
    )
}

fn run_doctor(path: &Path, json: bool) -> Result<()> {
    let report = collect_doctor_report(path)?;
    if json {
        println!("{}", format_doctor_json(&report));
    } else {
        println!("CraftUI Environment Doctor");
        println!("--------------------------");
        println!(
            "[{}] rustc: {}",
            if report.rustc_version.is_some() { "ok" } else { "missing" },
            report.rustc_version.as_deref().unwrap_or("not found")
        );
        println!(
            "[{}] cargo: {}",
            if report.cargo_version.is_some() { "ok" } else { "missing" },
            report.cargo_version.as_deref().unwrap_or("not found")
        );
        println!(
            "[{}] Desktop target (host): ready{}",
            if report.rustc_version.is_some() { "ok" } else { "missing" },
            if report.clang.is_some() { "" } else { " (clang missing)" }
        );
        println!(
            "[{}] Web target (wasm32-unknown-unknown): {} (wasm-bindgen: {}, wasm-opt: {}, trunk: {})",
            if report.wasm_target { "ok" } else { "missing" },
            if report.wasm_target { "installed" } else { "run `rustup target add wasm32-unknown-unknown`" },
            if report.wasm_bindgen { "yes" } else { "no" },
            if report.wasm_opt { "yes" } else { "no" },
            if report.trunk { "yes" } else { "no" },
        );
        println!(
            "[{}] Android target (aarch64-linux-android): {} (NDK: {}, adb: {})",
            if report.android_target && report.android_ndk.is_some() { "ok" } else { "warn" },
            if report.android_target { "installed" } else { "run `rustup target add aarch64-linux-android`" },
            report.android_ndk.as_ref().map_or_else(|| "not detected".into(), |p| p.display().to_string()),
            report.adb.as_ref().map_or_else(|| "not found".into(), |p| p.display().to_string()),
        );
        println!(
            "[{}] iOS target (aarch64-apple-ios): {} (xcodebuild: {})",
            if report.ios_target && report.xcodebuild { "ok" } else { "info" },
            if report.ios_target {
                "installed"
            } else if cfg!(target_os = "macos") {
                "run `rustup target add aarch64-apple-ios`"
            } else {
                "macOS/Xcode required for linking (`craftui build --platform ios --skip-cargo` emits Swift/Metal template)"
            },
            if report.xcodebuild { "yes" } else { "no" },
        );
        println!(
            "[{}] Templates (.cui): {} file(s), {} warning(s), {} error(s)",
            if report.cui_stats.errors == 0 { "ok" } else { "error" },
            report.cui_stats.files,
            report.cui_stats.warnings,
            report.cui_stats.errors,
        );
    }

    if report.rustc_version.is_none() || report.cargo_version.is_none() {
        return Err(CraftError::Platform(
            "Rust toolchain (rustc/cargo) is required".to_string(),
        ));
    }
    if report.cui_stats.errors > 0 {
        return Err(CraftError::Dsl(format!(
            "{} .cui validation error(s) found",
            report.cui_stats.errors
        )));
    }
    Ok(())
}

pub fn emit_web_bundle(out_dir: &Path, app_name: &str, built_wasm: Option<&Path>) -> Result<()> {
    std::fs::create_dir_all(out_dir)
        .map_err(|e| CraftError::Platform(format!("failed creating {}: {e}", out_dir.display())))?;

    let mut wasm_bindgen_ran = false;
    if let Some(wasm_path) = built_wasm
        && wasm_path.is_file()
    {
        let raw_dest = out_dir.join(
            wasm_path
                .file_name()
                .unwrap_or_else(|| std::ffi::OsStr::new("craftui_app.wasm")),
        );
        std::fs::copy(wasm_path, &raw_dest).map_err(|e| {
            CraftError::Platform(format!("failed copying wasm {}: {e}", wasm_path.display()))
        })?;

        if find_in_path("wasm-bindgen").is_some() {
            let status = Command::new("wasm-bindgen")
                .arg(wasm_path)
                .args(["--target", "web", "--out-name", "craftui_app", "--out-dir"])
                .arg(out_dir)
                .status()
                .map_err(|e| CraftError::Platform(format!("wasm-bindgen failed: {e}")))?;
            if !status.success() {
                return Err(CraftError::Platform(format!(
                    "wasm-bindgen exited with status {status}"
                )));
            }
            wasm_bindgen_ran = true;

            let bg_wasm = out_dir.join("craftui_app_bg.wasm");
            if bg_wasm.is_file() && find_in_path("wasm-opt").is_some() {
                let _ = Command::new("wasm-opt")
                    .arg("-O3")
                    .arg(&bg_wasm)
                    .arg("-o")
                    .arg(&bg_wasm)
                    .status();
            }
        }
    }

    let index_html = format!(
        r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover" />
  <title>{app_name} — CraftUI</title>
  <style>
    html, body {{
      margin: 0;
      padding: 0;
      width: 100%;
      height: 100%;
      overflow: hidden;
      background: #0b0e14;
      color: #f8fafc;
      font-family: system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
    }}
    #craftui-canvas {{
      width: 100vw;
      height: 100vh;
      display: block;
      outline: none;
    }}
    #craftui-status {{
      position: fixed;
      inset: 0;
      display: flex;
      align-items: center;
      justify-content: center;
      font-size: 14px;
      pointer-events: none;
      opacity: 0.78;
    }}
  </style>
</head>
<body>
  <canvas id="craftui-canvas" tabindex="0"></canvas>
  <div id="craftui-status">Loading {app_name}...</div>
  <script type="module">
    const status = document.getElementById("craftui-status");
    try {{
      const mod = await import("./craftui_app.js");
      await mod.default();
      status.remove();
    }} catch (err) {{
      status.textContent = "CraftUI WASM bundle pending wasm-bindgen output (./craftui_app.js): " + err;
    }}
  </script>
</body>
</html>
"#
    );

    let readme = format!(
        "# {app_name} (Web / WASM Bundle)\n\nGenerated by `craftui build --platform web`.\n{}\nServe locally:\n```sh\npython3 -m http.server 8080 -d {}\n```\n",
        if wasm_bindgen_ran {
            "JS glue (`craftui_app.js`) and `craftui_app_bg.wasm` generated via `wasm-bindgen`.\n"
        } else {
            "Note: `wasm-bindgen` was not detected on PATH, so only raw HTML + `.wasm` (if built) were written.\nInstall `cargo install wasm-bindgen-cli` and run `wasm-bindgen <target.wasm> --target web --out-name craftui_app --out-dir .` inside this directory.\n"
        },
        out_dir.display()
    );

    std::fs::write(out_dir.join("index.html"), index_html)
        .and_then(|_| std::fs::write(out_dir.join("README.md"), readme))
        .map_err(|e| CraftError::Platform(format!("failed writing web bundle: {e}")))?;
    Ok(())
}

pub fn emit_android_template(
    out_dir: &Path,
    app_name: &str,
    rust_lib_name: &str,
    built_so: Option<&Path>,
) -> Result<()> {
    let java_dir = out_dir.join("app/src/main/kotlin/dev/craftui/app");
    let jni_dir = out_dir.join("app/src/main/jniLibs/arm64-v8a");
    let res_values_dir = out_dir.join("app/src/main/res/values");
    std::fs::create_dir_all(&java_dir)
        .and_then(|_| std::fs::create_dir_all(&jni_dir))
        .and_then(|_| std::fs::create_dir_all(&res_values_dir))
        .map_err(|e| CraftError::Platform(format!("failed creating android dirs: {e}")))?;

    let settings_gradle = format!("rootProject.name = \"{app_name}\"\ninclude(\":app\")\n");
    let root_gradle = r#"plugins {
    id("com.android.application") version "8.6.1" apply false
    id("org.jetbrains.kotlin.android") version "2.0.21" apply false
}
"#;
    let app_gradle = r#"plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "dev.craftui.app"
    compileSdk = 35

    defaultConfig {
        applicationId = "dev.craftui.app"
        minSdk = 24
        targetSdk = 35
        versionCode = 1
        versionName = "0.1.0"
        ndk {
            abiFilters += listOf("arm64-v8a")
        }
    }

    buildFeatures {
        prefab = true
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions {
        jvmTarget = "17"
    }
}

dependencies {
    implementation("androidx.core:core-ktx:1.13.1")
    implementation("androidx.games:games-activity:3.0.5")
}
"#;

    let manifest = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<manifest xmlns:android="http://schemas.android.com/apk/res/android">
    <uses-permission android:name="android.permission.INTERNET" />
    <application
        android:label="{app_name}"
        android:allowBackup="true"
        android:supportsRtl="true"
        android:theme="@style/Theme.CraftUI">
        <activity
            android:name=".MainActivity"
            android:exported="true"
            android:launchMode="singleTask"
            android:windowSoftInputMode="adjustResize"
            android:configChanges="orientation|keyboardHidden|keyboard|screenSize|smallestScreenSize|locale|layoutDirection|fontScale|screenLayout|density|uiMode">
            <meta-data
                android:name="android.app.lib_name"
                android:value="{rust_lib_name}" />
            <intent-filter>
                <action android:name="android.intent.action.MAIN" />
                <category android:name="android.intent.category.LAUNCHER" />
            </intent-filter>
        </activity>
    </application>
</manifest>
"#
    );

    let styles_xml = r#"<?xml version="1.0" encoding="utf-8"?>
<resources>
    <style name="Theme.CraftUI" parent="@android:style/Theme.DeviceDefault.NoActionBar" />
</resources>
"#;

    let main_activity = format!(
        r#"package dev.craftui.app

import android.os.Bundle
import androidx.core.view.WindowCompat
import com.google.androidgamesdk.GameActivity

class MainActivity : GameActivity() {{
    companion object {{
        init {{
            System.loadLibrary("{rust_lib_name}")
        }}
    }}

    override fun onCreate(savedInstanceState: Bundle?) {{
        super.onCreate(savedInstanceState)
        WindowCompat.setDecorFitsSystemWindows(window, false)
    }}
}}
"#
    );

    if let Some(so_path) = built_so
        && so_path.is_file()
    {
        let target_so = jni_dir.join(format!("lib{rust_lib_name}.so"));
        std::fs::copy(so_path, &target_so).map_err(|e| {
            CraftError::Platform(format!("failed copying .so {}: {e}", so_path.display()))
        })?;
    }

    std::fs::write(out_dir.join("settings.gradle.kts"), settings_gradle)
        .and_then(|_| std::fs::write(out_dir.join("build.gradle.kts"), root_gradle))
        .and_then(|_| std::fs::write(out_dir.join("app/build.gradle.kts"), app_gradle))
        .and_then(|_| std::fs::write(out_dir.join("app/src/main/AndroidManifest.xml"), manifest))
        .and_then(|_| std::fs::write(res_values_dir.join("styles.xml"), styles_xml))
        .and_then(|_| std::fs::write(java_dir.join("MainActivity.kt"), main_activity))
        .map_err(|e| CraftError::Platform(format!("failed writing android template: {e}")))?;
    Ok(())
}

pub fn emit_ios_template(out_dir: &Path, app_name: &str, rust_lib_name: &str) -> Result<()> {
    std::fs::create_dir_all(out_dir)
        .map_err(|e| CraftError::Platform(format!("failed creating ios dirs: {e}")))?;

    let header = r#"#ifndef CRAFTUI_IOS_H
#define CRAFTUI_IOS_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct IosAppHost IosAppHost;

typedef struct {
    void *ca_metal_layer;
    float width_pt;
    float height_pt;
    float contents_scale;
} IosMetalLayerDescriptor;

typedef void (*IosBuildUiCallback)(IosAppHost *host, void *user_data);

IosAppHost *craftui_ios_app_create(float width_pt, float height_pt, float scale_factor);
IosAppHost *craftui_ios_app_launch(IosMetalLayerDescriptor layer, IosBuildUiCallback build, void *user_data);
int32_t craftui_ios_app_attach_metal(IosAppHost *app, IosMetalLayerDescriptor layer);
const char *craftui_ios_app_last_error(const IosAppHost *app);
void craftui_ios_app_clear_error(IosAppHost *app);
void craftui_ios_app_destroy(IosAppHost *app);
void craftui_ios_app_resize(IosAppHost *app, float width_pt, float height_pt, float scale_factor);
void craftui_ios_app_set_safe_area(IosAppHost *app, float top, float right, float bottom, float left);
void craftui_ios_app_set_keyboard(IosAppHost *app, float overlap_height_pt, bool visible);
bool craftui_ios_app_touch(IosAppHost *app, uint64_t touch_id, uint8_t phase, float x_pt, float y_pt, uint64_t timestamp_ms);
bool craftui_ios_app_insert_text(IosAppHost *app, const uint8_t *utf8_ptr, size_t utf8_len);
bool craftui_ios_app_set_marked_text(IosAppHost *app, const uint8_t *utf8_ptr, size_t utf8_len, size_t cursor_end);
bool craftui_ios_app_delete_backward(IosAppHost *app);
uint8_t craftui_ios_app_tick(IosAppHost *app, float dt_sec);

#ifdef __cplusplus
}
#endif

#endif
"#;

    let swift = r#"import QuartzCore
import UIKit

final class CraftUIMetalView: UIView {
    override class var layerClass: AnyClass { CAMetalLayer.self }
    var metalLayer: CAMetalLayer { layer as! CAMetalLayer }
}

final class CraftUIViewController: UIViewController, UIKeyInput {
    private let metalView = CraftUIMetalView()
    private var app: OpaquePointer?
    private var displayLink: CADisplayLink?
    private var lastTimestamp: CFTimeInterval = 0
    private var keyboardActive = false

    var hasText: Bool { true }
    override var canBecomeFirstResponder: Bool { true }

    override func loadView() {
        metalView.isMultipleTouchEnabled = true
        metalView.metalLayer.pixelFormat = .bgra8Unorm
        view = metalView
    }

    override func viewDidLoad() {
        super.viewDidLoad()
        let scale = Float(UIScreen.main.nativeScale)
        let layerDesc = IosMetalLayerDescriptor(
            ca_metal_layer: Unmanaged.passUnretained(metalView.metalLayer).toOpaque(),
            width_pt: Float(view.bounds.width),
            height_pt: Float(view.bounds.height),
            contents_scale: scale
        )
        app = craftui_ios_app_launch(layerDesc, nil, nil)
        if let app, let errPtr = craftui_ios_app_last_error(app) {
            NSLog("CraftUI launch error: %@", String(cString: errPtr))
            craftui_ios_app_clear_error(app)
        }

        NotificationCenter.default.addObserver(
            self,
            selector: #selector(handleKeyboard(_:)),
            name: UIResponder.keyboardWillChangeFrameNotification,
            object: nil
        )
        let link = CADisplayLink(target: self, selector: #selector(stepFrame(_:)))
        link.add(to: .main, forMode: .common)
        displayLink = link
    }

    override func viewDidLayoutSubviews() {
        super.viewDidLayoutSubviews()
        guard let app else { return }
        let scale = Float(view.window?.screen.nativeScale ?? UIScreen.main.nativeScale)
        metalView.metalLayer.contentsScale = CGFloat(scale)
        craftui_ios_app_resize(app, Float(view.bounds.width), Float(view.bounds.height), scale)
        let sa = view.safeAreaInsets
        craftui_ios_app_set_safe_area(app, Float(sa.top), Float(sa.right), Float(sa.bottom), Float(sa.left))
    }

    @objc private func handleKeyboard(_ note: Notification) {
        guard let app,
              let endFrame = note.userInfo?[UIResponder.keyboardFrameEndUserInfoKey] as? CGRect else { return }
        let overlap = max(0, view.bounds.maxY - view.convert(endFrame, from: nil).minY)
        craftui_ios_app_set_keyboard(app, Float(overlap), overlap > 0)
    }

    @objc private func stepFrame(_ link: CADisplayLink) {
        guard let app else { return }
        let dt = lastTimestamp == 0 ? 0.016 : Float(min(0.05, max(0.001, link.timestamp - lastTimestamp)))
        lastTimestamp = link.timestamp
        let flags = craftui_ios_app_tick(app, dt)
        let wantsKeyboard = (flags & 0b010) != 0
        if wantsKeyboard && !keyboardActive {
            keyboardActive = true
            becomeFirstResponder()
        } else if !wantsKeyboard && keyboardActive {
            keyboardActive = false
            resignFirstResponder()
        }
        if (flags & 0b100) != 0, let errPtr = craftui_ios_app_last_error(app) {
            NSLog("CraftUI runtime error: %@", String(cString: errPtr))
            craftui_ios_app_clear_error(app)
        }
    }

    private func sendTouches(_ touches: Set<UITouch>, phase: UInt8) {
        guard let app else { return }
        for t in touches {
            let pt = t.location(in: view)
            let tid = UInt64(bitPattern: Int64(ObjectIdentifier(t).hashValue))
            _ = craftui_ios_app_touch(app, tid, phase, Float(pt.x), Float(pt.y), UInt64(t.timestamp * 1000.0))
        }
    }

    override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent?) { sendTouches(touches, phase: 0) }
    override func touchesMoved(_ touches: Set<UITouch>, with event: UIEvent?) { sendTouches(touches, phase: 1) }
    override func touchesEnded(_ touches: Set<UITouch>, with event: UIEvent?) { sendTouches(touches, phase: 2) }
    override func touchesCancelled(_ touches: Set<UITouch>, with event: UIEvent?) { sendTouches(touches, phase: 3) }

    func insertText(_ text: String) {
        guard let app else { return }
        var bytes = Array(text.utf8)
        bytes.withUnsafeMutableBufferPointer { buf in
            _ = craftui_ios_app_insert_text(app, buf.baseAddress, buf.count)
        }
    }

    func deleteBackward() {
        guard let app else { return }
        _ = craftui_ios_app_delete_backward(app)
    }

    deinit {
        displayLink?.invalidate()
        NotificationCenter.default.removeObserver(self)
        if let app { craftui_ios_app_destroy(app) }
    }
}
"#;

    let info_plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key>
  <string>{app_name}</string>
  <key>CFBundleIdentifier</key>
  <string>dev.craftui.{rust_lib_name}</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>0.1.0</string>
  <key>UIRequiredDeviceCapabilities</key>
  <array><string>arm64</string><string>metal</string></array>
</dict>
</plist>
"#
    );

    let readme = format!(
        "# {app_name} (iOS UIKit + Metal Bridge)\n\n1. On macOS with Xcode installed, build the static library:\n   ```sh\n   cargo build --target aarch64-apple-ios --release -p {rust_lib_name}\n   ```\n2. Add `CraftUI.h` (Bridging Header), `CraftUIViewController.swift`, `Info.plist`, and link `lib{rust_lib_name}.a` + `Metal.framework`, `QuartzCore.framework`, `UIKit.framework`.\n"
    );

    std::fs::write(out_dir.join("CraftUI.h"), header)
        .and_then(|_| std::fs::write(out_dir.join("CraftUIViewController.swift"), swift))
        .and_then(|_| std::fs::write(out_dir.join("Info.plist"), info_plist))
        .and_then(|_| std::fs::write(out_dir.join("README.md"), readme))
        .map_err(|e| CraftError::Platform(format!("failed writing ios template: {e}")))?;
    Ok(())
}

fn find_built_artifact(target_triple: &str, release: bool, pkg: &str, ext: &str) -> Option<PathBuf> {
    let profile = if release { "release" } else { "debug" };
    let stem = pkg.replace('-', "_");
    let direct = PathBuf::from("target")
        .join(target_triple)
        .join(profile)
        .join(if ext == "so" {
            format!("lib{stem}.so")
        } else {
            format!("{stem}.{ext}")
        });
    if direct.is_file() {
        return Some(direct);
    }
    None
}

fn run_build(
    platform: PlatformTarget,
    release: bool,
    package: Option<&str>,
    out_override: Option<&Path>,
    skip_cargo: bool,
) -> Result<()> {
    let profile_dir = |default_sub: &str| {
        out_override
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("dist").join(default_sub))
    };

    match platform {
        PlatformTarget::Desktop => {
            if !skip_cargo {
                let mut cmd = Command::new("cargo");
                cmd.arg("build");
                if let Some(pkg) = package {
                    cmd.args(["-p", pkg]);
                }
                if release {
                    cmd.arg("--release");
                }
                let status = cmd
                    .status()
                    .map_err(|e| CraftError::Platform(format!("failed spawning cargo: {e}")))?;
                if !status.success() {
                    return Err(CraftError::Platform(format!(
                        "cargo build exited with status {status}"
                    )));
                }
            }
            println!("Desktop build finished.");
        }
        PlatformTarget::Web => {
            let pkg = package.unwrap_or("craftui-platform-web");
            if !skip_cargo {
                let mut cmd = Command::new("cargo");
                cmd.args(["build", "--target", "wasm32-unknown-unknown", "-p", pkg]);
                if release {
                    cmd.arg("--release");
                }
                let status = cmd
                    .status()
                    .map_err(|e| CraftError::Platform(format!("failed spawning cargo: {e}")))?;
                if !status.success() {
                    return Err(CraftError::Platform(format!(
                        "cargo wasm32 build exited with status {status}"
                    )));
                }
            }
            let out_dir = profile_dir("web");
            let wasm = find_built_artifact("wasm32-unknown-unknown", release, pkg, "wasm");
            emit_web_bundle(&out_dir, pkg, wasm.as_deref())?;
            println!("Packaged CraftUI Web bundle at {}", out_dir.display());
        }
        PlatformTarget::Android => {
            let pkg = package.unwrap_or("craftui-platform-android");
            if !skip_cargo {
                let mut cmd = Command::new("cargo");
                cmd.args(["build", "--target", "aarch64-linux-android", "-p", pkg]);
                if release {
                    cmd.arg("--release");
                }
                if let Some(ndk) = discover_android_ndk() {
                    let _ = configure_android_ndk_env(&mut cmd, &ndk);
                }
                let status = cmd
                    .status()
                    .map_err(|e| CraftError::Platform(format!("failed spawning cargo: {e}")))?;
                if !status.success() {
                    return Err(CraftError::Platform(format!(
                        "cargo android build exited with status {status}"
                    )));
                }
            }
            let out_dir = profile_dir("android");
            let lib_name = pkg.replace('-', "_");
            let so = find_built_artifact("aarch64-linux-android", release, pkg, "so");
            emit_android_template(&out_dir, pkg, &lib_name, so.as_deref())?;
            println!("Packaged CraftUI Android Gradle project at {}", out_dir.display());
        }
        PlatformTarget::Ios => {
            let pkg = package.unwrap_or("craftui-platform-ios");
            if !skip_cargo {
                if !cfg!(target_os = "macos") {
                    return Err(CraftError::Platform(
                        "iOS native compilation (`aarch64-apple-ios`) requires macOS + Xcode; pass `--skip-cargo` to emit the Swift/UIKit/Metal template bundle on Linux/Windows".to_string(),
                    ));
                }
                let mut cmd = Command::new("cargo");
                cmd.args(["build", "--target", "aarch64-apple-ios", "-p", pkg]);
                if release {
                    cmd.arg("--release");
                }
                let status = cmd
                    .status()
                    .map_err(|e| CraftError::Platform(format!("failed spawning cargo: {e}")))?;
                if !status.success() {
                    return Err(CraftError::Platform(format!(
                        "cargo iOS build exited with status {status}"
                    )));
                }
            }
            let out_dir = profile_dir("ios");
            let lib_name = pkg.replace('-', "_");
            emit_ios_template(&out_dir, pkg, &lib_name)?;
            println!("Packaged CraftUI iOS Xcode bridge bundle at {}", out_dir.display());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use craftui_reactive::reset_runtime;

    #[test]
    fn scaffolds_validates_and_renders_cui_project() {
        reset_runtime();
        let tmp = std::env::temp_dir().join(format!(
            "craftui_cli_test_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        scaffold_project("demo_app", &tmp).unwrap();
        let cui_path = tmp.join("ui/main.cui");
        assert!(cui_path.exists());

        let stats = check_cui_paths(&cui_path, false).unwrap();
        assert_eq!(
            stats,
            CheckStats {
                files: 1,
                warnings: 0,
                errors: 0
            }
        );

        let src = std::fs::read_to_string(&cui_path).unwrap();
        let png_out = tmp.join("preview.png");
        render_cui_to_png(&src, Some("MainView"), 320, 240, &png_out).unwrap();
        assert!(std::fs::metadata(&png_out).unwrap().len() > 100);

        // Multi-file import check + file render
        std::fs::write(
            tmp.join("ui/badge.cui"),
            "export component StatusTag {\n in property <string> label: \"v1\";\n Text { text: label; }\n}\n",
        )
        .unwrap();
        std::fs::write(
            tmp.join("ui/app.cui"),
            "import { StatusTag } from \"./badge.cui\"\nexport component App {\n Column { StatusTag { label: \"OK\"; } }\n}\n",
        )
        .unwrap();
        let multi_stats = check_cui_paths(&tmp.join("ui/app.cui"), false).unwrap();
        assert_eq!(multi_stats.errors, 0);
        let multi_png = tmp.join("multi.png");
        render_cui_file_to_png(&tmp.join("ui/app.cui"), Some("App"), 240, 120, &multi_png).unwrap();
        assert!(std::fs::metadata(&multi_png).unwrap().len() > 100);

        // Doctor check against scaffolded project + packaging bundle generators (Web / Android / iOS)
        let doctor = collect_doctor_report(&tmp).unwrap();
        assert!(doctor.rustc_version.is_some());
        assert_eq!(doctor.cui_stats.errors, 0);
        assert!(format_doctor_json(&doctor).contains("\"cui\""));

        let web_out = tmp.join("dist/web");
        emit_web_bundle(&web_out, "demo_app", None).unwrap();
        let html = std::fs::read_to_string(web_out.join("index.html")).unwrap();
        assert!(html.contains("craftui-canvas") && html.contains("craftui_app.js"));

        let android_out = tmp.join("dist/android");
        emit_android_template(&android_out, "demo_app", "demo_app", None).unwrap();
        let act = std::fs::read_to_string(
            android_out.join("app/src/main/kotlin/dev/craftui/app/MainActivity.kt"),
        )
        .unwrap();
        assert!(act.contains("GameActivity") && act.contains("System.loadLibrary(\"demo_app\")"));
        assert!(android_out.join("app/src/main/AndroidManifest.xml").exists());

        let ios_out = tmp.join("dist/ios");
        emit_ios_template(&ios_out, "demo_app", "demo_app").unwrap();
        let hdr = std::fs::read_to_string(ios_out.join("CraftUI.h")).unwrap();
        let vc = std::fs::read_to_string(ios_out.join("CraftUIViewController.swift")).unwrap();
        assert!(hdr.contains("craftui_ios_app_attach_metal"));
        assert!(hdr.contains("craftui_ios_app_last_error"));
        assert!(vc.contains("CAMetalLayer") && vc.contains("craftui_ios_app_tick"));

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
