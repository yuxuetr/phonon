#![allow(non_snake_case)]

use dioxus::prelude::*;

/// Renders a 64-bar FFT spectrum chart as an SVG.
#[component]
pub fn SpectrumView(bars: Vec<f32>) -> Element {
  let num_bars = bars.len();
  if num_bars == 0 {
    return rsx! { div { class: "text-gray-500 text-center py-8", "No spectrum data" } };
  }

  let svg_w = 1000.0_f64;
  let svg_h = 200.0_f64;
  let bar_gap = 2.0;
  let bar_w = (svg_w - bar_gap * (num_bars as f64 - 1.0)) / num_bars as f64;

  // Build rect elements
  let rects: Vec<String> = bars
        .iter()
        .enumerate()
        .map(|(i, &mag)| {
            let h = (mag as f64 * svg_h).max(1.0);
            let x = i as f64 * (bar_w + bar_gap);
            let y = svg_h - h;
            // Gradient colour: green for low freq → violet for high freq
            let hue = 120.0 + (i as f64 / num_bars as f64) * 180.0; // 120=green → 300=violet
            format!(
                r#"<rect x="{x:.1}" y="{y:.1}" width="{bar_w:.1}" height="{h:.1}" fill="hsl({hue:.0}, 70%, 55%)" rx="1" />"#,
            )
        })
        .collect();

  let inner_svg = rects.join("\n");

  rsx! {
      div {
          dangerous_inner_html: format!(
              r#"<svg width="100%" height="200" viewBox="0 0 {svg_w} {svg_h}" preserveAspectRatio="none">{inner_svg}</svg>"#,
          )
      }
  }
}
