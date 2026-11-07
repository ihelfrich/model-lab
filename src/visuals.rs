//! Shared native scientific plots: explicit axes, clipped marks and linked rows.
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Stroke, Vec2};
pub const INK: Color32 = Color32::from_rgb(224, 232, 242);
pub const MUTED: Color32 = Color32::from_rgb(145, 159, 178);
pub const TEAL: Color32 = Color32::from_rgb(84, 215, 193);
pub const AMBER: Color32 = Color32::from_rgb(242, 181, 108);
pub const BLUE: Color32 = Color32::from_rgb(122, 171, 248);
pub const VIOLET: Color32 = Color32::from_rgb(182, 156, 245);
pub const RED: Color32 = Color32::from_rgb(245, 134, 144);
pub const PALETTE: [Color32; 6] = [
    TEAL,
    BLUE,
    AMBER,
    VIOLET,
    RED,
    Color32::from_rgb(145, 204, 143),
];
pub fn style() -> egui::Style {
    let mut s = egui::Style::default();
    s.visuals = egui::Visuals::dark();
    s.visuals.panel_fill = Color32::from_rgb(15, 21, 30);
    s.visuals.window_fill = Color32::from_rgb(22, 30, 42);
    s.visuals.override_text_color = Some(INK);
    s.visuals.weak_text_color = Some(MUTED);
    s.visuals.selection.bg_fill = Color32::from_rgb(35, 75, 78);
    s.visuals.selection.stroke = Stroke::new(1., TEAL);
    for v in [
        &mut s.visuals.widgets.inactive,
        &mut s.visuals.widgets.hovered,
        &mut s.visuals.widgets.active,
    ] {
        v.corner_radius = egui::CornerRadius::same(6);
        v.fg_stroke = Stroke::new(1., INK);
    }
    s.visuals.widgets.inactive.bg_fill = Color32::from_rgb(29, 39, 53);
    s.visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(29, 39, 53);
    s.visuals.widgets.hovered.bg_fill = Color32::from_rgb(41, 56, 72);
    s.visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(41, 56, 72);
    s.visuals.widgets.active.bg_fill = Color32::from_rgb(42, 84, 84);
    s.visuals.widgets.active.weak_bg_fill = Color32::from_rgb(42, 84, 84);
    s.visuals.widgets.noninteractive.bg_stroke = Stroke::new(1., Color32::from_rgb(47, 61, 78));
    s.spacing.item_spacing = Vec2::new(10., 9.);
    s.spacing.button_padding = Vec2::new(12., 8.);
    s.spacing.slider_width = 135.;
    s.text_styles
        .insert(egui::TextStyle::Body, FontId::proportional(14.));
    s.text_styles
        .insert(egui::TextStyle::Button, FontId::proportional(14.));
    s.text_styles
        .insert(egui::TextStyle::Small, FontId::proportional(12.));
    s
}
#[derive(Clone)]
pub struct Series {
    pub name: String,
    pub points: Vec<(f64, f64)>,
    pub color: Color32,
    pub line: bool,
    pub hollow: bool,
    pub row_ids: Vec<usize>,
}
impl Series {
    pub fn line(name: impl Into<String>, points: Vec<(f64, f64)>, color: Color32) -> Self {
        Self {
            name: name.into(),
            points,
            color,
            line: true,
            hollow: false,
            row_ids: vec![],
        }
    }
    pub fn points(
        name: impl Into<String>,
        points: Vec<(f64, f64)>,
        ids: Vec<usize>,
        color: Color32,
        hollow: bool,
    ) -> Self {
        Self {
            name: name.into(),
            points,
            color,
            line: false,
            hollow,
            row_ids: ids,
        }
    }
}
pub struct Plot<'a> {
    pub id: &'a str,
    pub x_label: &'a str,
    pub y_label: &'a str,
    pub height: f32,
    pub series: &'a [Series],
    pub selected: Option<usize>,
    pub vertical: Option<f64>,
    pub y_range: Option<(f64, f64)>,
}
impl<'a> Plot<'a> {
    pub fn show(self, ui: &mut egui::Ui) -> Option<usize> {
        let width = ui.available_width().max(120.);
        let response = ui
            .push_id(self.id, |ui| {
                ui.allocate_response(Vec2::new(width, self.height), egui::Sense::click_and_drag())
            })
            .inner;
        let frame = response.rect;
        let graph = Rect::from_min_max(
            frame.min + Vec2::new(64., 19.),
            frame.max - Vec2::new(18., 49.),
        );
        let painter = ui.painter_at(frame);
        let mut xmin = f64::INFINITY;
        let mut xmax = f64::NEG_INFINITY;
        let mut ymin = f64::INFINITY;
        let mut ymax = f64::NEG_INFINITY;
        for s in self.series {
            for &(x, y) in &s.points {
                if x.is_finite() && y.is_finite() {
                    xmin = xmin.min(x);
                    xmax = xmax.max(x);
                    ymin = ymin.min(y);
                    ymax = ymax.max(y);
                }
            }
        }
        if !xmin.is_finite() {
            xmin = 0.;
            xmax = 1.;
            ymin = 0.;
            ymax = 1.;
        }
        let dx = if xmax > xmin {
            (xmax - xmin) * 0.04
        } else {
            1.
        };
        let dy = if ymax > ymin {
            (ymax - ymin) * 0.08
        } else {
            1.
        };
        xmin -= dx;
        xmax += dx;
        ymin -= dy;
        ymax += dy;
        if let Some((a, b)) = self.y_range {
            ymin = a;
            ymax = b;
        }
        let map = |x: f64, y: f64| {
            Pos2::new(
                graph.left() + ((x - xmin) / (xmax - xmin)) as f32 * graph.width(),
                graph.bottom() - ((y - ymin) / (ymax - ymin)) as f32 * graph.height(),
            )
        };
        painter.rect_filled(graph, 4., Color32::from_rgb(19, 28, 40));
        painter.rect_stroke(
            graph,
            4.,
            Stroke::new(1., Color32::from_rgb(50, 66, 85)),
            egui::StrokeKind::Inside,
        );
        let ticks = if width < 380. { 3 } else { 4 };
        for k in 0..=ticks {
            let x = xmin + (xmax - xmin) * k as f64 / ticks as f64;
            let y = ymin + (ymax - ymin) * k as f64 / ticks as f64;
            let a = map(x, ymin);
            let b = map(xmin, y);
            painter.line_segment(
                [Pos2::new(graph.left(), b.y), Pos2::new(graph.right(), b.y)],
                Stroke::new(0.6, Color32::from_rgb(37, 49, 66)),
            );
            painter.text(
                a + Vec2::new(0., 8.),
                Align2::CENTER_TOP,
                number(x),
                FontId::proportional(11.),
                MUTED,
            );
            painter.text(
                b - Vec2::new(8., 0.),
                Align2::RIGHT_CENTER,
                number(y),
                FontId::proportional(11.),
                MUTED,
            );
        }
        painter.text(
            Pos2::new(graph.center().x, frame.bottom() - 2.),
            Align2::CENTER_BOTTOM,
            self.x_label,
            FontId::proportional(12.),
            MUTED,
        );
        painter.text(
            frame.min,
            Align2::LEFT_TOP,
            self.y_label,
            FontId::proportional(11.),
            MUTED,
        );
        let clipped = painter.with_clip_rect(graph.shrink(1.));
        if ymin < 0. && ymax > 0. {
            clipped.line_segment(
                [map(xmin, 0.), map(xmax, 0.)],
                Stroke::new(1., Color32::from_rgb(73, 89, 109)),
            );
        }
        if let Some(x) = self.vertical {
            clipped.line_segment(
                [map(x, ymin), map(x, ymax)],
                Stroke::new(1.2, VIOLET.gamma_multiply(0.75)),
            );
        }
        let mut nearest: Option<(f32, usize, String, Pos2)> = None;
        for series in self.series {
            if series.line {
                for pair in series.points.windows(2) {
                    if pair.iter().all(|(x, y)| x.is_finite() && y.is_finite()) {
                        clipped.line_segment(
                            [map(pair[0].0, pair[0].1), map(pair[1].0, pair[1].1)],
                            Stroke::new(2.2, series.color),
                        );
                    }
                }
            } else {
                for (k, &(x, y)) in series.points.iter().enumerate() {
                    if !x.is_finite() || !y.is_finite() {
                        continue;
                    }
                    let point = map(x, y);
                    let row = series.row_ids.get(k).copied();
                    let selected = row.is_some() && row == self.selected;
                    if series.hollow {
                        clipped.circle_stroke(
                            point,
                            if selected { 5.5 } else { 3.0 },
                            Stroke::new(if selected { 2.0 } else { 1.2 }, series.color),
                        );
                    } else {
                        clipped.circle_filled(
                            point,
                            if selected { 5.5 } else { 2.8 },
                            series
                                .color
                                .gamma_multiply(if selected { 1. } else { 0.65 }),
                        );
                    }
                    if selected {
                        clipped.circle_stroke(point, 9., Stroke::new(1.5, AMBER));
                    }
                    if let (Some(pointer), Some(row)) = (response.hover_pos(), row) {
                        let distance = point.distance(pointer);
                        if distance < 15. && nearest.as_ref().is_none_or(|v| distance < v.0) {
                            nearest = Some((
                                distance,
                                row,
                                format!(
                                    "Observation {}\nx = {}\ny = {}",
                                    row + 1,
                                    number(x),
                                    number(y)
                                ),
                                point,
                            ));
                        }
                    }
                }
            }
        }
        if let Some((_, _, ref text, point)) = nearest {
            clipped.circle_stroke(point, 6., Stroke::new(1.3, INK));
            response.clone().on_hover_text(text);
        }
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 16.;
            for s in self.series {
                if !s.name.is_empty() {
                    ui.label(
                        egui::RichText::new(format!(
                            "{} {}",
                            if s.line {
                                "Line:"
                            } else if s.hollow {
                                "Open circles:"
                            } else {
                                "Filled circles:"
                            },
                            s.name
                        ))
                        .size(11.)
                        .color(s.color),
                    );
                }
            }
        });
        if response.clicked() {
            nearest.map(|v| v.1)
        } else {
            None
        }
    }
}
pub fn number(v: f64) -> String {
    if !v.is_finite() {
        "Unavailable".into()
    } else if v.abs() >= 1e5 || (v != 0. && v.abs() < 1e-3) {
        format!("{v:.2e}")
    } else if v.abs() >= 100. {
        format!("{v:.1}")
    } else {
        format!("{v:.3}")
    }
}
pub fn section(ui: &mut egui::Ui, title: &str, subtitle: &str) {
    ui.add_space(6.);
    ui.label(egui::RichText::new(title).size(18.).strong());
    if !subtitle.is_empty() {
        ui.label(egui::RichText::new(subtitle).size(12.).color(MUTED));
    }
    ui.add_space(3.);
}
pub fn panel() -> egui::Frame {
    egui::Frame::new()
        .fill(Color32::from_rgb(22, 30, 42))
        .stroke(Stroke::new(1., Color32::from_rgb(45, 58, 75)))
        .corner_radius(8)
        .inner_margin(14.)
}
pub fn metric(ui: &mut egui::Ui, label: &str, value: &str, color: Color32) {
    ui.vertical(|ui| {
        ui.label(egui::RichText::new(label).size(11.).color(MUTED));
        ui.label(egui::RichText::new(value).size(25.).color(color));
    });
}

pub fn coefficient_bars(ui: &mut egui::Ui, names: &[String], values: &[f64], selected: &mut usize) {
    let max = values.iter().map(|v| v.abs()).fold(0.01, f64::max);
    let width = ui.available_width();
    for (j, (name, &value)) in names.iter().zip(values).enumerate() {
        let (response, p) = ui.allocate_painter(Vec2::new(width, 36.), egui::Sense::click());
        let r = response.rect;
        if *selected == j {
            p.rect_filled(r, 4., Color32::from_rgb(30, 44, 59));
        }
        let zero = r.left() + width * 0.61;
        let half = (width * 0.26).max(20.);
        let end = zero + (value / max) as f32 * half;
        p.text(
            r.min + Vec2::new(8., 18.),
            Align2::LEFT_CENTER,
            name,
            FontId::proportional(13.),
            INK,
        );
        p.line_segment(
            [
                Pos2::new(zero, r.top() + 4.),
                Pos2::new(zero, r.bottom() - 4.),
            ],
            Stroke::new(1., MUTED.gamma_multiply(0.4)),
        );
        let bar = Rect::from_two_pos(
            Pos2::new(zero, r.center().y - 5.),
            Pos2::new(end, r.center().y + 5.),
        );
        p.rect_filled(bar, 2., PALETTE[j % 6]);
        p.text(
            r.right_center() - Vec2::new(5., 0.),
            Align2::RIGHT_CENTER,
            number(value),
            FontId::monospace(12.),
            INK,
        );
        if response.clicked() {
            *selected = j;
        }
    }
}
