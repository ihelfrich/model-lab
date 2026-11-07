//! Guided view: actual observations, an interpolated fitted slice, and residual tethers.
use crate::{animation, model::Fit};
use eframe::egui::{self, Color32, Pos2, Rect, Stroke, Vec2};
const MINT: Color32 = Color32::from_rgb(86, 238, 197);
const VIOLET: Color32 = Color32::from_rgb(168, 146, 255);
const WARM: Color32 = Color32::from_rgb(255, 187, 105);
const INK: Color32 = Color32::from_rgb(16, 24, 43);
#[derive(Default)]
pub struct SliceCache {
    revision: u64,
    feature: usize,
    pub grid: Vec<f64>,
    pub path: Vec<Vec<f64>>,
}
impl SliceCache {
    pub fn prepare(&mut self, fit: &Fit, feature: usize, revision: u64) {
        if self.revision == revision && self.feature == feature && !self.path.is_empty() {
            return;
        }
        self.revision = revision;
        self.feature = feature;
        let lo = fit
            .data
            .x
            .iter()
            .map(|x| x[feature])
            .fold(f64::INFINITY, f64::min);
        let hi = fit
            .data
            .x
            .iter()
            .map(|x| x[feature])
            .fold(f64::NEG_INFINITY, f64::max);
        self.grid = (0..320)
            .map(|i| lo + (hi - lo) * i as f64 / 319.0)
            .collect();
        let mut center = vec![0.0; fit.data.names.len()];
        for (k, v) in center.iter_mut().enumerate() {
            let mut a: Vec<_> = fit.train.iter().map(|&i| fit.data.x[i][k]).collect();
            a.sort_by(f64::total_cmp);
            *v = a[a.len() / 2];
        }
        let x: Vec<_> = self
            .grid
            .iter()
            .map(|&v| {
                let mut row = center.clone();
                row[feature] = v;
                row
            })
            .collect();
        self.path = vec![vec![fit.baseline; 320]];
        let mut sums = vec![0.0; 320];
        for (b, tree) in fit.trees.iter().enumerate() {
            let mut next = self.path.last().unwrap().clone();
            for i in 0..320 {
                let h = tree.predict(&x[i]);
                next[i] = match fit.cfg.method {
                    crate::model::Method::Tree => h,
                    crate::model::Method::Forest => {
                        sums[i] += h;
                        sums[i] / (b + 1) as f64
                    }
                    _ => next[i] + fit.cfg.rate * h,
                };
            }
            self.path.push(next);
        }
    }
}

pub fn transition_phase(phase: f64) -> f64 {
    ((phase - 2.0 / 3.0) / (1.0 / 3.0)).clamp(0.0, 1.0)
}
pub fn chart(
    ui: &mut egui::Ui,
    fit: &Fit,
    stage: usize,
    feature: usize,
    selected: usize,
    phase: f64,
    motion: bool,
    cache: &mut SliceCache,
    revision: u64,
    from_stage: usize,
) -> Option<usize> {
    let width = ui.available_width().max(320.0);
    let height = (width * 0.34).clamp(280.0, 390.0);
    let (response, p) = ui.allocate_painter(Vec2::new(width, height), egui::Sense::click());
    let outer = response.rect;
    p.rect_filled(outer, 18.0, INK);
    p.rect_stroke(
        outer.shrink(0.5),
        18.0,
        Stroke::new(1.0, Color32::from_rgb(63, 77, 102)),
        egui::StrokeKind::Inside,
    );
    let coating = outer.shrink(13.0);
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(
        coating.left_top(),
        Color32::from_rgba_unmultiplied(154, 175, 236, 13),
    );
    mesh.colored_vertex(
        coating.right_top(),
        Color32::from_rgba_unmultiplied(158, 202, 238, 5),
    );
    mesh.colored_vertex(coating.right_bottom(), Color32::TRANSPARENT);
    mesh.colored_vertex(coating.left_bottom(), Color32::TRANSPARENT);
    mesh.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
    p.add(egui::Shape::mesh(mesh));
    let graph = Rect::from_min_max(
        outer.min + Vec2::new(68.0, 42.0),
        outer.max - Vec2::new(27.0, 45.0),
    );
    let faint = Color32::from_rgb(44, 57, 80);
    let text = Color32::from_rgb(174, 189, 210);
    let feature = feature.min(fit.data.names.len() - 1);
    let xs: Vec<_> = fit.data.x.iter().map(|x| x[feature]).collect();
    let xlo = xs.iter().copied().fold(f64::INFINITY, f64::min);
    let xhi = xs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let xpad = if xhi > xlo { (xhi - xlo) * 0.025 } else { 1.0 };
    cache.prepare(fit, feature, revision);
    let previous = from_stage.min(fit.trees.len());
    let curves: Vec<_> = (0..320)
        .map(|i| (cache.grid[i], cache.path[previous][i], cache.path[stage][i]))
        .collect();
    let mut ylo = fit.data.y.iter().copied().fold(f64::INFINITY, f64::min);
    let mut yhi = fit.data.y.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    for &(_, a, b) in &curves {
        ylo = ylo.min(a.min(b));
        yhi = yhi.max(a.max(b));
    }
    let old = fit.stage_predictions[previous][selected];
    let new = fit.stage_predictions[stage][selected];
    ylo = ylo.min(old.min(new));
    yhi = yhi.max(old.max(new));
    let ypad = if yhi > ylo { (yhi - ylo) * 0.12 } else { 1.0 };
    ylo -= ypad;
    yhi += ypad;
    let map = |x: f64, y: f64| {
        Pos2::new(
            graph.left() + ((x - xlo + xpad) / (xhi - xlo + 2.0 * xpad)) as f32 * graph.width(),
            graph.bottom() - ((y - ylo) / (yhi - ylo)) as f32 * graph.height(),
        )
    };
    for k in 0..=4 {
        let x = xlo + (xhi - xlo) * k as f64 / 4.0;
        let y = ylo + (yhi - ylo) * k as f64 / 4.0;
        let xp = map(x, ylo).x;
        let yp = map(xlo, y).y;
        p.line_segment(
            [Pos2::new(xp, graph.top()), Pos2::new(xp, graph.bottom())],
            Stroke::new(0.6, faint),
        );
        p.line_segment(
            [Pos2::new(graph.left(), yp), Pos2::new(graph.right(), yp)],
            Stroke::new(0.6, faint),
        );
        p.text(
            Pos2::new(xp, graph.bottom() + 9.0),
            egui::Align2::CENTER_TOP,
            number(x),
            egui::FontId::proportional(11.0),
            text,
        );
        p.text(
            Pos2::new(graph.left() - 10.0, yp),
            egui::Align2::RIGHT_CENTER,
            number(y),
            egui::FontId::proportional(11.0),
            text,
        );
    }
    p.text(
        outer.min + Vec2::new(20.0, 15.0),
        egui::Align2::LEFT_TOP,
        &fit.data.target,
        egui::FontId::proportional(13.0),
        text,
    );
    p.text(
        Pos2::new(graph.center().x, outer.bottom() - 7.0),
        egui::Align2::CENTER_BOTTOM,
        &fit.data.names[feature],
        egui::FontId::proportional(13.0),
        text,
    );
    let pp = p.with_clip_rect(graph);
    let at = transition_phase(phase);
    let eased = animation::ease(at);
    let sample = stage.checked_sub(1).map(|b| &fit.samples[b]);
    // A bounded scatter layer; followed observation remains visible even if downsampled.
    let stride = (fit.data.y.len() / 600).max(1);
    let mut valid_mask = vec![false; fit.data.y.len()];
    for &i in &fit.valid {
        valid_mask[i] = true;
    }
    let mut sampled_mask = vec![false; fit.data.y.len()];
    if let Some(s) = sample {
        for &i in s {
            sampled_mask[i] = true;
        }
    }
    for i in (0..fit.data.y.len()).step_by(stride) {
        let point = map(xs[i], fit.data.y[i]);
        let color = if valid_mask[i] {
            WARM
        } else {
            Color32::from_rgb(153, 171, 199)
        };
        pp.circle_filled(point, 2.5, color.gamma_multiply(0.60));
        if motion && phase < 0.28 && sampled_mask[i] {
            let radius = 3.0 + 6.0 * (phase / 0.28) as f32;
            pp.circle_stroke(
                point,
                radius,
                Stroke::new(1.0, MINT.gamma_multiply((1.0 - phase / 0.28) as f32 * 0.7)),
            );
        }
    }
    // Old fit remains a faint reference; the bright fit moves only between computed endpoints.
    for pair in curves.windows(2) {
        pp.line_segment(
            [map(pair[0].0, pair[0].1), map(pair[1].0, pair[1].1)],
            Stroke::new(1.2, VIOLET.gamma_multiply(0.45)),
        );
        let a = map(pair[0].0, pair[0].1 + (pair[0].2 - pair[0].1) * eased);
        let b = map(pair[1].0, pair[1].1 + (pair[1].2 - pair[1].1) * eased);
        pp.line_segment([a, b], Stroke::new(12.0, MINT.gamma_multiply(0.035)));
        pp.line_segment([a, b], Stroke::new(6.0, MINT.gamma_multiply(0.08)));
        pp.line_segment([a, b], Stroke::new(2.5, MINT));
    }
    // A luminous tracer sweeps the fitted slice during the correction phase.
    if motion && at > 0.0 && at < 1.0 {
        let idx = ((curves.len() - 1) as f64 * at) as usize;
        let c = curves[idx];
        let point = map(c.0, c.1 + (c.2 - c.1) * eased);
        for (rad, alpha) in [(18.0, 0.035), (10.0, 0.09), (5.0, 0.4)] {
            pp.circle_filled(point, rad, MINT.gamma_multiply(alpha));
        }
        pp.circle_filled(point, 3.5, Color32::WHITE);
    }
    let actual = map(xs[selected], fit.data.y[selected]);
    let before = map(xs[selected], old);
    let moving = map(xs[selected], animation::interpolate(old, new, at));
    pp.line_segment(
        [actual, moving],
        Stroke::new(2.0, WARM.gamma_multiply(0.85)),
    );
    pp.circle_stroke(before, 5.0, Stroke::new(1.2, VIOLET));
    pp.circle_filled(moving, 12.0, MINT.gamma_multiply(0.10));
    pp.circle_filled(moving, 6.0, MINT);
    pp.circle_filled(actual, 4.5, WARM);
    pp.circle_stroke(actual, 9.0, Stroke::new(1.5, WARM.gamma_multiply(0.5)));
    // Labels are anchored inward and separated vertically when values are close.
    let right = actual.x < graph.center().x;
    let dx = if right { 15.0 } else { -15.0 };
    let anchor = if right {
        egui::Align2::LEFT_CENTER
    } else {
        egui::Align2::RIGHT_CENTER
    };
    let actual_y = actual.y.clamp(graph.top() + 12.0, graph.bottom() - 12.0);
    let mut predicted_y = moving.y;
    if (actual_y - predicted_y).abs() < 24.0 {
        predicted_y += if predicted_y > graph.center().y {
            -26.0
        } else {
            26.0
        };
    }
    predicted_y = predicted_y.clamp(graph.top() + 12.0, graph.bottom() - 12.0);
    pp.text(
        Pos2::new(actual.x + dx, actual_y),
        anchor,
        format!("actual {}", number(fit.data.y[selected])),
        egui::FontId::proportional(13.0),
        WARM,
    );
    pp.text(
        Pos2::new(moving.x + dx, predicted_y),
        anchor,
        format!(
            "prediction {}",
            number(animation::interpolate(old, new, at))
        ),
        egui::FontId::proportional(13.0),
        MINT,
    );
    let mut picked = None;
    if let Some(cursor) = response.hover_pos() {
        if graph.contains(cursor) {
            if let Some((i, d)) = fit
                .data
                .y
                .iter()
                .enumerate()
                .map(|(i, &v)| (i, map(xs[i], v).distance(cursor)))
                .min_by(|a, b| a.1.total_cmp(&b.1))
            {
                if d < 18.0 {
                    pp.circle_stroke(
                        map(xs[i], fit.data.y[i]),
                        8.0,
                        Stroke::new(1.2, Color32::WHITE),
                    );
                    response.clone().on_hover_text(format!(
                        "Row {} · actual {:.4}\nClick to follow this observation",
                        i + 1,
                        fit.data.y[i]
                    ));
                    if response.clicked() {
                        picked = Some(i);
                    }
                }
            }
        }
    }
    picked
}
fn number(v: f64) -> String {
    if v.abs() >= 10000.0 {
        format!("{:.1}k", v / 1000.0)
    } else {
        format!("{v:.2}")
    }
}

pub fn tree_counts(node: &crate::model::Node) -> (usize, usize) {
    if let Some((_, _, l, r)) = &node.split {
        let (a, b) = tree_counts(l);
        let (c, d) = tree_counts(r);
        (1 + a + c, b + d)
    } else {
        (0, 1)
    }
}
pub fn error_path(ui: &mut egui::Ui, fit: &Fit, stage: usize) {
    ui.strong("Does the improvement carry over?");
    let training = fit.train_mse[stage];
    let validation = fit.valid_mse[stage];
    ui.horizontal_wrapped(|ui| {
        ui.label(format!("Training {training:.3}"));
        ui.colored_label(
            Color32::from_rgb(183, 102, 37),
            format!("Validation {validation:.3}"),
        );
    });
    let (response, p) =
        ui.allocate_painter(Vec2::new(ui.available_width(), 122.0), egui::Sense::hover());
    let r = Rect::from_min_max(
        response.rect.min + Vec2::new(12.0, 8.0),
        response.rect.max - Vec2::new(12.0, 24.0),
    );
    let ymax = fit
        .train_mse
        .iter()
        .chain(&fit.valid_mse)
        .copied()
        .fold(0.0, f64::max)
        .max(1e-12)
        * 1.08;
    let map = |k: usize, v: f64| {
        Pos2::new(
            r.left() + k as f32 / fit.trees.len().max(1) as f32 * r.width(),
            r.bottom() - (v / ymax) as f32 * r.height(),
        )
    };
    p.line_segment(
        [r.left_bottom(), r.right_bottom()],
        Stroke::new(1.0, Color32::LIGHT_GRAY),
    );
    for (values, color) in [
        (&fit.train_mse, Color32::from_rgb(53, 112, 188)),
        (&fit.valid_mse, Color32::from_rgb(199, 119, 48)),
    ] {
        for k in 1..values.len() {
            p.line_segment(
                [map(k - 1, values[k - 1]), map(k, values[k])],
                Stroke::new(
                    1.8,
                    color.gamma_multiply(if k <= stage { 1.0 } else { 0.22 }),
                ),
            );
        }
        p.circle_filled(map(stage, values[stage]), 4.0, color);
    }
    let x = map(stage, 0.0).x;
    p.line_segment(
        [Pos2::new(x, r.top()), Pos2::new(x, r.bottom())],
        Stroke::new(1.0, Color32::from_gray(150)),
    );
    p.text(
        Pos2::new(r.center().x, response.rect.bottom()),
        egui::Align2::CENTER_BOTTOM,
        "Number of trees  ·  vertical axis: MSE",
        egui::FontId::proportional(11.0),
        Color32::GRAY,
    );
    if stage > 0 {
        let change = fit.valid_mse[stage] - fit.valid_mse[stage - 1];
        ui.small(if change < 0.0 {
            "Validation error fell in this step."
        } else if change > 0.0 {
            "Validation error rose in this step. The next tree need not help unseen data."
        } else {
            "Validation error is unchanged in this step."
        });
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn correction_animation_has_measurement_phase_and_exact_endpoint() {
        assert_eq!(super::transition_phase(0.2), 0.0);
        assert_eq!(super::transition_phase(0.5), 0.0);
        assert_eq!(super::transition_phase(1.0), 1.0);
        assert!(super::transition_phase(0.8) > 0.0);
    }
    #[test]
    fn cached_slice_matches_direct_predictions_for_every_method() {
        for method in crate::model::Method::ALL {
            let data = crate::data::Table::demo(0).dataset(2, &[0]).unwrap();
            let f = crate::model::fit(
                data,
                crate::model::Config {
                    method,
                    trees: 5,
                    ..Default::default()
                },
                std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            )
            .unwrap();
            let mut c = super::SliceCache::default();
            c.prepare(&f, 0, 1);
            for b in 0..=f.trees.len() {
                for i in [0, 100, 319] {
                    assert!((c.path[b][i] - f.predict(&[c.grid[i]], b)).abs() < 1e-10);
                }
            }
        }
    }
}
