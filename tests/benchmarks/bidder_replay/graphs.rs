use super::Scenario;
use anyhow::{Context, Result};
use plotters::prelude::*;
use std::{fs, path::Path};

pub(super) fn render(
    output: &Path,
    scenarios: &[Scenario],
    filename: &str,
    title: &str,
) -> Result<()> {
    let path = output.join(format!("{filename}.svg"));
    {
        let root = SVGBackend::new(&path, (1680, 650)).into_drawing_area();
        root.fill(&WHITE)?;
        let ink = RGBColor(25, 37, 49);
        let old = RGBColor(100, 116, 139);
        let new = RGBColor(15, 118, 110);
        root.draw(&Text::new(
            title,
            (35, 45),
            ("sans-serif", 32)
                .into_font()
                .style(FontStyle::Bold)
                .color(&ink),
        ))?;
        root.draw(&Text::new("Counterfactual · fixed public competitor bids · measured local p50 plus assumed delivery overhead",(35,80),("sans-serif",20).into_font().color(&ink)))?;
        let panels = root.margin(115, 80, 25, 25).split_evenly((1, 3));
        for (panel, scenario) in panels.iter().zip(scenarios) {
            let max = scenario
                .outcomes
                .iter()
                .flat_map(|o| [o.old_cumulative, o.new_cumulative])
                .fold(1.0, f64::max)
                * 1.12;
            let min = scenario
                .outcomes
                .iter()
                .flat_map(|o| [o.old_cumulative, o.new_cumulative])
                .fold(0.0, f64::min)
                * 1.12;
            let mut chart = ChartBuilder::on(panel)
                .caption(
                    format!("{:.0} ms delivery overhead", scenario.network_ms),
                    ("sans-serif", 23),
                )
                .margin(20)
                .x_label_area_size(45)
                .y_label_area_size(68)
                .build_cartesian_2d(0.0..scenario.outcomes.len() as f64, min..max)?;
            chart
                .configure_mesh()
                .x_desc("Auction in replay")
                .y_desc("Cumulative modeled profit")
                .label_style(("sans-serif", 16))
                .axis_desc_style(("sans-serif", 16))
                .light_line_style(WHITE)
                .bold_line_style(RGBColor(229, 233, 238))
                .draw()?;
            chart
                .draw_series(LineSeries::new(
                    std::iter::once((0.0, 0.0)).chain(
                        scenario
                            .outcomes
                            .iter()
                            .map(|o| (o.auction as f64, o.old_cumulative)),
                    ),
                    old.stroke_width(2),
                ))?
                .label("Previous bidder")
                .legend(move |(x, y)| PathElement::new([(x, y), (x + 25, y)], old.stroke_width(2)));
            chart
                .draw_series(LineSeries::new(
                    std::iter::once((0.0, 0.0)).chain(
                        scenario
                            .outcomes
                            .iter()
                            .map(|o| (o.auction as f64, o.new_cumulative)),
                    ),
                    new.stroke_width(3),
                ))?
                .label("Adaptive bidder")
                .legend(move |(x, y)| PathElement::new([(x, y), (x + 25, y)], new.stroke_width(3)));
            chart
                .configure_series_labels()
                .position(SeriesLabelPosition::UpperLeft)
                .background_style(WHITE.mix(0.9))
                .label_font(("sans-serif", 17))
                .draw()?;
        }
        root.draw(&Text::new("Learning begins empty. Both policies face the same recorded bids; this is not a claim of live winnings.",(35,617),("sans-serif",18).into_font().color(&ink)))?;
        root.present()?;
    }
    let mut options = resvg::usvg::Options::default();
    options.fontdb_mut().load_system_fonts();
    let tree = resvg::usvg::Tree::from_data(&fs::read(&path)?, &options)?;
    let size = tree.size().to_int_size();
    let mut pixmap = resvg::tiny_skia::Pixmap::new(size.width(), size.height())
        .context("allocating replay image")?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::default(),
        &mut pixmap.as_mut(),
    );
    pixmap.save_png(path.with_extension("png"))?;
    Ok(())
}
