use crate::data::Table;
pub struct Example {
    pub name: &'static str,
    pub csv: &'static str,
    pub lesson: &'static str,
}
pub fn examples() -> [Example; 3] {
    [
    Example{name:"Income, age and year",csv:include_str!("../examples/income.csv"),lesson:"3,000 rows from your existing income course data. Predict income using age/year. Compare shallow and deeper fits; inspect nonlinear age patterns and validation error. Associations are predictive, not causal. Categorical columns were omitted rather than silently encoded."},
    Example{name:"Diamond prices",csv:include_str!("../examples/diamonds.csv"),lesson:"1,500 reproducibly sampled rows from your existing 53,940-row diamonds file. Predict price from carat and numeric measurements. Compare carat alone with correlated dimensions, tree depth and forest candidate-feature counts. All sampled values are preserved; categorical grading variables are omitted."},
    Example{name:"Florida communities",csv:include_str!("../examples/florida.csv"),lesson:"All 90 rows, with eight numeric predictors selected from your existing Florida course dataset. Predict violentcrimes.perpop. Compare training and validation errors with deeper trees; small validation samples are unstable. Community associations are not individual risk estimates or causal effects."},
]
}
pub fn table(index: usize) -> Result<Table, String> {
    let e = &examples()[index];
    Table::parse(e.name, e.csv)
}
#[cfg(test)]
mod tests {
    #[test]
    fn existing_data_examples_parse_and_fit_all_methods() {
        use crate::model::{self, Config, Method};
        use std::sync::{atomic::AtomicBool, Arc};
        for i in 0..3 {
            let t = super::table(i).unwrap();
            assert_eq!(t.rows.len(), [3000, 1500, 90][i]);
            let target = t.headers.len() - 1;
            let features: Vec<_> = (0..target).collect();
            let d = t.dataset(target, &features).unwrap();
            for method in Method::ALL {
                let f = model::fit(
                    d.clone(),
                    Config {
                        method,
                        trees: 3,
                        ..Default::default()
                    },
                    Arc::new(AtomicBool::new(false)),
                )
                .unwrap();
                assert!(f.valid_mse.iter().all(|v| v.is_finite()));
            }
        }
    }
}
