//! Scenario accounting view (egui). Numerics live in model_lab_engine::decision.
use crate::visuals as v;
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Stroke, Vec2};
pub use model_lab_engine::decision::*;
pub struct Decision {
    evidence: Option<Evidence>,
    path: String,
    error: String,
    selected: usize,
    demand: f64,
    price: f64,
    production: f64,
    cost: f64,
    carrying: f64,
    elasticity: f64,
    demand_scale: f64,
    playing: bool,
    phase: f64,
    validation_round: u32,
    stress_sales_fraction: f64,
    collection_delay: f64,
}
impl Default for Decision {
    fn default() -> Self {
        Self {
            evidence: None,
            path: String::new(),
            error: String::new(),
            selected: 0,
            demand: 1000.,
            price: 25.,
            production: 1000.,
            cost: 15.,
            carrying: 0.12,
            elasticity: 0.,
            demand_scale: 1.,
            playing: false,
            phase: 0.,
            validation_round: 5,
            stress_sales_fraction: 1.,
            collection_delay: 15.,
        }
    }
}
impl Decision {
    pub fn load(&mut self, path: String) {
        match Evidence::read(&path) {
            Ok(e) => {
                self.selected = e
                    .products
                    .iter()
                    .position(|p| p.name == "Cedar")
                    .unwrap_or(0);
                self.evidence = Some(e);
                self.validation_round = self.evidence.as_ref().unwrap().round;
                self.path = path;
                self.error.clear();
                self.reset();
            }
            Err(e) => self.error = e,
        }
    }
    fn reset(&mut self) {
        if let Some(e) = &self.evidence {
            let p = &e.products[self.selected];
            self.price = p.price.max(0.1);
            self.cost = p.material_cost + p.labor_cost;
            self.demand = e
                .round6_constant_share_anchors_not_recommendations
                .iter()
                .find(|a| a.name == p.name)
                .map(|a| a.grown_segment_potential)
                .unwrap_or(p.units_sold);
            self.production = (self.demand - p.inventory)
                .max(0.)
                .min(p.capacity_next_round * 2.);
            self.demand_scale = 1.;
            self.elasticity = 0.;
            self.playing = false;
        }
    }
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        ui.colored_label(v::AMBER, "Conditional contribution only. Future demand and costs are assumptions; this is not a Capsim profit forecast.");
        ui.label("Observed report / explicit assumptions / conditional tradeoffs");
        ui.horizontal(|ui| {
            ui.label("Evidence JSON");
            ui.add(egui::TextEdit::singleline(&mut self.path).desired_width(480.));
            if ui.button("Load evidence").clicked() {
                self.load(self.path.clone());
            }
        });
        if !self.error.is_empty() {
            ui.colored_label(v::RED, &self.error);
        }
        let Some(e) = self.evidence.clone() else {
            ui.label("Load a checked report extraction. No private course data is bundled with Model Lab.");
            return;
        };
        ui.small(format!(
            "Observed round {} / {} · source SHA-256 {}",
            e.round, e.year, e.source_pdf_sha256
        ));
        let old = self.selected;
        egui::ComboBox::from_id_salt("decision_product")
            .selected_text(format!(
                "{} · {}",
                e.products[self.selected].company, e.products[self.selected].name
            ))
            .show_ui(ui, |ui| {
                for (i, p) in e.products.iter().enumerate() {
                    ui.selectable_value(
                        &mut self.selected,
                        i,
                        format!("{} · {} · {}", p.company, p.name, p.primary_segment),
                    );
                }
            });
        if old != self.selected {
            self.reset();
        }
        let p = &e.products[self.selected];
        if let Some(n) = &e.native_stress { self.native_stress_ui(ui,n); }
        if let Some(h) = &e.historical_validation {
            self.validation_ui(ui, h, &p.name, &p.company);
        }
        let a = e
            .round6_constant_share_anchors_not_recommendations
            .iter()
            .find(|a| a.name == p.name);
        ui.label(format!(
            "Observed sales {:.0}k · opening stock {:.0}k · next-round capacity {:.0}k",
            p.units_sold, p.inventory, p.capacity_next_round
        ));
        if let Some(a) = a {
            ui.colored_label(if a.stock_out{v::AMBER}else{v::MUTED},format!("Constant-share anchors: potential {:.0}k / actual sales {:.0}k. Prior stockout: {}.",a.grown_segment_potential,a.grown_segment_sales,a.stock_out));
        }
        if self.playing {
            self.phase += ui.input(|i| i.stable_dt).min(0.1) as f64;
            self.demand_scale = 1.1 + 0.7 * (self.phase * 0.7).sin();
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(16));
        }
        ui.columns(3, |cols| {
            cols[0].add(egui::Slider::new(&mut self.demand, 0.0..=8000.).text("Base demand (000)"));
            cols[0].add(
                egui::Slider::new(&mut self.demand_scale, 0.4..=1.8).text("Demand multiplier"),
            );
            cols[1].add(egui::Slider::new(&mut self.price, 0.1..=50.).text("Price ($)"));
            cols[1].add(
                egui::Slider::new(&mut self.production, 0.0..=p.capacity_next_round * 2.)
                    .text("Adjusted output (000)"),
            );
            cols[2]
                .add(egui::Slider::new(&mut self.cost, 0.0..=40.).text("Unit variable cost ($)"));
            cols[2].add(
                egui::Slider::new(&mut self.carrying, 0.0..=0.3).text("Inventory carrying rate"),
            );
        });
        ui.add(
            egui::Slider::new(&mut self.elasticity, 0.0..=5.)
                .text("Assumed price elasticity — not estimated"),
        );
        ui.horizontal(|ui| {
            if ui
                .button(if self.playing {
                    "Pause demand sweep"
                } else {
                    "Animate demand sweep"
                })
                .clicked()
            {
                self.playing = !self.playing;
            }
            if ui.button("Reset to observed anchors").clicked() {
                self.reset();
            }
            ui.small("Click the map to select output and demand multiplier.");
        });
        let base = self.demand * (self.price / p.price.max(0.1)).powf(-self.elasticity);
        let o = evaluate(
            p.inventory,
            self.production,
            base * self.demand_scale,
            self.price,
            self.cost,
            self.carrying,
        )
        .unwrap();
        ui.label(format!(
            "Demand {:.0}k  |  Sales {:.0}k  |  Unsold {:.0}k  |  Unfilled demand {:.0}k  |  Contribution ${:.2}m",
            o.demand,
            o.sales,
            o.inventory,
            o.shortage,
            o.contribution / 1000.
        ));
        ui.small(format!("Cash proxy ${:.2}m = receipts − new production cost − carrying cost; excludes receivables, payables and all fixed spending.",o.operating_cash/1000.));
        self.map(ui, p, base);
        ui.colored_label(v::AMBER,"This is conditional contribution, not Capsim profit or a learned optimum. Demand, elasticity, future costs and carrying rate are assumptions. Adjusted output differs from the production-order slider. R&D timing, overtime cost changes, fixed spending, taxes and finance are not included here.");
    }
    fn map(&mut self, ui: &mut egui::Ui, p: &Product, base: f64) {
        let (response, paint) = ui.allocate_painter(
            Vec2::new(ui.available_width().max(400.), 320.),
            egui::Sense::click_and_drag(),
        );
        let graph = Rect::from_min_max(
            response.rect.min + Vec2::new(65., 18.),
            response.rect.max - Vec2::new(18., 48.),
        );
        let maxq = (p.capacity_next_round * 2.).max(1.);
        let n = 36;
        let mut values = Vec::new();
        for y in 0..n {
            for x in 0..n {
                values.push(
                    evaluate(
                        p.inventory,
                        maxq * (x as f64 + 0.5) / n as f64,
                        base * (0.4 + 1.4 * (y as f64 + 0.5) / n as f64),
                        self.price,
                        self.cost,
                        self.carrying,
                    )
                    .unwrap()
                    .contribution,
                );
            }
        }
        let lo = values.iter().copied().fold(f64::INFINITY, f64::min);
        let hi = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        for y in 0..n {
            for x in 0..n {
                let z = (values[y * n + x] - lo) / (hi - lo).max(1.);
                let c = Color32::from_rgb(
                    (35. + 35. * z) as u8,
                    (45. + 160. * z) as u8,
                    (85. + 75. * z) as u8,
                );
                let r = Rect::from_min_max(
                    Pos2::new(
                        graph.left() + graph.width() * x as f32 / n as f32,
                        graph.bottom() - graph.height() * (y + 1) as f32 / n as f32,
                    ),
                    Pos2::new(
                        graph.left() + graph.width() * (x + 1) as f32 / n as f32,
                        graph.bottom() - graph.height() * y as f32 / n as f32,
                    ),
                );
                paint.rect_filled(r, 0., c);
            }
        }
        if (response.clicked() || response.dragged()) && response.interact_pointer_pos().is_some() {
            let pos = response.interact_pointer_pos().unwrap();
            if graph.contains(pos) {
                self.production = ((pos.x - graph.left()) / graph.width()) as f64 * maxq;
                self.demand_scale = 0.4 + ((graph.bottom() - pos.y) / graph.height()) as f64 * 1.4;
                self.playing = false;
            }
        }
        let point = Pos2::new(
            graph.left() + graph.width() * (self.production / maxq) as f32,
            graph.bottom() - graph.height() * ((self.demand_scale - 0.4) / 1.4) as f32,
        );
        paint.circle_stroke(point, 7., Stroke::new(2., Color32::WHITE));
        for i in 0..=4 {
            let t = i as f32 / 4.;
            paint.text(
                Pos2::new(graph.left() + t * graph.width(), graph.bottom() + 8.),
                Align2::CENTER_TOP,
                format!("{:.0}", maxq * t as f64),
                FontId::proportional(11.),
                v::INK,
            );
            paint.text(
                Pos2::new(graph.left() - 8., graph.bottom() - t * graph.height()),
                Align2::RIGHT_CENTER,
                format!("{:.2}×", 0.4 + 1.4 * t),
                FontId::proportional(11.),
                v::INK,
            );
        }
        paint.text(
            Pos2::new(graph.center().x, response.rect.bottom() - 3.),
            Align2::CENTER_BOTTOM,
            "Adjusted production (thousand units)",
            FontId::proportional(12.),
            v::INK,
        );
        ui.small(format!("Vertical axis: demand multiplier · color: contribution ${:.2}m to ${:.2}m (scale recalculates with assumptions)",lo/1000.,hi/1000.));
    }
    fn validation_ui(&mut self, ui: &mut egui::Ui, h: &HistoricalValidation, product: &str, company: &str) {
        egui::CollapsingHeader::new("Historical forecast tests — held-out rounds")
            .default_open(true).show(ui, |ui| {
                ui.small(&h.method);
                let mut rounds: Vec<u32> = h.results.iter().map(|r| r.round).collect();
                rounds.sort_unstable(); rounds.dedup();
                ui.horizontal(|ui| {
                    ui.label("Holdout:");
                    for round in rounds { ui.selectable_value(&mut self.validation_round, round, format!("Round {round}")); }
                });
                let groups: Vec<_> = h.results.iter().filter(|r| r.round==self.validation_round && r.scope==company).collect();
                if groups.is_empty() { ui.label("No company-specific validation loaded for this selection."); return; }
                let max = groups.iter().map(|r|r.mae()).fold(1.0, f64::max);
                ui.label(format!("{company} mean absolute error, thousands of units. Lower is better; computed from loaded predictions."));
                for r in groups {
                    ui.horizontal(|ui| {
                        let name=match r.model.as_str() {"last_sales"=>"Last sales", "grown_primary_potential"=>"Growth × potential", "ridge"=>"Ridge regression", "forest"=>"Random forest", _=>&r.model};
                        ui.add_sized([220.,20.],egui::Label::new(format!("{name}: {:.1}k",r.mae())));
                        let (rect,_) = ui.allocate_exact_size(Vec2::new(180.,12.),egui::Sense::hover());
                        ui.painter().rect_filled(Rect::from_min_size(rect.min,Vec2::new(180.*(r.mae()/max) as f32,12.)),2.,v::AMBER);
                        if let Some(p)=r.rows.iter().find(|p|p.name==product) {
                            ui.small(format!("{product}: predicted {:.0} / actual {:.0} / error {:+.0}k{}",p.predicted,p.actual,p.predicted-p.actual,if p.ending_inventory==0. {" · supply may censor sales"}else{""}));
                        }
                    });
                }
                ui.small("These tests predict realized sales from lagged observations. They do not estimate the causal effect of changing a decision. The scenario map below remains separate conditional accounting.");
                egui::CollapsingHeader::new("Validation limits").show(ui,|ui| { for l in &h.limits {ui.small(l);} });
            });
    }
    fn native_stress_ui(&mut self, ui: &mut egui::Ui, n: &NativeStress) {
        egui::CollapsingHeader::new("Whole-company cash stress — native quoted operating case")
            .default_open(false).show(ui,|ui| {
                ui.label(&n.package);
                ui.small(&n.source);
                let low=n.cases.first().unwrap().revenue;
                let high=n.cases.last().unwrap().revenue;
                let reference=if n.reference_revenue>0. {n.reference_revenue}else{high};
                self.stress_sales_fraction=self.stress_sales_fraction.clamp(low/reference,high/reference);
                ui.add(egui::Slider::new(&mut self.stress_sales_fraction,low/reference..=high/reference).text("Revenue / reference case"));
                ui.add(egui::Slider::new(&mut self.collection_delay,0.0..=60.).text("Additional collection delay (days)"));
                ui.horizontal(|ui| { for c in &n.cases {
                    if ui.button(format!("Native ${:.2}m sales",c.revenue/1000.)).clicked() {self.stress_sales_fraction=c.revenue/reference;}
                }});
                let c=n.interpolate((self.stress_sales_fraction*reference).clamp(low,high)).unwrap();
                let after=c.closing_cash-n.maturity-c.revenue*self.collection_delay/365.;
                ui.label(format!("Revenue ${:.2}m | profit ${:.2}m | year-end cash ${:.2}m",c.revenue/1000.,c.net_profit/1000.,c.closing_cash/1000.));
                ui.colored_label(if after<0. {v::RED}else{v::TEAL},format!("Cash less ${:.2}m maturity and extra receivables: ${:.2}m",n.maturity/1000.,after/1000.));
                ui.small("Linear interpolation only between native quotes; not a demand forecast or a new native result. Delay reserve = revenue × extra days / 365. This is arithmetic liquidity stress, not a monthly cash simulation. Product controls below do not change this fixed-package comparison.");
            });
    }
}
