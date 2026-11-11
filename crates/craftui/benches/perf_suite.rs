use craftui::layout::LayoutStyle;
use craftui::prelude::*;
use craftui::reactive::reset_runtime;
use craftui::text::TextEngine;
use std::cell::Cell;
use std::hint::black_box;
use std::rc::Rc;
use std::time::{Duration, Instant};

#[derive(Clone, Copy)]
struct Config {
    quick: bool,
    json: bool,
    list: bool,
}

#[derive(Clone)]
struct BenchStats {
    name: &'static str,
    iterations: usize,
    median_us: f64,
    p95_us: f64,
    min_us: f64,
    max_us: f64,
    mean_us: f64,
    detail: String,
}

fn parse_args() -> Config {
    let mut cfg = Config {
        quick: false,
        json: false,
        list: false,
    };
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--quick" => cfg.quick = true,
            "--json" => cfg.json = true,
            "--list" => cfg.list = true,
            "--help" | "-h" => {
                println!(
                    "CraftUI zero-dep microbenchmark suite\n\
                     Usage: perf_suite [--quick] [--json] [--list]\n\
                       --quick   Fewer iterations (~1-2s total) for CI smoke checks\n\
                       --json    Emit machine-readable JSON metrics\n\
                       --list    List available benchmark names"
                );
                std::process::exit(0);
            }
            // Cargo passes `--bench` (and libtest-like runners may pass `--quiet` / `--nocapture`).
            "--bench" | "--quiet" | "--nocapture" => {}
            _ => {}
        }
    }
    cfg
}

fn percentile_sorted(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    if sorted.len() == 1 {
        return sorted[0];
    }
    let pos = ((sorted.len() - 1) as f64) * p.clamp(0.0, 1.0);
    let lo = pos.floor() as usize;
    let hi = pos.ceil() as usize;
    if lo == hi {
        sorted[lo]
    } else {
        let frac = pos - lo as f64;
        sorted[lo] * (1.0 - frac) + sorted[hi] * frac
    }
}

fn compute_stats(name: &'static str, mut samples_us: Vec<f64>, detail: String) -> BenchStats {
    samples_us.sort_by(f64::total_cmp);
    let iterations = samples_us.len();
    let min_us = samples_us.first().copied().unwrap_or(0.0);
    let max_us = samples_us.last().copied().unwrap_or(0.0);
    let sum: f64 = samples_us.iter().sum();
    let mean_us = if iterations > 0 {
        sum / iterations as f64
    } else {
        0.0
    };
    let median_us = percentile_sorted(&samples_us, 0.50);
    let p95_us = percentile_sorted(&samples_us, 0.95);
    BenchStats {
        name,
        iterations,
        median_us,
        p95_us,
        min_us,
        max_us,
        mean_us,
        detail,
    }
}

fn measure_samples<F>(warmup: usize, iterations: usize, mut f: F) -> Vec<f64>
where
    F: FnMut(usize) -> Duration,
{
    for i in 0..warmup {
        black_box(f(i));
    }
    let mut samples_us = Vec::with_capacity(iterations);
    for i in 0..iterations {
        let dur = f(warmup + i);
        samples_us.push(dur.as_secs_f64() * 1_000_000.0);
    }
    samples_us
}

fn bench_reactive_signal_fanout_10k(quick: bool) -> BenchStats {
    reset_runtime();
    let cx = Scope::root();

    const NUM_SIGNALS: usize = 1_000;
    const EFFECTS_PER_COMPUTED: usize = 9;
    // 1,000 signals -> 1,000 computeds + 9,000 effects = 10,000 downstream subscribers.
    let signals: Vec<Signal<i64>> = (0..NUM_SIGNALS).map(|i| cx.signal(i as i64)).collect();
    let computeds: Vec<Computed<i64>> = (0..NUM_SIGNALS)
        .map(|i| {
            let a = signals[i];
            let b = signals[(i + 1) % NUM_SIGNALS];
            cx.computed(move || a.get() + b.get())
        })
        .collect();

    let accumulator = Rc::new(Cell::new(0i64));
    for comp in &computeds {
        let comp = *comp;
        for _ in 0..EFFECTS_PER_COMPUTED {
            let acc = accumulator.clone();
            cx.effect(move || {
                acc.set(acc.get().wrapping_add(comp.get()));
            });
        }
    }

    let warmup = if quick { 2 } else { 5 };
    let iterations = if quick { 12 } else { 40 };

    let samples = measure_samples(warmup, iterations, |step| {
        accumulator.set(0);
        let base = (step as i64) + 1;
        let t0 = Instant::now();
        batch(|| {
            for (i, sig) in signals.iter().enumerate() {
                sig.set(base + i as i64);
            }
        });
        let elapsed = t0.elapsed();
        black_box(accumulator.get());
        elapsed
    });

    cx.dispose();
    reset_runtime();

    compute_stats(
        "reactive_signal_fanout_10k",
        samples,
        "1,000 signals -> 1,000 computeds + 9,000 effects (batched)".to_string(),
    )
}

fn bench_text_shaping_cache_cold_vs_warm(quick: bool) -> (BenchStats, BenchStats, f64) {
    let mut engine = TextEngine::new();
    const BATCH_STRINGS: usize = 64;
    let corpus: Vec<String> = (0..BATCH_STRINGS)
        .map(|i| {
            format!(
                "CraftUI Metric #{i:03} — shaping pipeline latency {:.2} ms (status: active)",
                i as f32 * 1.37
            )
        })
        .collect();

    let cold_style = TextStyle::default().with_size(14.0);
    // Color changes must still hit ShapedLayoutCache because shaping geometry is color-agnostic.
    let warm_style = TextStyle::default()
        .with_size(14.0)
        .with_color(Color::rgb(59, 130, 246));

    let cold_warmup = if quick { 1 } else { 3 };
    let cold_iters = if quick { 10 } else { 30 };

    let cold_samples = measure_samples(cold_warmup, cold_iters, |_| {
        engine.clear_cache();
        let (_, misses_before) = engine.cache_stats();
        let t0 = Instant::now();
        for text in &corpus {
            black_box(engine.build_layout_cached(text, &cold_style, Some(280.0)));
        }
        let elapsed = t0.elapsed();
        let (_, misses_after) = engine.cache_stats();
        debug_assert_eq!(misses_after - misses_before, BATCH_STRINGS as u64);
        elapsed
    });

    // Ensure all 64 entries are warm in cache before measuring warm hits.
    engine.clear_cache();
    for text in &corpus {
        black_box(engine.build_layout_cached(text, &cold_style, Some(280.0)));
    }

    const WARM_REPEATS: usize = 16;
    let warm_warmup = if quick { 2 } else { 6 };
    let warm_iters = if quick { 20 } else { 60 };

    let warm_samples = measure_samples(warm_warmup, warm_iters, |_| {
        let (hits_before, _) = engine.cache_stats();
        let t0 = Instant::now();
        for _ in 0..WARM_REPEATS {
            for text in &corpus {
                black_box(engine.build_layout_cached(text, &warm_style, Some(280.0)));
            }
        }
        let elapsed = t0.elapsed();
        let (hits_after, _) = engine.cache_stats();
        debug_assert_eq!(
            hits_after - hits_before,
            (BATCH_STRINGS * WARM_REPEATS) as u64
        );
        elapsed.div_f64(WARM_REPEATS as f64)
    });

    let cold_stats = compute_stats(
        "text_shaping_cache_cold_miss",
        cold_samples,
        format!("{BATCH_STRINGS} strings shaped cold (cache cleared)"),
    );
    let speedup_x = if warm_samples.is_empty() {
        1.0
    } else {
        let warm_tmp = compute_stats("tmp", warm_samples.clone(), String::new());
        cold_stats.median_us / warm_tmp.median_us.max(0.001)
    };
    let warm_stats = compute_stats(
        "text_shaping_cache_warm_hit",
        warm_samples,
        format!("{BATCH_STRINGS} strings warm ShapedLayoutCache hit ({speedup_x:.1}x speedup)"),
    );

    (cold_stats, warm_stats, speedup_x)
}

fn bench_virtual_list_10k_scroll_layout(quick: bool) -> BenchStats {
    reset_runtime();
    let cx = Scope::root();
    let mut tree = WidgetTree::new(cx);

    let metrics = WindowMetrics {
        logical_size: Size::new(480.0, 640.0),
        scale_factor: 1.0,
        ..Default::default()
    };

    let vlist = tree.virtual_list(
        cx,
        Axis::Vertical,
        1,
        44.0,
        10_000usize,
        LayoutStyle::column().with_size(Length::Dp(480.0), Length::Dp(640.0)),
        Rc::new(|scope, idx, t| {
            let row = t.container(
                scope,
                LayoutStyle::row()
                    .with_padding(Insets::symmetric(12.0, 8.0))
                    .with_gap(10.0)
                    .with_align(Alignment::Center),
                BoxStyle::new(),
            );
            let badge = t.badge(scope, format!("#{idx:04}"));
            let label = t.text(
                scope,
                format!("Virtual row #{idx:05} — telemetry payload"),
                TextStyle::default().with_size(14.0),
            );
            t.append_child(row, badge);
            t.append_child(row, label);
            row
        }),
    );
    tree.set_root(vlist);
    tree.layout(metrics);

    let warmup = if quick { 12 } else { 30 };
    let iterations = if quick { 60 } else { 200 };

    let samples = measure_samples(warmup, iterations, |step| {
        // Reverse scroll direction periodically so we stay within valid [0, max_off] bounds.
        let dy = if (step / 250) % 2 == 0 { -36.0 } else { 36.0 };
        let t0 = Instant::now();
        let handled = tree.handle_event(&UiEvent::Scroll(ScrollDelta {
            x: 0.0,
            y: dy,
            position: Point::new(240.0, 320.0),
        }));
        let dirty = tree.layout(metrics);
        let elapsed = t0.elapsed();
        black_box((handled, dirty));
        elapsed
    });

    let active_nodes = tree.nodes.len();
    assert!(
        active_nodes < 160,
        "VirtualList should keep active nodes windowed (< 160), got {active_nodes}"
    );
    reset_runtime();

    compute_stats(
        "virtual_list_10k_scroll_layout",
        samples,
        format!("10,000 rows, incremental 36dp scroll + layout ({active_nodes} live nodes)"),
    )
}

#[component]
fn bench_card_view(cx: Scope, level: Signal<f32>, toggle: Signal<bool>) -> impl View {
    cui! {
        Column(
            width: pct(100),
            height: pct(100),
            bg: theme.background,
            padding: 16,
            gap: 10
        ) {
            Row(width: pct(100), align: Alignment::Center, gap: 8) {
                Icon(svg: "check", size: 20, color: theme.primary)
                Text(text: "CraftUI Render Card", size: 18, color: theme.text_primary)
                Badge(label: "400x300", icon: "check", bg: theme.primary, fg: theme.on_primary)
            }
            Container(
                width: pct(100),
                bg: theme.surface,
                border_width: 1,
                border_color: theme.border,
                radius: 10,
                shadow: true,
                padding: 12,
                gap: 8
            ) {
                Text(
                    text: "Rounded card, drop shadow, text glyphs & controls",
                    size: 13,
                    color: theme.text_secondary
                )
                Slider(value: level, min: 0.0, max: 1.0, label: "Quality")
                Row(width: pct(100), align: Alignment::Center, gap: 12) {
                    Switch(checked: toggle, label: "Subpixel AA")
                    Button(label: "Apply", bg: theme.primary, fg: theme.on_primary, on_tap: || {})
                }
            }
        }
    }
}

fn bench_paint_and_rasterize_400x300(quick: bool) -> (BenchStats, BenchStats, BenchStats) {
    reset_runtime();
    let cx = Scope::root();
    let mut tree = WidgetTree::new(cx);
    let level = cx.signal(0.68f32);
    let toggle = cx.signal(true);

    tree.mount(cx, bench_card_view(cx, level, toggle));
    let metrics = WindowMetrics {
        logical_size: Size::new(400.0, 300.0),
        scale_factor: 1.0,
        ..Default::default()
    };
    tree.layout(metrics);

    let mut scene = PaintScene::new();
    let textures = TextureRegistry::new();
    let bg = tree.theme().background;

    // Warmup & verify pixel buffer dimensions on both CPU backends.
    tree.paint(&mut scene);
    let tiny_check = render_scene_to_rgba(
        BackendKind::TinySkia,
        &scene,
        400,
        300,
        1.0,
        bg,
        &textures,
        &mut tree.text_engine,
    );
    assert_eq!(tiny_check.len(), 400 * 300 * 4);
    let vello_check = render_scene_to_rgba(
        BackendKind::VelloCpu,
        &scene,
        400,
        300,
        1.0,
        bg,
        &textures,
        &mut tree.text_engine,
    );
    assert_eq!(vello_check.len(), 400 * 300 * 4);

    let paint_warmup = if quick { 5 } else { 15 };
    let paint_iters = if quick { 40 } else { 120 };
    let paint_samples = measure_samples(paint_warmup, paint_iters, |_| {
        let t0 = Instant::now();
        tree.paint(&mut scene);
        let elapsed = t0.elapsed();
        black_box(scene.commands.len());
        elapsed
    });

    let rast_warmup = if quick { 2 } else { 5 };
    let rast_iters = if quick { 15 } else { 40 };

    let tinyskia_samples = measure_samples(rast_warmup, rast_iters, |_| {
        let t0 = Instant::now();
        tree.paint(&mut scene);
        let rgba = render_scene_to_rgba(
            BackendKind::TinySkia,
            &scene,
            400,
            300,
            1.0,
            bg,
            &textures,
            &mut tree.text_engine,
        );
        let elapsed = t0.elapsed();
        black_box(rgba[0]);
        elapsed
    });

    let vellocpu_samples = measure_samples(rast_warmup, rast_iters, |_| {
        let t0 = Instant::now();
        tree.paint(&mut scene);
        let rgba = render_scene_to_rgba(
            BackendKind::VelloCpu,
            &scene,
            400,
            300,
            1.0,
            bg,
            &textures,
            &mut tree.text_engine,
        );
        let elapsed = t0.elapsed();
        black_box(rgba[0]);
        elapsed
    });

    let cmd_count = scene.commands.len();
    reset_runtime();

    let paint_stats = compute_stats(
        "paint_scene_400x300",
        paint_samples,
        format!("PaintScene build ({cmd_count} commands)"),
    );
    let tiny_stats = compute_stats(
        "paint_and_rasterize_400x300_tinyskia",
        tinyskia_samples,
        format!("PaintScene + BackendKind::TinySkia 400x300 ({cmd_count} cmds)"),
    );
    let vello_stats = compute_stats(
        "paint_and_rasterize_400x300_vellocpu",
        vellocpu_samples,
        format!("PaintScene + BackendKind::VelloCpu 400x300 ({cmd_count} cmds)"),
    );

    (paint_stats, tiny_stats, vello_stats)
}

fn format_duration_us(us: f64) -> String {
    if us >= 1_000.0 {
        format!("{:.2} ms", us / 1_000.0)
    } else {
        format!("{us:.2} us")
    }
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

fn print_human_table(results: &[BenchStats], speedup_x: f64, quick: bool, total_ms: f64) {
    let mode = if quick { "quick" } else { "standard" };
    println!("CraftUI Performance Suite (mode: {mode}, total: {total_ms:.1} ms)");
    println!(
        "{:<38} {:>6} {:>12} {:>12}  Detail",
        "Benchmark", "Iters", "Median", "p95"
    );
    println!("{}", "-".repeat(110));
    for r in results {
        println!(
            "{:<38} {:>6} {:>12} {:>12}  {}",
            r.name,
            r.iterations,
            format_duration_us(r.median_us),
            format_duration_us(r.p95_us),
            r.detail
        );
    }
    println!("{}", "-".repeat(110));
    println!("text_shaping_cache cold->warm speedup: {speedup_x:.1}x");
}

fn print_json_report(results: &[BenchStats], speedup_x: f64, quick: bool, total_ms: f64) {
    let mode = if quick { "quick" } else { "standard" };
    println!("{{");
    println!("  \"suite\": \"craftui_perf_suite\",");
    println!("  \"mode\": {},", json_escape(mode));
    println!("  \"total_elapsed_ms\": {:.3},", total_ms);
    println!("  \"text_shaping_speedup_x\": {:.3},", speedup_x);
    println!("  \"benchmarks\": [");
    for (idx, r) in results.iter().enumerate() {
        let comma = if idx + 1 < results.len() { "," } else { "" };
        println!("    {{");
        println!("      \"name\": {},", json_escape(r.name));
        println!("      \"iterations\": {},", r.iterations);
        println!("      \"median_us\": {:.3},", r.median_us);
        println!("      \"p95_us\": {:.3},", r.p95_us);
        println!("      \"min_us\": {:.3},", r.min_us);
        println!("      \"max_us\": {:.3},", r.max_us);
        println!("      \"mean_us\": {:.3},", r.mean_us);
        println!("      \"detail\": {}", json_escape(&r.detail));
        println!("    }}{comma}");
    }
    println!("  ]");
    println!("}}");
}

fn main() {
    let cfg = parse_args();
    if cfg.list {
        for name in [
            "reactive_signal_fanout_10k",
            "text_shaping_cache_cold_vs_warm",
            "virtual_list_10k_scroll_layout",
            "paint_and_rasterize_400x300",
        ] {
            println!("{name}");
        }
        return;
    }

    let suite_t0 = Instant::now();
    let reactive = bench_reactive_signal_fanout_10k(cfg.quick);
    let (text_cold, text_warm, speedup_x) = bench_text_shaping_cache_cold_vs_warm(cfg.quick);
    let virtual_list = bench_virtual_list_10k_scroll_layout(cfg.quick);
    let (paint_scene, raster_tiny, raster_vello) = bench_paint_and_rasterize_400x300(cfg.quick);
    let total_ms = suite_t0.elapsed().as_secs_f64() * 1_000.0;

    let results = vec![
        reactive,
        text_cold,
        text_warm,
        virtual_list,
        paint_scene,
        raster_tiny,
        raster_vello,
    ];

    if cfg.json {
        print_json_report(&results, speedup_x, cfg.quick, total_ms);
    } else {
        print_human_table(&results, speedup_x, cfg.quick, total_ms);
    }
}
