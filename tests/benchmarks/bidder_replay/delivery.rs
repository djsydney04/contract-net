//! Inspect archived timing residuals and replay their variation without lookahead.
use super::{Measurements, Scenario, graphs, historical::Cases, replay, replay_delays};
use anyhow::Result;
use plotters::prelude::*;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fmt::Write as _, fs, path::Path};

const INK: RGBColor = RGBColor(25, 37, 49);
const TEAL: RGBColor = RGBColor(15, 118, 110);
const BLUE: RGBColor = RGBColor(37, 99, 235);
const GRAY: RGBColor = RGBColor(100, 116, 139);

fn percentile(sorted: &[f64], percent: usize) -> f64 {
    sorted[(percent * sorted.len()).div_ceil(100).saturating_sub(1)]
}

pub(super) fn report(output: &Path, data: &Measurements, cases: &Cases) -> Result<String> {
    let delays: Vec<_> = cases.delivery.iter().map(|r| r.residual_ms).collect();
    let variable = replay_delays(data, &cases.tasks, &delays, 1)?;
    let fixed = replay(data, &cases.tasks, 50.0, 1)?;
    let mut sorted = delays.clone();
    sorted.sort_by(f64::total_cmp);
    let p50 = percentile(&sorted, 50);
    let p95 = percentile(&sorted, 95);
    let p99 = percentile(&sorted, 99);
    overhead(output, cases, &variable)?;
    distribution(output, cases, &sorted)?;

    let mut hashes = BTreeMap::new();
    for path in [
        "src/bidder.rs",
        "src/strategy.rs",
        "src/market.rs",
        "src/benchmark.rs",
        "tests/benchmarks/bidder_replay/main.rs",
        "tests/benchmarks/bidder_replay/historical.rs",
        "tests/benchmarks/bidder_replay/delivery.rs",
        "tests/benchmarks/bidder_replay/graphs.rs",
    ] {
        hashes.insert(path, format!("{:x}", Sha256::digest(fs::read(path)?)));
    }
    fs::write(
        output.join("delivery-replay.json"),
        serde_json::to_string_pretty(&serde_json::json!({
            "version": 1,
            "method": "Same archived residual per auction for both bidders; chronological, empty initial learning, learn after simulated wins only.",
            "limitations": "Residuals belong to archived competitors, not this client. Manager runtime is rounded to 10 ms; residual includes unseparated queue/processing. Small reconstructed subset and fixed competition.",
            "measurement_source_commit": data.source_commit,
            "measurement_completed_at": data.completed_at,
            "analysis_sources_sha256": hashes,
            "parsed_measurements_sha256": format!("{:x}", Sha256::digest(serde_json::to_vec(data)?)),
            "parsed_delivery_records_sha256": format!("{:x}", Sha256::digest(serde_json::to_vec(&cases.delivery)?)),
            "records": cases.delivery,
            "empirical_residual_ms": {"samples": sorted.len(), "min": sorted[0], "p50": p50, "p95": p95, "p99": p99, "max": sorted[sorted.len()-1]},
            "fixed_50_ms": fixed,
            "varying_archived_residual": variable,
        }))? + "\n",
    )?;
    let mut csv = String::from(
        "auction,task_id,task_type,agent,local_seconds,manager_seconds,residual_ms,prior_delivery_allowance_ms\n",
    );
    for (i, (r, o)) in cases.delivery.iter().zip(&variable.outcomes).enumerate() {
        // JSON is the authoritative export; quote and escape arbitrary text in CSV.
        writeln!(
            csv,
            "{},{},\"{}\",\"{}\",{:.4},{:.2},{:.4},{:.4}",
            i + 1,
            r.task_id,
            r.task_type.replace('"', "\"\""),
            r.agent.replace('"', "\"\""),
            r.local_seconds,
            r.manager_seconds,
            r.residual_ms,
            o.forecast_delivery_ms
        )?;
    }
    fs::write(output.join("delivery-observations.csv"), csv)?;
    let mut doc = format!(
        "\n## Shared versus varying delivery overhead\n\n\
        A shared baseline is a useful starting estimate. An identical delay for every\n\
        auction hides variation. The current bidder pools overhead observations across\n\
        task types and uses their p95 plus 5 ms; before observations it allows 55 ms.\n\
        Compute and queue estimates remain separate.\n\n\
        ![Archived residual and allowance available before each auction](delivery-overhead.svg)\n\n\
        The archived residual is `manager_seconds - local_seconds`. These {} records\n\
        belong to the competitor named in the fixture, **not this client's network**.\n\
        They range from {:.1} to {:.1} ms. They can include queueing, processing, and\n\
        timing differences; the manager values were rounded to 10 ms. Treat them as\n\
        a proxy for a varying-overhead scenario, not measured network RTT. The blue\n\
        allowance uses only earlier simulated wins, before the current delay is known.\n\n\
        ![Individual overhead observations by task type and empirical distribution](delivery-distribution.svg)\n\n\
        Across this subset, empirical p50/p95/p99 are {:.1}/{:.1}/{:.1} ms.\n\
        With {} samples, upper percentiles can select the same maximum observation\n\
        and do not establish reliable tail latency. Per-task groups are smaller still.\n\
        Different group averages do not establish a task-type effect.\n\n\
        ![Fixed versus varying overhead profit replay](variable-delivery-profit.svg)\n\n\
        The varying replay uses each archived residual for its corresponding auction.\n\
        Both bidders receive the same realized delay for that auction. Each auction\n\
        is replayed once in chronological order with empty initial learning; the\n\
        current/future delay is never provided to the bidding decision. The fixed\n\
        50 ms comparison uses the same tasks, compute samples, and competitor bids.\n\n\
        | Overhead scenario | Old modeled profit | New modeled profit | Old/new wins | Old/new losing contracts |\n\
        |---|---:|---:|---:|---:|\n",
        sorted.len(),
        sorted[0],
        sorted[sorted.len() - 1],
        p50,
        p95,
        p99,
        sorted.len()
    );
    for (label, s) in [
        ("Fixed 50 ms", &fixed),
        ("Varying archived residual", &variable),
    ] {
        let last = s.outcomes.last().unwrap();
        writeln!(
            doc,
            "| {label} | {:.4} | {:.4} | {} / {} | {} / {} |",
            last.old_cumulative,
            last.new_cumulative,
            s.old_wins,
            s.new_wins,
            s.old_losses,
            s.new_losses
        )?;
        println!(
            "{label}: old profit {:.4}, new {:.4}; wins {}/{}",
            last.old_cumulative, last.new_cumulative, s.old_wins, s.new_wins
        );
    }
    doc.push_str("\nAll values remain counterfactual: inputs are reconstructed as described above,\ncompetition is held fixed, and applying another contractor's overhead to our\nclient is an explicit assumption. The saved compute and decision timings are\nreused unchanged; these new plots are not a fresh latency measurement.\n\n[Individual residuals and prior forecasts](delivery-observations.csv) ·\n[Replay outcomes and analysis source hashes](delivery-replay.json).\n");
    graphs::render(
        output,
        &[fixed, variable],
        "variable-delivery-profit",
        "Historical profit: shared versus varying delivery",
    )?;
    Ok(doc)
}

fn overhead(output: &Path, cases: &Cases, scenario: &Scenario) -> Result<()> {
    let path = output.join("delivery-overhead.svg");
    {
        let root = SVGBackend::new(&path, (1680, 760)).into_drawing_area();
        root.fill(&WHITE)?;
        root.draw(&Text::new(
            "Does one delivery allowance fit every auction?",
            (35, 45),
            ("sans-serif", 32)
                .into_font()
                .style(FontStyle::Bold)
                .color(&INK),
        ))?;
        root.draw(&Text::new("Archived competitor residuals and the allowance available before each simulated decision", (35, 80),
            ("sans-serif", 20).into_font().color(&INK)))?;
        let max = scenario
            .outcomes
            .iter()
            .flat_map(|o| [o.delivery_ms, o.forecast_delivery_ms])
            .fold(55.0, f64::max)
            * 1.25;
        let panel = root.margin(115, 100, 30, 30);
        let mut chart = ChartBuilder::on(&panel)
            .margin(15)
            .x_label_area_size(65)
            .y_label_area_size(75)
            .build_cartesian_2d(0.5..cases.delivery.len() as f64 + 0.5, 0.0..max)?;
        chart
            .configure_mesh()
            .x_desc("Historical auction in chronological order (task ID)")
            .y_desc("Overhead / allowance (ms)")
            .x_labels(cases.delivery.len())
            .x_label_formatter(&|x| {
                let i = x.round() as usize;
                if i > 0 && i <= cases.delivery.len() && (x - i as f64).abs() < 0.01 {
                    format!("#{:02}", cases.delivery[i - 1].task_id)
                } else {
                    String::new()
                }
            })
            .label_style(("sans-serif", 18))
            .axis_desc_style(("sans-serif", 18))
            .light_line_style(WHITE)
            .bold_line_style(RGBColor(229, 233, 238))
            .draw()?;
        for (label, color, points) in [
            (
                "Archived residual",
                TEAL,
                scenario
                    .outcomes
                    .iter()
                    .map(|o| (o.auction as f64, o.delivery_ms))
                    .collect::<Vec<_>>(),
            ),
            (
                "Prior learned allowance",
                BLUE,
                scenario
                    .outcomes
                    .iter()
                    .map(|o| (o.auction as f64, o.forecast_delivery_ms))
                    .collect(),
            ),
            (
                "Fixed 50 ms assumption",
                GRAY,
                vec![(0.5, 50.0), (cases.delivery.len() as f64 + 0.5, 50.0)],
            ),
        ] {
            chart
                .draw_series(LineSeries::new(points.clone(), color.stroke_width(3)))?
                .label(label)
                .legend(move |(x, y)| {
                    PathElement::new([(x, y), (x + 25, y)], color.stroke_width(3))
                });
            if color != GRAY {
                chart.draw_series(points.iter().map(|&p| Circle::new(p, 5, color.filled())))?;
            }
        }
        chart
            .configure_series_labels()
            .position(SeriesLabelPosition::LowerRight)
            .background_style(WHITE.mix(0.95))
            .label_font(("sans-serif", 18))
            .draw()?;
        root.draw(&Text::new("Residual = manager time minus reported compute; may include queue/processing. Manager time rounded to 10 ms.",
            (35,700),("sans-serif",18).into_font().color(&INK)))?;
        root.draw(&Text::new("The delay varies across auctions; both policies face the same per-auction delay. This is not our measured network latency.",
            (35,730),("sans-serif",18).into_font().color(&INK)))?;
        root.present()?;
    }
    graphs::rasterize(&path)
}

fn distribution(output: &Path, cases: &Cases, sorted: &[f64]) -> Result<()> {
    let path = output.join("delivery-distribution.svg");
    {
        let root = SVGBackend::new(&path, (1680, 760)).into_drawing_area();
        root.fill(&WHITE)?;
        root.draw(&Text::new(
            "Delivery residuals: task groups and observed distribution",
            (35, 45),
            ("sans-serif", 30)
                .into_font()
                .style(FontStyle::Bold)
                .color(&INK),
        ))?;
        root.draw(&Text::new(
            format!(
                "{} archived competitor observations · manager runtime rounded to 10 ms",
                sorted.len()
            ),
            (35, 80),
            ("sans-serif", 20).into_font().color(&INK),
        ))?;
        let panels = root.margin(115, 100, 25, 25).split_evenly((1, 2));
        let groups: BTreeMap<_, Vec<_>> =
            cases
                .delivery
                .iter()
                .fold(BTreeMap::new(), |mut groups, r| {
                    groups
                        .entry(r.task_type.as_str())
                        .or_default()
                        .push(r.residual_ms);
                    groups
                });
        let labels: Vec<_> = groups
            .iter()
            .map(|(kind, values)| {
                format!(
                    "{} (n={})",
                    match *kind {
                        "matmul_mod" => "Matrix",
                        "monte_carlo_pi" => "Monte Carlo",
                        "prime_count" => "Primes",
                        "sort_checksum" => "Sort",
                        other => other,
                    },
                    values.len()
                )
            })
            .collect();
        let max = sorted[sorted.len() - 1] * 1.2;
        let mut chart = ChartBuilder::on(&panels[0])
            .caption("Every observation, grouped by task", ("sans-serif", 23))
            .margin(20)
            .x_label_area_size(55)
            .y_label_area_size(65)
            .build_cartesian_2d(-0.5..groups.len() as f64 - 0.5, 0.0..max)?;
        chart
            .configure_mesh()
            .x_labels(groups.len())
            .x_label_formatter(&|x| {
                let i = x.round() as isize;
                if i >= 0 && (x - i as f64).abs() < 0.01 {
                    labels.get(i as usize).cloned().unwrap_or_default()
                } else {
                    String::new()
                }
            })
            .y_desc("Residual (ms)")
            .disable_x_mesh()
            .label_style(("sans-serif", 17))
            .axis_desc_style(("sans-serif", 18))
            .light_line_style(WHITE)
            .draw()?;
        for (i, (_, values)) in groups.iter().enumerate() {
            chart.draw_series(values.iter().enumerate().map(|(j, &ms)| {
                Circle::new(
                    (
                        i as f64 + (j as f64 - (values.len() - 1) as f64 / 2.0) * 0.09,
                        ms,
                    ),
                    6,
                    TEAL.filled(),
                )
            }))?;
        }
        let mut chart = ChartBuilder::on(&panels[1])
            .caption("Empirical cumulative distribution", ("sans-serif", 23))
            .margin(20)
            .x_label_area_size(55)
            .y_label_area_size(65)
            .build_cartesian_2d(0.0..max, 0.0..105.0)?;
        chart
            .configure_mesh()
            .x_desc("Residual (ms)")
            .y_desc("Observations at or below (%)")
            .label_style(("sans-serif", 17))
            .axis_desc_style(("sans-serif", 18))
            .light_line_style(WHITE)
            .draw()?;
        let mut points = vec![(0.0, 0.0)];
        for (i, &ms) in sorted.iter().enumerate() {
            points.push((ms, i as f64 / sorted.len() as f64 * 100.0));
            points.push((ms, (i + 1) as f64 / sorted.len() as f64 * 100.0));
        }
        points.push((max, 100.0));
        chart.draw_series(LineSeries::new(points, TEAL.stroke_width(3)))?;
        let upper_label = if percentile(sorted, 95) == percentile(sorted, 99) {
            "p95 = p99"
        } else {
            "p99"
        };
        let mut guides = vec![(50, "p50", BLUE), (99, upper_label, GRAY)];
        if percentile(sorted, 95) != percentile(sorted, 99) {
            guides.push((95, "p95", RGBColor(180, 83, 9)));
        }
        for (percent, label, color) in guides {
            let ms = percentile(sorted, percent);
            chart
                .draw_series(LineSeries::new(
                    [(ms, 0.0), (ms, 100.0)],
                    color.stroke_width(1),
                ))?
                .label(format!("{label}: {ms:.1} ms"))
                .legend(move |(x, y)| {
                    PathElement::new([(x, y), (x + 25, y)], color.stroke_width(2))
                });
        }
        chart
            .configure_series_labels()
            .position(SeriesLabelPosition::UpperLeft)
            .background_style(WHITE.mix(0.95))
            .label_font(("sans-serif", 17))
            .draw()?;
        root.draw(&Text::new("Small samples do not establish per-task differences or reliable p95/p99 tails. All recorded observations are shown.",
            (35,700),("sans-serif",18).into_font().color(&INK)))?;
        root.draw(&Text::new("These are elapsed-time residuals from another contractor, not network RTT measurements for TheGoodGuys.",
            (35,730),("sans-serif",18).into_font().color(&INK)))?;
        root.present()?;
    }
    graphs::rasterize(&path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paired_delays_change_both_costs_without_leaking_into_the_current_bid() {
        let data: Measurements =
            serde_json::from_str(include_str!("../results/bidder.json")).unwrap();
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/practice-history.json");
        let cases = super::super::historical::cases(&data, &path).unwrap();
        let mut delays: Vec<_> = cases.delivery.iter().map(|r| r.residual_ms).collect();
        let original = replay_delays(&data, &cases.tasks, &delays, 1).unwrap();
        let index = 3;
        delays[index] += 30.0;
        let changed = replay_delays(&data, &cases.tasks, &delays, 1).unwrap();
        for (a, b) in original
            .outcomes
            .iter()
            .zip(&changed.outcomes)
            .take(index + 1)
        {
            assert_eq!(a.old_price, b.old_price);
            assert_eq!(a.new_price, b.new_price);
            assert_eq!(a.forecast_delivery_ms, b.forecast_delivery_ms);
        }
        let a = &original.outcomes[index];
        let b = &changed.outcomes[index];
        assert!(a.old_price.is_some() && a.new_price.is_some());
        let extra_cost = 0.030 * cases.tasks[index].market.config.cost_rate;
        assert!((a.old_profit - b.old_profit - extra_cost).abs() < 1e-10);
        assert!((a.new_profit - b.new_profit - extra_cost).abs() < 1e-10);
        assert!((b.delivery_ms - a.delivery_ms - 30.0).abs() < 1e-10);
        assert!(replay_delays(&data, &cases.tasks, &delays[..1], 1).is_err());
        delays[0] = f64::NAN;
        assert!(replay_delays(&data, &cases.tasks, &delays, 1).is_err());
    }
}
