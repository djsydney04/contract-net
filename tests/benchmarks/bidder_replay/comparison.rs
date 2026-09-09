//! Pin the actual pre-optimization policy and make its cost coverage visible.
use super::{Scenario, graphs};
use anyhow::{Result, ensure};
use plotters::prelude::*;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::Path;

pub(super) const BEFORE_LABEL: &str = "Before: 71ed371";
pub(super) const AFTER_LABEL: &str = "After: optimized";
const BEFORE_COMMIT: &str = "71ed37123263ce0dc859cbf3b870e3087843a0d5";
const BEFORE_SHA256: &str = "39aa0b427d0bdbecdbde94bc73236d0b135759ec34fe454918e43617249e95da";
const ARCHIVE: &str = include_str!("../../fixtures/bidder-before-optimization.rs");

pub(super) const DESCRIPTION: &str = "
## Before and after bidder optimization

**Before** executes the actual Rust `MyContractor::on_cfp` from
[`71ed371`](https://github.com/djsydney04/contract-net/blob/71ed37123263ce0dc859cbf3b870e3087843a0d5/src/strategy.rs),
the revision immediately before the bidder optimizations. Its complete strategy
source is frozen verbatim in
[`bidder-before-optimization.rs`](../../tests/fixtures/bidder-before-optimization.rs),
compiled into the replay tool, and checked against its pinned SHA-256 on every
run. It quotes `compute_seconds * cost_rate * 1.6`, includes queue time in its
completion estimate, and makes no allowance for delivery overhead.

**After** executes the current optimized Rust bidder with settlement learning,
delivery and compute reserves, and competition-aware pricing. Both receive
identical saved calibration, workloads, competitor bids, and per-auction delays.
This isolates bidding policy; it is not a comparison of the entire historical
application, old calibration noise, or Python versus Rust execution speed.
Old/new report columns mean these before/after policies throughout.

![Before and after bid prices, with the old model's cost coverage shown in detail](bid-price-comparison.svg)

The left panel compares both policies on the same price scale. The right panel
uses a separate, explicitly labeled detail scale to show the old bid against
the modeled cost of executing that auction. A bid below the cost line loses
money if awarded. Points represent submitted quotes; a refusal has no point.
Costs remain counterfactual and use the archived delivery residual as a proxy,
as described below. Policy revision metadata accompanies both replay JSON files.
";

pub(super) fn verify_archive() -> Result<()> {
    ensure!(
        format!("{:x}", Sha256::digest(ARCHIVE.as_bytes())) == BEFORE_SHA256,
        "pre-optimization source no longer matches the pinned historical revision"
    );
    Ok(())
}

pub(super) fn provenance() -> Value {
    json!({
        "before": {
            "commit": BEFORE_COMMIT,
            "original_path": "src/strategy.rs",
            "fixture": "tests/fixtures/bidder-before-optimization.rs",
            "source_sha256": BEFORE_SHA256,
            "execution": "Archived MyContractor::on_cfp, compiled verbatim with a context adapter"
        },
        "after": {
            "implementation": "src/strategy.rs",
            "strategy_source_sha256": format!("{:x}", Sha256::digest(include_bytes!("../../../src/strategy.rs"))),
            "learning_source_sha256": format!("{:x}", Sha256::digest(include_bytes!("../../../src/bidder.rs")))
        },
        "controls": "Same saved calibration, task compute samples, queue, competitors and realized delivery delays for both policies; compares policy behavior, not historical process performance."
    })
}

pub(super) fn render(output: &Path, scenario: &Scenario) -> Result<()> {
    let path = output.join("bid-price-comparison.svg");
    {
        let root = SVGBackend::new(&path, (1680, 760)).into_drawing_area();
        root.fill(&WHITE)?;
        let ink = RGBColor(25, 37, 49);
        let before = RGBColor(100, 116, 139);
        let after = RGBColor(15, 118, 110);
        let cost = RGBColor(185, 28, 28);
        root.draw(&Text::new(
            "Before and after optimization: bid price versus cost",
            (35, 45),
            ("sans-serif", 32)
                .into_font()
                .style(FontStyle::Bold)
                .color(&ink),
        ))?;
        root.draw(&Text::new("Before: archived Rust policy 71ed371 · After: optimized bidder · identical tasks, competition and per-auction delays",(35,80),
            ("sans-serif",20).into_font().color(&ink)))?;
        let panels = root.margin(115, 100, 25, 25).split_evenly((1, 2));
        for (detail, panel) in panels.iter().enumerate() {
            let max = scenario
                .outcomes
                .iter()
                .flat_map(|o| {
                    [
                        o.old_price.unwrap_or(0.0),
                        if detail == 0 {
                            o.new_price.unwrap_or(0.0)
                        } else {
                            0.0
                        },
                        o.cost_if_awarded,
                    ]
                })
                .fold(0.001, f64::max)
                * 1.15;
            let mut chart = ChartBuilder::on(panel)
                .caption(
                    if detail == 0 {
                        "Both policies (common price scale)"
                    } else {
                        "Before: cost coverage (detail scale)"
                    },
                    ("sans-serif", 23),
                )
                .margin(20)
                .x_label_area_size(60)
                .y_label_area_size(70)
                .build_cartesian_2d(0.5..scenario.outcomes.len() as f64 + 0.5, 0.0..max)?;
            chart
                .configure_mesh()
                .x_desc("Historical auction in chronological order (task ID)")
                .y_desc("Price / modeled cost ($)")
                .x_labels(scenario.outcomes.len())
                .x_label_formatter(&|x| {
                    let i = x.round() as usize;
                    if i > 0 && i <= scenario.outcomes.len() && (x - i as f64).abs() < 0.01 {
                        format!("#{}", scenario.outcomes[i - 1].task_id)
                    } else {
                        String::new()
                    }
                })
                .label_style(("sans-serif", 16))
                .axis_desc_style(("sans-serif", 16))
                .light_line_style(WHITE)
                .bold_line_style(RGBColor(229, 233, 238))
                .draw()?;
            for (series, label, color) in [
                (0, BEFORE_LABEL, before),
                (1, AFTER_LABEL, after),
                (2, "Modeled cost if awarded", cost),
            ] {
                if detail == 1 && series == 1 {
                    continue;
                }
                let points: Vec<_> = scenario
                    .outcomes
                    .iter()
                    .map(|o| {
                        let price = match series {
                            0 => o.old_price,
                            1 => o.new_price,
                            _ => Some(o.cost_if_awarded),
                        };
                        price.map(|p| (o.auction as f64, p))
                    })
                    .collect();
                // Break the line at refused auctions rather than implying a quote.
                chart
                    .draw_series(points.windows(2).filter_map(|w| {
                        Some(PathElement::new([w[0]?, w[1]?], color.stroke_width(3)))
                    }))?
                    .label(label)
                    .legend(move |(x, y)| {
                        PathElement::new([(x, y), (x + 25, y)], color.stroke_width(3))
                    });
                chart.draw_series(
                    points
                        .iter()
                        .flatten()
                        .map(|&p| Circle::new(p, 4, color.filled())),
                )?;
            }
            chart
                .configure_series_labels()
                .position(SeriesLabelPosition::UpperRight)
                .background_style(WHITE.mix(0.95))
                .label_font(("sans-serif", 17))
                .draw()?;
        }
        root.draw(&Text::new("The archived bidder prices local compute only. Below the red cost line, a correct completed contract still loses money.",
            (35,700),("sans-serif",18).into_font().color(&ink)))?;
        root.draw(&Text::new("Counterfactual replay; another contractor's archived residuals are used as delivery assumptions. Competition is held fixed.",
            (35,730),("sans-serif",18).into_font().color(&ink)))?;
        root.present()?;
    }
    graphs::rasterize(&path)
}

#[cfg(test)]
mod tests {
    #[test]
    fn baseline_is_the_unmodified_archived_strategy_source() {
        super::verify_archive().unwrap();
    }
}
