use super::{data::Dataset, report::Summary};
use anyhow::{Context, Result};
use plotters::{coord::Shift, prelude::*};
use std::{fs, path::Path};

mod lines;

const INK: RGBColor = RGBColor(25, 37, 49);
const MUTED: RGBColor = RGBColor(88, 103, 116);
const RUST: RGBColor = RGBColor(15, 118, 110);
const PERCENTILES: [RGBColor; 3] = [RUST, RGBColor(37, 99, 175), RGBColor(184, 104, 18)];
const GRID: RGBColor = RGBColor(229, 233, 238);

fn label(task: &str) -> &str {
    match task {
        "monte_carlo_pi" => "Monte Carlo pi",
        "prime_count" => "Prime count",
        "hash_search" => "Hash search",
        "sort_checksum" => "Sort checksum",
        "matmul_mod" => "Matrix checksum",
        _ => task,
    }
}

fn rasterize(path: &Path) -> Result<()> {
    let mut options = resvg::usvg::Options::default();
    options.fontdb_mut().load_system_fonts();
    let tree = resvg::usvg::Tree::from_data(&fs::read(path)?, &options)?;
    let size = tree.size().to_int_size();
    let mut pixmap = resvg::tiny_skia::Pixmap::new(size.width(), size.height())
        .context("allocating chart image")?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::default(),
        &mut pixmap.as_mut(),
    );
    pixmap.save_png(path.with_extension("png"))?;
    Ok(())
}

pub fn render(output: &Path, current: &Dataset, rows: &[Summary]) -> Result<()> {
    fs::create_dir_all(output)?;
    let path = output.join("latency-percentiles.svg");
    {
        let root = SVGBackend::new(&path, (1560, 1000)).into_drawing_area();
        root.fill(&WHITE)?;
        root.draw(&Text::new(
            "Task latency · p50 / p95 / p99",
            (36, 44),
            ("sans-serif", 32)
                .into_font()
                .style(FontStyle::Bold)
                .color(&INK),
        ))?;
        root.draw(&Text::new(
            format!(
                "{} runs per task · {} warmups · compute only · milliseconds · lower is better",
                current.samples_per_task, current.warmup_iterations
            ),
            (36, 79),
            ("sans-serif", 19).into_font().color(&MUTED),
        ))?;
        let area = root.margin(116, 52, 24, 24);
        let panels = area.split_evenly((2, 3));
        for (panel, row) in panels.iter().zip(rows) {
            percentile_panel(panel, row)?;
        }
        let legend = &panels[5];
        legend.draw(&Text::new(
            "Reading this figure",
            (40, 70),
            ("sans-serif", 23)
                .into_font()
                .style(FontStyle::Bold)
                .color(&INK),
        ))?;
        for (i, name) in [
            "p50 · median",
            "p95 · 95th percentile",
            "p99 · 99th percentile",
        ]
        .iter()
        .enumerate()
        {
            let y = 99 + i as i32 * 34;
            legend.draw(&Rectangle::new(
                [(40, y), (61, y + 21)],
                PERCENTILES[i].filled(),
            ))?;
            legend.draw(&Text::new(
                *name,
                (76, y + 17),
                ("sans-serif", 20).into_font().color(&INK),
            ))?;
        }
        for (i, text) in [
            "Each task has its own linear y-axis.",
            "All bars start at zero.",
            "Warmup excluded; no outliers removed.",
            "Exact values and raw samples: graph/README.md",
        ]
        .iter()
        .enumerate()
        {
            legend.draw(&Text::new(
                *text,
                (40, 240 + i as i32 * 26),
                ("sans-serif", 16).into_font().color(&MUTED),
            ))?;
        }
        root.draw(&Text::new(
            format!(
                "{} · {} · Fixed golden workloads; task sizes are listed in the report",
                current.host.cpu,
                &current.completed_at[..10]
            ),
            (36, 978),
            ("sans-serif", 16).into_font().color(&MUTED),
        ))?;
        root.present()?;
    }
    rasterize(&path)?;
    {
        let path = output.join("historical-comparison.svg");
        {
            let root = SVGBackend::new(&path, (1440, 760)).into_drawing_area();
            root.fill(&WHITE)?;
            root.draw(&Text::new(
                "Historical timing / Rust median",
                (36, 46),
                ("sans-serif", 32)
                    .into_font()
                    .style(FontStyle::Bold)
                    .color(&INK),
            ))?;
            root.draw(&Text::new(
                "Existing rounded verification log compared with new Rust p50 · approximate historical context",
                (36, 82),
                ("sans-serif", 20).into_font().color(&MUTED),
            ))?;
            let area = root.margin(118, 64, 28, 70);
            let max = rows
                .iter()
                .filter_map(|r| r.historical_ratio)
                .fold(1.0, f64::max)
                * 1.25;
            let mut chart = ChartBuilder::on(&area)
                .margin(12)
                .x_label_area_size(52)
                .y_label_area_size(195)
                .build_cartesian_2d(
                    0.0..max,
                    (0..500).with_key_points(vec![50, 150, 250, 350, 450]),
                )?;
            chart
                .configure_mesh()
                .disable_y_mesh()
                .light_line_style(WHITE)
                .bold_line_style(GRID)
                .axis_style(GRID)
                .label_style(("sans-serif", 19).into_font().color(&INK))
                .x_desc("Recorded historical time / current Rust p50")
                .axis_desc_style(("sans-serif", 19))
                .y_label_formatter(&|y: &i32| {
                    rows.get(4usize.saturating_sub((*y / 100) as usize))
                        .map(|r| label(&r.task_type).to_owned())
                        .unwrap_or_default()
                })
                .x_label_formatter(&|x| format!("{x:.0}x"))
                .draw()?;
            chart.draw_series(std::iter::once(PathElement::new(
                vec![(1.0, 0), (1.0, 500)],
                MUTED.stroke_width(1),
            )))?;
            for (i, row) in rows.iter().enumerate() {
                let y = 450 - i as i32 * 100;
                let Some(speedup) = row.historical_ratio else {
                    chart.draw_series(std::iter::once(Text::new(
                        "Unavailable: old log rounded to 0 ms",
                        (max * 0.04, y),
                        ("sans-serif", 18).into_font().color(&MUTED),
                    )))?;
                    continue;
                };
                chart.draw_series(std::iter::once(Rectangle::new(
                    [(0.0, y - 23), (speedup, y + 23)],
                    RUST.filled(),
                )))?;
                chart.draw_series(std::iter::once(Text::new(
                    format!("{speedup:.2}x"),
                    (speedup + max * 0.015, y),
                    ("sans-serif", 22)
                        .into_font()
                        .style(FontStyle::Bold)
                        .color(&INK),
                )))?;
            }
            root.draw(&Text::new(
                format!(
                    "Rust: {} runs per task. Old log: one observation rounded to 1 ms. Not a percentile-to-percentile comparison.",
                    current.samples_per_task
                ),
                (36, 731),
                ("sans-serif", 17).into_font().color(&MUTED),
            ))?;
            root.present()?;
        }
        rasterize(&path)?;
    }
    lines::render(output, current, rows)?;
    Ok(())
}

fn percentile_panel(panel: &DrawingArea<SVGBackend<'_>, Shift>, row: &Summary) -> Result<()> {
    let current = [row.current.p50_ms, row.current.p95_ms, row.current.p99_ms];
    let max = current.iter().copied().fold(0.0, f64::max) * 1.28;
    let mut chart = ChartBuilder::on(panel)
        .caption(
            label(&row.task_type),
            ("sans-serif", 24)
                .into_font()
                .style(FontStyle::Bold)
                .color(&INK),
        )
        .margin(22)
        .x_label_area_size(34)
        .y_label_area_size(67)
        .build_cartesian_2d((0..300).with_key_points(vec![50, 150, 250]), 0.0..max)?;
    chart
        .configure_mesh()
        .disable_x_mesh()
        .light_line_style(WHITE)
        .bold_line_style(GRID)
        .axis_style(GRID)
        .y_desc("ms")
        .label_style(("sans-serif", 17).into_font().color(&MUTED))
        .y_labels(5)
        .x_label_formatter(&|x: &i32| {
            match *x / 100 {
                0 => "p50",
                1 => "p95",
                _ => "p99",
            }
            .into()
        })
        .y_label_formatter(&|y| {
            if max < 0.1 {
                format!("{y:.3}")
            } else if max < 1.0 {
                format!("{y:.2}")
            } else if max < 10.0 {
                format!("{y:.1}")
            } else {
                format!("{y:.0}")
            }
        })
        .draw()?;
    for i in 0..3 {
        bar(
            &mut chart,
            i as i32 * 100 + 31,
            current[i],
            max,
            PERCENTILES[i],
        )?;
    }
    Ok(())
}

fn bar<'a, X: Ranged<ValueType = i32>, Y: Ranged<ValueType = f64>>(
    chart: &mut ChartContext<'a, SVGBackend<'_>, Cartesian2d<X, Y>>,
    x: i32,
    value: f64,
    max: f64,
    color: RGBColor,
) -> Result<()> {
    chart.draw_series(std::iter::once(Rectangle::new(
        [(x, 0.0), (x + 36, value)],
        color.filled(),
    )))?;
    chart.draw_series(std::iter::once(Text::new(
        format!("{value:.3}"),
        (x + 2, value + max * 0.045),
        ("sans-serif", 14).into_font().color(&INK),
    )))?;
    Ok(())
}
