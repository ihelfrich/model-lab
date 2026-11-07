//! Executable Python reproductions of the displayed experiment's exact split.
use crate::{
    experiment::{ModelId, Run},
    linear::LinearMethod,
};
pub fn python(run: &Run) -> String {
    let cfg = &run.config;
    let mut code=format!("# {} — generated from the displayed experiment\n# Data: {} observations; training/validation rows are fixed below.\n# Run locally with NumPy and scikit-learn. No network access is needed.\nimport json\nimport os\nimport numpy as np\nfrom sklearn.preprocessing import StandardScaler\nfrom sklearn.linear_model import LinearRegression, LogisticRegression, Ridge, Lasso, ElasticNet\nfrom sklearn.ensemble import GradientBoostingRegressor, RandomForestRegressor\nfrom sklearn.tree import DecisionTreeRegressor\nfrom sklearn.metrics import mean_squared_error, log_loss\n\nX = np.array({}, dtype=float)\ny = np.array({}, dtype=float)\ntrain_rows = np.array({}, dtype=int)\nvalidation_rows = np.array({}, dtype=int)\nfeature_names = {}\n\n",cfg.model.descriptor().name,run.data.y.len(),serde_json::to_string_pretty(&run.data.x).unwrap(),serde_json::to_string(&run.data.y).unwrap(),serde_json::to_string(&run.partition.train).unwrap(),serde_json::to_string(&run.partition.valid).unwrap(),serde_json::to_string(&run.data.names).unwrap());
    let (expression, logistic) = match cfg.model {
        ModelId::Linear(m) => {
            code.push_str(&format!("# Learn both centering and scaling from training observations only.\nscaler = StandardScaler(with_std={})\nscaler.fit(X[train_rows])\nZ = scaler.transform(X)\n",if cfg.linear.standardize{"True"}else{"False"}));
            let e = match m {
                LinearMethod::Ols => "LinearRegression()".into(),
                LinearMethod::Logistic => {
                    "LogisticRegression(C=float('inf'), tol=1e-10, max_iter=10000)".into()
                }
                LinearMethod::Ridge => format!(
                    "Ridge(alpha=len(train_rows) * {}, solver='svd')",
                    cfg.linear.alpha
                ),
                LinearMethod::Lasso => format!(
                    "Lasso(alpha={}, tol=1e-10, max_iter=100000)",
                    cfg.linear.alpha
                ),
                LinearMethod::ElasticNet => format!(
                    "ElasticNet(alpha={}, l1_ratio={}, tol=1e-10, max_iter=100000)",
                    cfg.linear.alpha, cfg.linear.mix
                ),
            };
            (e, m == LinearMethod::Logistic)
        }
        ModelId::Tree => {
            code.push_str("Z = X\n");
            (
                format!(
                    "DecisionTreeRegressor(max_depth={}, min_samples_leaf={}, random_state={})",
                    cfg.tree.depth, cfg.tree.min_leaf, cfg.seed
                ),
                false,
            )
        }
        ModelId::Boosting | ModelId::Stochastic => {
            code.push_str("Z = X\n");
            (format!("GradientBoostingRegressor(n_estimators={}, max_depth={}, min_samples_leaf={}, learning_rate={}, subsample={}, random_state={})",cfg.tree.trees,cfg.tree.depth,cfg.tree.min_leaf,cfg.tree.rate,if cfg.model==ModelId::Stochastic{cfg.tree.fraction}else{1.},cfg.seed),false)
        }
        ModelId::Forest => {
            code.push_str("Z = X\n");
            (format!("RandomForestRegressor(n_estimators={}, max_depth={}, min_samples_leaf={}, max_features={}, random_state={})",cfg.tree.trees,cfg.tree.depth,cfg.tree.min_leaf,cfg.tree.mtry,cfg.seed),false)
        }
    };
    if cfg.model == ModelId::Linear(LinearMethod::Ridge) {
        code.push_str(
            "# Native ridge uses RSS/(2n) + alpha*||b||²/2. sklearn Ridge needs n*alpha.\n",
        );
    }
    code.push_str(&format!(
        "model = {expression}\nmodel.fit(Z[train_rows], y[train_rows])\n"
    ));
    if logistic {
        code.push_str("predictions = model.predict_proba(Z)[:, 1]\ntrain_loss = log_loss(y[train_rows], predictions[train_rows], labels=[0, 1])\nvalidation_loss = log_loss(y[validation_rows], predictions[validation_rows], labels=[0, 1])\nmetric_name = 'Log loss'\n");
    } else {
        code.push_str("predictions = model.predict(Z)\ntrain_loss = mean_squared_error(y[train_rows], predictions[train_rows])\nvalidation_loss = mean_squared_error(y[validation_rows], predictions[validation_rows])\nmetric_name = 'MSE'\n");
    }
    if matches!(cfg.model, ModelId::Linear(_)) {
        code.push_str("scale = scaler.scale_ if scaler.with_std else np.ones(X.shape[1])\ncoefficient = model.coef_.reshape(-1) / scale\nintercept = float(np.asarray(model.intercept_).reshape(-1)[0] - scaler.mean_ @ coefficient)\nprint('Intercept (original units):', intercept)\nprint('Coefficients (original units):', dict(zip(feature_names, coefficient)))\n");
    }
    code.push_str("print('Training', metric_name, ':', train_loss)\nprint('Validation', metric_name, ':', validation_loss)\n# This event records a completed library fit; it does not invent internal optimizer states.\nevent = {'kind': 'stage', 'stage': 1, 'metric_name': metric_name,\n         'train_loss': float(train_loss), 'validation_loss': float(validation_loss),\n         'predictions': [float(v) for v in predictions]}\nprint('@LAB@' + json.dumps(event, allow_nan=False), flush=True)\nif os.environ.get('LAB_MODE') == 'step':\n    input()\n");
    if cfg.model.tree_method_for_codegen() {
        code.push_str("# Tree implementations/RNGs differ. Matching seed numbers do not imply native/library fit parity.\n");
    }
    code
}
trait Family {
    fn tree_method_for_codegen(self) -> bool;
}
impl Family for ModelId {
    fn tree_method_for_codegen(self) -> bool {
        !matches!(self, ModelId::Linear(_))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::experiment::{scenario, RunConfig};
    use std::sync::{atomic::AtomicBool, Arc};
    #[test]
    fn generated_linear_code_has_exact_split_and_penalty_conventions() {
        let r = Run::fit(
            "code".into(),
            scenario(0, 1, 1.),
            RunConfig {
                model: ModelId::Linear(LinearMethod::Ridge),
                ..Default::default()
            },
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
        let s = python(&r);
        assert!(s.contains("alpha=len(train_rows) *"));
        assert!(s.contains("scaler.fit(X[train_rows])"));
        assert!(s.contains("validation_rows = np.array("));
    }
}

#[cfg(test)]
mod execution_tests {
    use super::*;
    use crate::{
        experiment::{scenario, RunConfig},
        python::{self, Event},
    };
    use std::sync::{atomic::AtomicBool, Arc};
    #[test]
    fn new_model_python_exports_execute_and_match_native_predictions() {
        let interpreter = python::default_interpreter();
        if !cfg!(target_os = "macos") {
            return;
        }
        for method in LinearMethod::ALL {
            let run = Run::fit(
                "export".into(),
                scenario(
                    if method == LinearMethod::Logistic {
                        3
                    } else {
                        2
                    },
                    4710,
                    0.8,
                ),
                RunConfig {
                    model: ModelId::Linear(method),
                    ..Default::default()
                },
                Arc::new(AtomicBool::new(false)),
            )
            .unwrap();
            let mut session = python::start(
                &interpreter,
                python(&run),
                run.data.clone(),
                false,
                "linear export check".into(),
            )
            .unwrap();
            let mut stage = None;
            let mut done = false;
            let mut output = String::new();
            while let Ok(event) = session
                .receiver
                .recv_timeout(std::time::Duration::from_secs(15))
            {
                match event {
                    Event::Protocol(v) if v["kind"] == "stage" => stage = Some(v),
                    Event::Protocol(v) if v["kind"] == "done" => {
                        done = true;
                        break;
                    }
                    Event::Output(s) => {
                        output.push_str(&s);
                        output.push('\n');
                    }
                    _ => {}
                }
            }
            session.stop();
            assert!(done, "{}: {output}", method.label());
            let event = stage.expect("actual result event");
            let predictions = event["predictions"].as_array().unwrap();
            for (a, b) in predictions.iter().zip(run.predictions(run.last_stage())) {
                assert!((a.as_f64().unwrap() - b).abs() < 2e-5, "{}", method.label());
            }
            assert!(event["train_loss"].as_f64().unwrap().is_finite());
        }
    }
}
