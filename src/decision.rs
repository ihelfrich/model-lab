//! Scenario accounting view. External observations are never bundled as examples.
use crate::visuals as v;
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Stroke, Vec2};
use serde::{Deserialize, Serialize};

#[derive(Clone, Deserialize, Serialize)]
pub struct Product {
    pub name: String,
    pub company: String,
    pub primary_segment: String,
    pub units_sold: f64,
    pub inventory: f64,
    pub price: f64,
    pub material_cost: f64,
    pub labor_cost: f64,
    pub capacity_next_round: f64,
}
#[derive(Clone, Deserialize, Serialize)]
pub struct Anchor {
    pub name: String,
    pub grown_segment_potential: f64,
    pub grown_segment_sales: f64,
    pub stock_out: bool,
}
#[derive(Clone, Deserialize, Serialize)]
pub struct Evidence {
    pub round: u32,
    pub year: u32,
    pub source_pdf_sha256: String,
    pub products: Vec<Product>,
    pub round6_constant_share_anchors_not_recommendations: Vec<Anchor>,
    #[serde(default)]
    pub historical_validation: Option<HistoricalValidation>,
    #[serde(default)]
    pub native_stress: Option<NativeStress>,
}
#[derive(Clone, Deserialize, Serialize)]
pub struct NativeStress {
    pub package: String,
    pub source: String,
    pub maturity: f64,
    #[serde(default)]
    pub reference_revenue: f64,
    pub cases: Vec<NativeCase>,
}
#[derive(Clone, Deserialize, Serialize)]
pub struct NativeCase {
    pub revenue: f64,
    pub net_profit: f64,
    pub closing_cash: f64,
}
impl NativeStress {
    fn validate(&self) -> Result<(), String> {
        if self.package.is_empty() || self.source.is_empty() || self.cases.len()<2
            || self.cases.len()>1000 || !self.maturity.is_finite() || self.maturity<0.
            || !self.reference_revenue.is_finite() || self.reference_revenue<0. {
            return Err("Invalid native stress identity".into());
        }
        let mut previous=-1.;
        for c in &self.cases {
            if !c.revenue.is_finite() || c.revenue<=0. || c.revenue<=previous || !c.net_profit.is_finite() || !c.closing_cash.is_finite() {
                return Err("Native cases require increasing nonnegative revenues and finite outputs".into());
            }
            previous=c.revenue;
        }
        Ok(())
    }
    fn interpolate(&self, revenue: f64) -> Option<NativeCase> {
        if !revenue.is_finite() { return None; }
        for pair in self.cases.windows(2) {
            if revenue>=pair[0].revenue && revenue<=pair[1].revenue {
                let t=(revenue-pair[0].revenue)/(pair[1].revenue-pair[0].revenue);
                return Some(NativeCase {revenue,
                    net_profit:pair[0].net_profit+t*(pair[1].net_profit-pair[0].net_profit),
                    closing_cash:pair[0].closing_cash+t*(pair[1].closing_cash-pair[0].closing_cash)});
            }
        }
        None
    }
}
#[derive(Clone, Deserialize, Serialize)]
pub struct HistoricalValidation {
    pub method: String,
    pub results: Vec<ValidationResult>,
    pub limits: Vec<String>,
}
#[derive(Clone, Deserialize, Serialize)]
pub struct ValidationResult {
    pub round: u32,
    pub scope: String,
    pub model: String,
    pub rows: Vec<Prediction>,
}
#[derive(Clone, Deserialize, Serialize)]
pub struct Prediction {
    pub name: String,
    pub predicted: f64,
    pub actual: f64,
    pub ending_inventory: f64,
}
impl ValidationResult {
    fn mae(&self) -> f64 {
        self.rows.iter().map(|p| (p.predicted-p.actual).abs()).sum::<f64>() / self.rows.len() as f64
    }
    fn validate(&self) -> Result<(), String> {
        let mut names = std::collections::HashSet::new();
        if self.rows.is_empty() || self.rows.len()>1000 || self.model.is_empty() || self.scope.is_empty() {
            return Err("Invalid historical validation group".into());
        }
        for p in &self.rows {
            if p.name.is_empty() || !names.insert(&p.name) || !p.predicted.is_finite()
                || [p.actual,p.ending_inventory].iter().any(|x| !x.is_finite() || *x<0.) {
                return Err("Invalid historical prediction row".into());
            }
        }
        Ok(())
    }
}
impl Evidence {
    pub fn read(path: &str) -> Result<Self, String> {
        let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
        if meta.len() > 8 * 1024 * 1024 {
            return Err("Evidence exceeds 8MB".into());
        }
        let s = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let e: Self = serde_json::from_str(&s).map_err(|e| e.to_string())?;
        e.validate()?;
        Ok(e)
    }
    fn validate(&self) -> Result<(), String> {
        if let Some(n)=&self.native_stress { n.validate()?; }
        if let Some(h) = &self.historical_validation {
            if h.results.is_empty() || h.results.len()>1000 || h.method.is_empty() {
                return Err("Invalid historical validation metadata".into());
            }
            let mut groups = std::collections::HashSet::new();
            for r in &h.results {
                r.validate()?;
                if r.round>self.round || !groups.insert((r.round,&r.scope,&r.model)) {
                    return Err("Duplicate or future validation group".into());
                }
            }
        }
        if self.products.is_empty()
            || self.products.len() > 1000
            || self.source_pdf_sha256.len() != 64
            || !self
                .source_pdf_sha256
                .bytes()
                .all(|c| c.is_ascii_hexdigit())
        {
            return Err("Invalid evidence identity or product count".into());
        }
        let mut names = std::collections::HashSet::new();
        for p in &self.products {
            if p.name.is_empty()
                || !names.insert(&p.name)
                || [
                    p.units_sold,
                    p.inventory,
                    p.price,
                    p.material_cost,
                    p.labor_cost,
                    p.capacity_next_round,
                ]
                .iter()
                .any(|x| !x.is_finite() || *x < 0.)
            {
                return Err(format!("Invalid or duplicate product {}", p.name));
            }
        }
        for a in &self.round6_constant_share_anchors_not_recommendations {
            if !names.contains(&a.name)
                || [a.grown_segment_potential, a.grown_segment_sales]
                    .iter()
                    .any(|x| !x.is_finite() || *x < 0.)
            {
                return Err("Invalid demand anchor".into());
            }
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Outcome {
    pub demand: f64,
    pub sales: f64,
    pub inventory: f64,
    pub shortage: f64,
    pub contribution: f64,
    pub operating_cash: f64,
}
/// Quantities and currency are thousands; price/cost are dollars per unit.
/// Unit costs held constant, immediate cash collection assumed. No net-profit claim.
pub fn evaluate(
    opening: f64,
    production: f64,
    demand: f64,
    price: f64,
    cost: f64,
    carrying: f64,
) -> Result<Outcome, String> {
    if [opening, production, demand, price, cost, carrying]
        .iter()
        .any(|v| !v.is_finite() || *v < 0.)
    {
        return Err("Scenario inputs must be finite and nonnegative".into());
    }
    let supply = opening + production;
    let sales = demand.min(supply);
    let inventory = supply - sales;
    let carry = inventory * cost * carrying;
    Ok(Outcome {
        demand,
        sales,
        inventory,
        shortage: (demand - supply).max(0.),
        contribution: sales * (price - cost) - carry,
        operating_cash: sales * price - production * cost - carry,
    })
}

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
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn conservation_and_cash_timing() {
        let o = evaluate(100., 900., 800., 25., 10., 0.1).unwrap();
        assert_eq!(o.sales + o.inventory, 1000.);
        assert_eq!(o.shortage, 0.);
        assert_eq!(o.contribution, 11800.);
        assert_eq!(o.operating_cash, 10800.);
    }
    #[test]
    fn stockout_and_invalid() {
        let o = evaluate(0., 100., 150., 25., 10., 0.1).unwrap();
        assert_eq!(o.sales, 100.);
        assert_eq!(o.shortage, 50.);
        assert!(evaluate(0., -1., 1., 1., 1., 0.).is_err());
        assert!(evaluate(0., 1., f64::NAN, 1., 1., 0.).is_err());
    }
    #[test]
    fn historical_error_is_recomputed_and_duplicate_rows_rejected() {
        let mut r=ValidationResult{round:3,scope:"Example".into(),model:"ridge".into(),rows:vec![
            Prediction{name:"A".into(),predicted:80.,actual:100.,ending_inventory:0.},
            Prediction{name:"B".into(),predicted:-10.,actual:30.,ending_inventory:10.}]};
        assert_eq!(r.mae(),30.);
        assert!(r.validate().is_ok()); // Negative unconstrained ridge predictions remain visible.
        r.rows.push(r.rows[0].clone());
        assert!(r.validate().is_err());
        r.rows.clear();
        assert!(r.validate().is_err());
    }
    #[test]
    fn native_interpolation_preserves_quotes_and_refuses_extrapolation() {
        let n=NativeStress{package:"Synthetic fixture".into(),source:"test".into(),maturity:20.,reference_revenue:150.,cases:vec![
            NativeCase{revenue:100.,net_profit:-10.,closing_cash:30.},NativeCase{revenue:200.,net_profit:20.,closing_cash:80.}]};
        assert!(n.validate().is_ok());
        assert_eq!(n.interpolate(100.).unwrap().closing_cash,30.);
        assert_eq!(n.interpolate(150.).unwrap().net_profit,5.);
        assert_eq!(n.interpolate(200.).unwrap().closing_cash,80.);
        assert!(n.interpolate(99.).is_none());
        assert!(n.interpolate(f64::NAN).is_none());
    }
}
