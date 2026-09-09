use super::{GRID, INK, MUTED, PERCENTILES, label, rasterize};
use crate::{
    data::{Dataset, TaskSamples},
    report::Summary,
    statistics::summarize,
};
use anyhow::Result;
use plotters::{coord::Shift, prelude::*};
use std::path::Path;

const SAMPLE: RGBColor = RGBColor(148, 163, 184);
const CURVE: RGBColor = RGBColor(41, 71, 103);
const WINDOW: usize = 100;

#[derive(Clone, Copy)]
enum View {
    Runs,
    Distribution,
    Rolling,
}

impl View {
    fn filename(self) -> &'static str {
        match self {
            Self::Runs => "latency-runs.svg",
            Self::Distribution => "latency-distribution.svg",
            Self::Rolling => "latency-rolling-percentiles.svg",
        }
    }

    fn title(self) -> &'static str {
        match self {
            Self::Runs => "Latency across benchmark runs",
            Self::Distribution => "Cumulative latency distribution",
            Self::Rolling => "Rolling latency · p50 / p95 / p99",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::Runs => {
                "Every timed execution in measurement order · dashed lines mark whole-run percentiles"
            }
            Self::Distribution => {
                "Share of executions completed at or below each latency · markers show p50 / p95 / p99"
            }
            Self::Rolling => {
                "Trailing 100 executions per task · one window ending at each run from 100 onward"
            }
        }
    }
}

pub(super) fn render(output: &Path, current: &Dataset, rows: &[Summary]) -> Result<()> {
    for view in [View::Runs, View::Distribution, View::Rolling] {
        let path = output.join(view.filename());
        {
            let root = SVGBackend::new(&path, (1680, 1120)).into_drawing_area();
            root.fill(&WHITE)?;
            root.draw(&Text::new(
                view.title(),
                (36, 46),
                ("sans-serif", 34)
                    .into_font()
                    .style(FontStyle::Bold)
                    .color(&INK),
            ))?;
            root.draw(&Text::new(
                view.description(),
                (36, 83),
                ("sans-serif", 20).into_font().color(&MUTED),
            ))?;
            let panels = root.margin(124, 65, 24, 24).split_evenly((2, 3));
            for ((panel, task), row) in panels.iter().zip(&current.tasks).zip(rows) {
                draw_panel(panel, task, row, view)?;
            }
            legend(&panels[5], current, view)?;
            root.draw(&Text::new(
                format!(
                    "{} · {} · Compute only · Same saved measurements as the percentile report · No outliers removed",
                    current.host.cpu, &current.completed_at[..10],
                ),
                (36, 1093),
                ("sans-serif", 17).into_font().color(&MUTED),
            ))?;
            root.present()?;
        }
        rasterize(&path)?;
    }
    Ok(())
}

fn milliseconds(value: f64) -> String {
    if value == 0.0 {
        "0".into()
    } else if value < 0.1 {
        format!("{value:.3}")
    } else if value < 1.0 {
        format!("{value:.2}")
    } else {
        format!("{value:.1}")
    }
}

fn draw_panel(
    panel: &DrawingArea<SVGBackend<'_>, Shift>,
    task: &TaskSamples,
    row: &Summary,
    view: View,
) -> Result<()> {
    let count = task.samples_ns.len() as f64;
    let rolling = if matches!(view, View::Rolling) {
        task.samples_ns
            .windows(WINDOW)
            .map(summarize)
            .collect::<Result<Vec<_>>>()?
    } else {
        Vec::new()
    };
    let (x_range, y_max, x_label, y_label) = match view {
        View::Runs => (
            1.0..count,
            row.current.max_ms * 1.08,
            "Run number",
            "Latency (ms)",
        ),
        View::Distribution => (
            0.0..row.current.max_ms * 1.06,
            103.0,
            "Latency (ms)",
            "Runs at or below (%)",
        ),
        View::Rolling => (
            // At the minimum supported dataset size, there is one full window.
            WINDOW as f64..count.max(WINDOW as f64 + 1.0),
            rolling.iter().map(|s| s.p99_ms).fold(0.0, f64::max) * 1.12,
            "Window ending at run",
            "Latency (ms)",
        ),
    };
    let mut chart = ChartBuilder::on(panel)
        .caption(
            label(&task.task_type),
            ("sans-serif", 24)
                .into_font()
                .style(FontStyle::Bold)
                .color(&INK),
        )
        .margin(22)
        .x_label_area_size(56)
        .y_label_area_size(77)
        .build_cartesian_2d(x_range, 0.0..y_max)?;
    chart
        .configure_mesh()
        .light_line_style(WHITE)
        .bold_line_style(GRID)
        .axis_style(GRID)
        .label_style(("sans-serif", 16).into_font().color(&MUTED))
        .axis_desc_style(("sans-serif", 17).into_font().color(&MUTED))
        .x_desc(x_label)
        .y_desc(y_label)
        .x_labels(5)
        .y_labels(5)
        .x_label_formatter(&|x| match view {
            View::Distribution => milliseconds(*x),
            _ => format!("{x:.0}"),
        })
        .y_label_formatter(&|y| match view {
            View::Distribution => format!("{y:.0}"),
            _ => milliseconds(*y),
        })
        .draw()?;
    let percentiles = [row.current.p50_ms, row.current.p95_ms, row.current.p99_ms];
    match view {
        View::Runs => {
            chart.draw_series(LineSeries::new(
                task.samples_ns
                    .iter()
                    .enumerate()
                    .map(|(i, ns)| (i as f64 + 1.0, *ns as f64 / 1_000_000.0)),
                SAMPLE.stroke_width(1),
            ))?;
            for (value, color) in percentiles.into_iter().zip(PERCENTILES) {
                chart.draw_series(DashedLineSeries::new(
                    [(1.0, value), (count, value)],
                    7,
                    5,
                    color.stroke_width(2),
                ))?;
            }
        }
        View::Distribution => {
            let mut sorted = task.samples_ns.clone();
            sorted.sort_unstable();
            // Exact empirical CDF: horizontal between observations and a jump
            // of 1/n at every observation, including repeated equal values.
            let points =
                std::iter::once((0.0, 0.0)).chain(sorted.iter().enumerate().flat_map(|(i, ns)| {
                    let ms = *ns as f64 / 1_000_000.0;
                    [
                        (ms, i as f64 * 100.0 / count),
                        (ms, (i + 1) as f64 * 100.0 / count),
                    ]
                }));
            chart.draw_series(LineSeries::new(points, CURVE.stroke_width(2)))?;
            for ((value, percent), color) in percentiles
                .into_iter()
                .zip([50.0, 95.0, 99.0])
                .zip(PERCENTILES)
            {
                chart.draw_series(DashedLineSeries::new(
                    [(0.0, percent), (value, percent), (value, 0.0)],
                    5,
                    5,
                    color.mix(0.7).stroke_width(1),
                ))?;
                chart.draw_series(std::iter::once(Circle::new(
                    (value, percent),
                    4,
                    color.filled(),
                )))?;
            }
        }
        View::Rolling => {
            for (index, color) in PERCENTILES.into_iter().enumerate() {
                let points: Vec<_> = rolling
                    .iter()
                    .enumerate()
                    .map(|(i, stats)| {
                        (
                            (i + WINDOW) as f64,
                            [stats.p50_ms, stats.p95_ms, stats.p99_ms][index],
                        )
                    })
                    .collect();
                chart.draw_series(LineSeries::new(
                    points.iter().copied(),
                    color.stroke_width(2),
                ))?;
                if points.len() == 1 {
                    chart.draw_series(
                        points
                            .into_iter()
                            .map(|point| Circle::new(point, 4, color.filled())),
                    )?;
                }
            }
        }
    }
    Ok(())
}

fn legend(panel: &DrawingArea<SVGBackend<'_>, Shift>, current: &Dataset, view: View) -> Result<()> {
    panel.draw(&Text::new(
        "Reading this figure",
        (42, 58),
        ("sans-serif", 24)
            .into_font()
            .style(FontStyle::Bold)
            .color(&INK),
    ))?;
    for (i, (text, color)) in [
        "p50 · median",
        "p95 · 95th percentile",
        "p99 · 99th percentile",
    ]
    .into_iter()
    .zip(PERCENTILES)
    .enumerate()
    {
        let y = 100 + i as i32 * 37;
        panel.draw(&PathElement::new([(42, y), (78, y)], color.stroke_width(3)))?;
        panel.draw(&Text::new(
            text,
            (93, y + 6),
            ("sans-serif", 20).into_font().color(&INK),
        ))?;
    }
    let notes = match view {
        View::Runs => [
            "Gray line: every measured execution.",
            "Dashed lines: percentiles of the full run.",
            "Independent linear y-axes start at zero.",
            "Spikes remain visible up to the maximum.",
            "Run number is per task, not wall-clock time.",
        ],
        View::Distribution => [
            "Dark line: empirical cumulative distribution.",
            "Colored markers: nearest-rank percentiles.",
            "Each task has its own linear latency axis.",
            "The full tail is shown through the maximum.",
            "At p95, at least 95% of runs are this fast.",
        ],
        View::Rolling => [
            "Each point summarizes the last 100 runs.",
            "Windows overlap; warmups are excluded.",
            "Independent linear y-axes start at zero.",
            "This shows variation within one benchmark.",
            "It does not compare different code versions.",
        ],
    };
    for (i, text) in notes.into_iter().enumerate() {
        panel.draw(&Text::new(
            text,
            (42, 228 + i as i32 * 27),
            ("sans-serif", 17).into_font().color(&MUTED),
        ))?;
    }
    panel.draw(&Text::new(
        format!(
            "{} runs per task · {} warmups excluded",
            current.samples_per_task, current.warmup_iterations
        ),
        (42, 397),
        ("sans-serif", 17).into_font().color(&INK),
    ))?;
    Ok(())
}
