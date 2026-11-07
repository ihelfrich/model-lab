//! Original native instrument UI: layered controls, optical surfaces, real stage keys.
use eframe::egui::{self, Color32, Pos2, Stroke, Vec2};
pub const BASE: Color32 = Color32::from_rgb(19, 22, 29);
pub const PANEL: Color32 = Color32::from_rgb(26, 31, 40);
pub const SURFACE: Color32 = Color32::from_rgb(35, 42, 54);
pub const TEXT: Color32 = Color32::from_rgb(227, 234, 244);
pub const MUTED: Color32 = Color32::from_rgb(151, 163, 184);
pub const ACCENT: Color32 = Color32::from_rgb(145, 181, 255);

pub fn glass() -> egui::Frame {
    egui::Frame::new()
        .fill(Color32::from_rgba_unmultiplied(38, 46, 61, 224))
        .stroke(Stroke::new(1.0, Color32::from_rgb(66, 78, 101)))
        .corner_radius(14)
        .inner_margin(12.0)
        .shadow(egui::epaint::Shadow {
            offset: [0, 5],
            blur: 16,
            spread: 0,
            color: Color32::from_black_alpha(35),
        })
}
pub fn style() -> egui::Style {
    let mut s = egui::Style::default();
    s.visuals = egui::Visuals::dark();
    s.visuals.panel_fill = BASE;
    s.visuals.window_fill = PANEL;
    s.visuals.override_text_color = Some(TEXT);
    s.visuals.weak_text_color = Some(MUTED);
    s.visuals.selection.bg_fill = Color32::from_rgb(57, 78, 117);
    s.visuals.selection.stroke = Stroke::new(1.0, ACCENT);
    s.visuals.widgets.inactive.bg_fill = SURFACE;
    s.visuals.widgets.inactive.weak_bg_fill = SURFACE;
    s.visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(61, 72, 91));
    s.visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT);
    s.visuals.widgets.hovered.bg_fill = Color32::from_rgb(51, 63, 84);
    s.visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(51, 63, 84);
    s.visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    s.visuals.widgets.active.bg_fill = Color32::from_rgb(65, 87, 124);
    s.visuals.widgets.active.weak_bg_fill = Color32::from_rgb(65, 87, 124);
    s.visuals.widgets.active.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    s.visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(57, 67, 85));
    s.visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
    for v in [
        &mut s.visuals.widgets.inactive,
        &mut s.visuals.widgets.hovered,
        &mut s.visuals.widgets.active,
    ] {
        v.corner_radius = egui::CornerRadius::same(7);
    }
    s.spacing.item_spacing = Vec2::new(10.0, 8.0);
    s.spacing.button_padding = Vec2::new(12.0, 9.0);
    s.text_styles
        .insert(egui::TextStyle::Body, egui::FontId::proportional(14.0));
    s.text_styles
        .insert(egui::TextStyle::Button, egui::FontId::proportional(14.0));
    s
}
pub fn mode_key(ui: &mut egui::Ui, index: &str, label: &str, selected: bool) -> egui::Response {
    let response = ui.add_sized(
        [76.0, 60.0],
        egui::Button::new(
            egui::RichText::new(format!("{index}\n{label}"))
                .monospace()
                .size(11.0)
                .color(if selected {
                    Color32::from_rgb(17, 29, 47)
                } else {
                    MUTED
                }),
        )
        .fill(if selected { ACCENT } else { PANEL }),
    );
    if selected {
        let r = response.rect;
        ui.painter().line_segment(
            [
                r.left_bottom() + Vec2::new(12.0, -4.0),
                r.right_bottom() + Vec2::new(-12.0, -4.0),
            ],
            Stroke::new(2.0, Color32::WHITE.gamma_multiply(0.5)),
        );
    }
    response
}
pub fn dial(
    ui: &mut egui::Ui,
    value: &mut f64,
    range: std::ops::RangeInclusive<f64>,
    label: &str,
    log: bool,
    integer: bool,
    color: Color32,
) -> bool {
    let (before, min, max) = (*value, *range.start(), *range.end());
    let (response, p) = ui.allocate_painter(Vec2::new(108.0, 102.0), egui::Sense::click_and_drag());
    if response.drag_started() {
        ui.data_mut(|d| d.insert_temp(response.id, before));
    }
    if response.dragged() {
        let start = ui
            .data(|d| d.get_temp::<f64>(response.id))
            .unwrap_or(before);
        let delta = response.drag_delta();
        let movement = (-delta.y + delta.x * 0.35) as f64 / 160.0;
        *value = map_dial(start, movement, min, max, log, integer);
    }
    let center = response.rect.min + Vec2::new(54.0, 43.0);
    let radius = 27.0;
    p.circle_filled(
        center + Vec2::new(0.0, 3.0),
        radius + 2.0,
        Color32::from_black_alpha(55),
    );
    p.circle_filled(center, radius, Color32::from_rgb(36, 43, 56));
    p.circle_stroke(
        center,
        radius,
        Stroke::new(1.0, Color32::from_rgb(83, 96, 119)),
    );
    let ratio = if log {
        (value.ln() - min.ln()) / (max.ln() - min.ln())
    } else {
        (*value - min) / (max - min)
    };
    let angle = 2.35 + ratio * 4.72;
    let vector = Vec2::new(angle.cos() as f32, angle.sin() as f32);
    for k in 0..24 {
        let a = 2.35 + k as f32 / 23.0 * 4.72;
        let v = Vec2::new(a.cos(), a.sin());
        p.line_segment(
            [center + v * 32.0, center + v * 35.0],
            Stroke::new(
                1.6,
                if k as f64 / 23.0 <= ratio {
                    color
                } else {
                    Color32::from_rgb(62, 72, 90)
                },
            ),
        );
    }
    p.line_segment(
        [center + vector * 16.0, center + vector * 24.0],
        Stroke::new(2.5, color),
    );
    p.text(
        center,
        egui::Align2::CENTER_CENTER,
        if integer {
            format!("{value:.0}")
        } else {
            format!("{value:.2}")
        },
        egui::FontId::monospace(14.0),
        TEXT,
    );
    p.text(
        Pos2::new(center.x, response.rect.bottom() - 10.0),
        egui::Align2::CENTER_BOTTOM,
        label,
        egui::FontId::monospace(11.0),
        MUTED,
    );
    response.on_hover_text("Drag up or right to increase; down or left to decrease. Precise entry is available beside the dial.");
    *value != before
}
fn map_dial(start: f64, movement: f64, min: f64, max: f64, log: bool, integer: bool) -> f64 {
    let value = if log {
        (start.ln() + movement * (max.ln() - min.ln()))
            .exp()
            .clamp(min, max)
    } else {
        (start + movement * (max - min)).clamp(min, max)
    };
    if integer {
        value.round()
    } else {
        value
    }
}
pub fn step_keys(ui: &mut egui::Ui, fit: &crate::model::Fit, stage: usize) -> Option<usize> {
    let n = fit.trees.len();
    let count = n.min(16).max(1);
    let mut selected = None;
    ui.label(
        egui::RichText::new("TREE SEQUENCE  /  click a key to jump")
            .monospace()
            .size(10.0)
            .color(MUTED),
    );
    let width = ((ui.available_width() - 5.0 * (count - 1) as f32) / count as f32).max(20.0);
    ui.horizontal(|ui|{ui.spacing_mut().item_spacing.x=5.0;
        for k in 0..count{let b=((k+1)*n+count-1)/count;let prior=k*n/count;let active=stage>prior&&stage<=b;
            let color=if active{ACCENT}else if b==fit.best_stage(){Color32::from_rgb(86,156,131)}else{SURFACE};
            let response=ui.add_sized([width,34.0],egui::Button::new(egui::RichText::new(format!("{b:02}")).monospace().size(11.0).color(if active{BASE}else{TEXT})).fill(color));
            if response.clicked(){selected=Some(b);}response.on_hover_text(format!("Tree {b}\nValidation MSE {:.5}\nGreen key marks the best validation stage when represented.",fit.valid_mse[b]));
        }
    });
    selected
}
#[cfg(test)]
mod tests {
    #[test]
    fn encoder_accumulates_small_drag_and_respects_domains() {
        assert_eq!(
            super::map_dial(1.0, 12.0 / 160.0, 1.0, 8.0, false, true),
            2.0
        );
        assert_eq!(super::map_dial(2.0, -100.0, 1.0, 8.0, false, true), 1.0);
        assert_eq!(super::map_dial(0.1, 100.0, 0.01, 1.0, true, false), 1.0);
        assert!(super::map_dial(0.1, 0.1, 0.01, 1.0, true, false) > 0.1);
    }
}
