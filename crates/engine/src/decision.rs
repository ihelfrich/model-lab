//! Scenario accounting: evidence records, validation and the operating-outcome arithmetic.
//! External observations are never bundled as examples.
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
    pub fn interpolate(&self, revenue: f64) -> Option<NativeCase> {
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
    pub fn mae(&self) -> f64 {
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
