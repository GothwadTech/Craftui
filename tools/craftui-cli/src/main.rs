use clap::{Parser, Subcommand, ValueEnum};
use craftui_core::{CraftError, Result, Size, WindowMetrics};
use craftui_dsl::{Severity, generate_rust, parse, validate};
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
    /// Build a CraftUI application for desktop, web, android, or ios
    Build {
        #[arg(long, value_enum, default_value_t = PlatformTarget::Desktop)]
        platform: PlatformTarget,
        #[arg(long)]
        release: bool,
        #[arg(short, long)]
        package: Option<String>,
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
            let source = std::fs::read_to_string(&file).map_err(|e| {
                CraftError::Platform(format!("failed reading {}: {e}", file.display()))
            })?;
            render_cui_to_png(&source, component.as_deref(), width, height, &out)?;
            println!(
                "Rendered {}x{} snapshot to {}",
                width,
                height,
                out.display()
            );
            Ok(())
        }
        Commands::Build {
            platform,
            release,
            package,
        } => run_build(platform, release, package.as_deref()),
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

        Text {
            text: "CraftUI App";
            font_size: 24;
            color: Theme.text_primary;
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
                on_tap => { count = count + 1; }
            }
            Button {
                text: "Reset";
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

#[derive(Debug, Default, PartialEq, Eq)]
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
        let src = std::fs::read_to_string(&f)
            .map_err(|e| CraftError::Platform(format!("failed reading {}: {e}", f.display())))?;
        let parsed = parse(&src);
        let mut diags = parsed.diagnostics;
        diags.extend(validate(&parsed.file));
        for d in &diags {
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
            let code = generate_rust(&parsed.file);
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
        for p in event.paths {
            if p.extension().and_then(|e| e.to_str()) == Some("cui")
                && let Ok(src) = std::fs::read_to_string(&p)
            {
                let t0 = Instant::now();
                match session.reload(&src, None, &mut tree) {
                    Ok(_) => println!(
                        "[hot-reload] {} synced in {:.2}ms",
                        p.display(),
                        t0.elapsed().as_secs_f64() * 1000.0
                    ),
                    Err(e) => eprintln!("[hot-reload] {}: {e}", p.display()),
                }
            }
        }
    }
    Ok(())
}

fn run_build(platform: PlatformTarget, release: bool, package: Option<&str>) -> Result<()> {
    let mut cmd = Command::new("cargo");
    match platform {
        PlatformTarget::Desktop => {
            cmd.arg("build");
            if let Some(pkg) = package {
                cmd.args(["-p", pkg]);
            }
            if release {
                cmd.arg("--release");
            }
        }
        PlatformTarget::Web => {
            cmd.args(["build", "--target", "wasm32-unknown-unknown"]);
            if let Some(pkg) = package {
                cmd.args(["-p", pkg]);
            } else {
                cmd.args(["-p", "craftui-platform-web"]);
            }
            if release {
                cmd.arg("--release");
            }
        }
        PlatformTarget::Android => {
            cmd.args(["build", "--target", "aarch64-linux-android"]);
            if let Some(pkg) = package {
                cmd.args(["-p", pkg]);
            } else {
                cmd.args(["-p", "craftui-platform-android"]);
            }
            if release {
                cmd.arg("--release");
            }
        }
        PlatformTarget::Ios => {
            cmd.args(["build", "--target", "aarch64-apple-ios"]);
            if let Some(pkg) = package {
                cmd.args(["-p", pkg]);
            } else {
                cmd.args(["-p", "craftui-platform-ios"]);
            }
            if release {
                cmd.arg("--release");
            }
        }
    }

    let status = cmd
        .status()
        .map_err(|e| CraftError::Platform(format!("failed spawning cargo: {e}")))?;
    if !status.success() {
        return Err(CraftError::Platform(format!(
            "cargo build exited with status {status}"
        )));
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

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
