//! Plots as Vega-Lite specifications built on the Rust side from real
//! experiment data. Every plot carries the same rows as an accessible table,
//! uses one theme, and shows a rate or score only with its denominator.
use model_lab_engine::byhand::Round;
use model_lab_engine::experiment::Run;
use model_lab_engine::model::Fit;
use model_lab_engine::science::Analysis;
use serde::Serialize;
use serde_json::{json, Value};

pub const C_TRAIN: &str = "#2A6F97";
pub const C_VALID: &str = "#C8553D";
pub const C_OOB: &str = "#5B8E7D";
pub const C_SLICE: &str = "#7B6D8D";
pub const C_HIGHLIGHT: &str = "#F2A541";

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Table {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Value>>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Plot {
    pub id: String,
    pub title: String,
    pub description: String,
    pub spec: Value,
    pub table: Table,
}

/// The one theme: palette tokens, inherited font, six ticks.
pub fn theme() -> Value {
    json!({
        "font": "inherit",
        "background": null,
        "axis": {"tickCount": 6, "labelFontSize": 12, "titleFontSize": 13, "grid": true, "gridOpacity": 0.25},
        "legend": {"labelFontSize": 12, "titleFontSize": 12, "orient": "top"},
        "view": {"stroke": null},
        "range": {"category": [C_TRAIN, C_VALID, C_OOB, C_SLICE, C_HIGHLIGHT]},
        "point": {"filled": true, "size": 42},
        "line": {"strokeWidth": 2}
    })
}

fn base(title: &str, datasets: Value, layer: Value) -> Value {
    json!({
        "$schema": "https://vega.github.io/schema/vega-lite/v6.json",
        "title": title,
        "width": "container",
        "height": 300,
        "config": theme(),
        "datasets": datasets,
        "layer": layer
    })
}

fn objects(columns: &[&str], rows: &[Vec<Value>]) -> Vec<Value> {
    rows.iter()
        .map(|r| Value::Object(columns.iter().zip(r).map(|(c, v)| (c.to_string(), v.clone())).collect()))
        .collect()
}

fn table(columns: &[&str], rows: Vec<Vec<Value>>) -> Table {
    Table { columns: columns.iter().map(|s| s.to_string()).collect(), rows }
}

fn num(v: f64) -> Value {
    json!(v)
}

/// Training and validation loss by stage, with a rule at the best stage.
/// Legend labels carry the number of rows each curve is averaged over.
pub fn loss_curve(run: &Run) -> Plot {
    let n_train = run.partition.train.len();
    let n_valid = run.partition.valid.len();
    let train_label = format!("training (n={n_train})");
    let valid_label = format!("validation (n={n_valid})");
    let columns = ["stage", "series", "loss"];
    let mut rows = vec![];
    for s in &run.trace {
        rows.push(vec![json!(s.stage), json!(train_label), num(s.train_loss)]);
        rows.push(vec![json!(s.stage), json!(valid_label), num(s.valid_loss)]);
    }
    let best = run
        .trace
        .iter()
        .min_by(|a, b| a.valid_loss.total_cmp(&b.valid_loss))
        .map(|s| s.stage)
        .unwrap_or(0);
    let spec = base(
        "Loss by stage",
        json!({"main": objects(&columns, &rows), "best": [{"stage": best, "label": format!("best stage {best}")}]}),
        json!([
            {"data": {"name": "main"}, "mark": "line",
             "encoding": {"x": {"field": "stage", "type": "quantitative", "title": "stage (trees added)"},
                          "y": {"field": "loss", "type": "quantitative", "title": "mean squared error"},
                          "color": {"field": "series", "type": "nominal", "title": null,
                                    "scale": {"domain": [train_label, valid_label], "range": [C_TRAIN, C_VALID]}}}},
            {"data": {"name": "best"}, "mark": {"type": "rule", "strokeDash": [4, 4], "color": C_HIGHLIGHT},
             "encoding": {"x": {"field": "stage", "type": "quantitative"}}},
            {"data": {"name": "best"}, "mark": {"type": "text", "align": "left", "dx": 4, "dy": -130, "color": C_HIGHLIGHT},
             "encoding": {"x": {"field": "stage", "type": "quantitative"}, "text": {"field": "label"}}}
        ]),
    );
    Plot {
        id: "loss_curve".into(),
        title: "Loss by stage".into(),
        description: format!(
            "Training loss over {n_train} rows and validation loss over {n_valid} rows at every stage; the dashed rule marks the stage with the lowest validation loss."
        ),
        spec,
        table: table(&columns, rows),
    }
}

/// Observations against one predictor with the model's slice through them;
/// training and validation rows are shape-coded, the selected row is ringed.
pub fn scatter_with_slice(run: &Run, feature: usize, stage: usize, selected: Option<usize>) -> Plot {
    let name = run.data.names[feature].clone();
    let pred = run.predictions(stage);
    let sample: Option<&Vec<usize>> = match &run.fitted {
        model_lab_engine::experiment::FittedModel::Trees(f) if stage >= 1 => f.samples.get(stage - 1),
        _ => None,
    };
    let columns = ["row", "x", "y", "prediction", "split", "selected", "in_sample"];
    let mut rows = vec![];
    for (i, x) in run.data.x.iter().enumerate() {
        let training = run.partition.train.contains(&i);
        let split = if training { "training" } else { "validation" };
        let in_sample = match sample {
            Some(s) => json!(s.contains(&i)),
            None => json!(training),
        };
        rows.push(vec![
            json!(i),
            num(x[feature]),
            num(run.data.y[i]),
            num(pred[i]),
            json!(split),
            json!(selected == Some(i)),
            in_sample,
        ]);
    }
    let slice: Vec<Value> = run
        .slice(feature, stage)
        .into_iter()
        .map(|(x, y)| json!({"x": x, "prediction": y}))
        .collect();
    let selected_rows: Vec<Value> = objects(&columns, &rows)
        .into_iter()
        .filter(|r| r["selected"] == json!(true))
        .collect();
    let spec = base(
        &format!("{} against {name}", run.data.target),
        json!({"main": objects(&columns, &rows), "slice": slice, "selected": selected_rows}),
        json!([
            {"data": {"name": "main"}, "mark": "point", "params": [{"name": "select_row", "select": {"type": "point", "fields": ["row"]}}],
             "encoding": {"x": {"field": "x", "type": "quantitative", "title": name},
                          "y": {"field": "y", "type": "quantitative", "title": run.data.target},
                          "shape": {"field": "split", "type": "nominal", "title": null},
                          "fill": {"condition": {"test": "datum.in_sample", "field": "split", "type": "nominal", "scale": {"domain": ["training", "validation"], "range": [C_TRAIN, C_VALID]}}, "value": "transparent"},
                          "color": {"field": "split", "type": "nominal", "title": null,
                                    "scale": {"domain": ["training", "validation"], "range": [C_TRAIN, C_VALID]}}}},
            {"data": {"name": "slice"}, "mark": {"type": "line", "color": C_SLICE},
             "encoding": {"x": {"field": "x", "type": "quantitative"}, "y": {"field": "prediction", "type": "quantitative"}}},
            {"data": {"name": "selected"}, "mark": {"type": "point", "filled": false, "size": 220, "strokeWidth": 3, "stroke": C_HIGHLIGHT},
             "encoding": {"x": {"field": "x", "type": "quantitative"}, "y": {"field": "y", "type": "quantitative"}}}
        ]),
    );
    Plot {
        id: "scatter".into(),
        title: format!("{} against {name}", run.data.target),
        description: format!(
            "Each point is an observation; the line is the model at stage {stage} with the other predictors held at their training medians (a slice, not a partial dependence). Hollow points were not in the sample this stage's tree was fitted to."
        ),
        spec,
        table: table(&columns, rows),
    }
}

/// The four-point route: outcomes, predictions before and after each round,
/// and the residual each round still has to explain.
pub fn byhand_rounds(rounds: &[Round], upto: usize) -> Plot {
    let columns = ["round", "row", "y", "before", "after", "residual_after"];
    let mut rows = vec![];
    for r in rounds.iter().take(upto + 1) {
        for i in 0..r.after.len() {
            let y = r.before[i] + r.residual_before[i];
            rows.push(vec![json!(r.stage), json!(i), num(y), num(r.before[i]), num(r.after[i]), num(r.residual_after[i])]);
        }
    }
    let spec = base(
        "Predictions by round",
        json!({"main": objects(&columns, &rows)}),
        json!([
            {"data": {"name": "main"}, "mark": {"type": "rule", "color": C_VALID, "opacity": 0.6},
             "encoding": {"x": {"field": "row", "type": "ordinal", "title": "observation"},
                          "y": {"field": "after", "type": "quantitative", "title": "value"}, "y2": {"field": "y"},
                          "column": {"field": "round", "type": "ordinal", "title": "round"}}},
            {"data": {"name": "main"}, "mark": {"type": "point", "color": C_TRAIN},
             "encoding": {"x": {"field": "row", "type": "ordinal"}, "y": {"field": "y", "type": "quantitative"},
                          "column": {"field": "round", "type": "ordinal"}}},
            {"data": {"name": "main"}, "mark": {"type": "tick", "color": C_HIGHLIGHT, "thickness": 3},
             "encoding": {"x": {"field": "row", "type": "ordinal"}, "y": {"field": "after", "type": "quantitative"},
                          "column": {"field": "round", "type": "ordinal"}}}
        ]),
    );
    Plot {
        id: "byhand_rounds".into(),
        title: "Predictions by round".into(),
        description: "Points are the outcomes, ticks the prediction after each round, and the red segments the residual that round leaves.".into(),
        spec,
        table: table(&columns, rows),
    }
}

/// Out-of-bag error by stage with its coverage: how many training rows had
/// an out-of-bag prediction at that stage, out of how many.
pub fn oob_curve(fit: &Fit) -> Plot {
    let n_train = fit.train.len();
    let columns = ["stage", "oob_mse", "oob_coverage", "rows_covered", "rows_train"];
    let mut rows = vec![];
    for (stage, cov) in fit.oob_coverage.iter().enumerate() {
        let covered = (cov * n_train as f64).round() as usize;
        rows.push(vec![
            json!(stage),
            fit.oob_mse[stage].map(num).unwrap_or(Value::Null),
            num(*cov),
            json!(covered),
            json!(n_train),
        ]);
    }
    let spec = base(
        "Out-of-bag error by stage",
        json!({"main": objects(&columns, &rows)}),
        json!([
            {"data": {"name": "main"}, "mark": {"type": "line", "color": C_OOB},
             "encoding": {"x": {"field": "stage", "type": "quantitative", "title": "stage (trees)"},
                          "y": {"field": "oob_mse", "type": "quantitative", "title": "out-of-bag MSE"}}},
            {"data": {"name": "main"}, "mark": {"type": "line", "color": C_SLICE, "strokeDash": [2, 2]},
             "encoding": {"x": {"field": "stage", "type": "quantitative"},
                          "y": {"field": "oob_coverage", "type": "quantitative", "title": format!("coverage (share of {n_train} training rows)"), "axis": {"orient": "right"}}}}
        ]),
    );
    let spec = {
        let mut s = spec;
        s["resolve"] = json!({"scale": {"y": "independent"}});
        s
    };
    Plot {
        id: "oob_curve".into(),
        title: "Out-of-bag error by stage".into(),
        description: format!(
            "Out-of-bag mean squared error at each stage, and the share of the {n_train} training rows that had an out-of-bag prediction; early stages cover few rows."
        ),
        spec,
        table: table(&columns, rows),
    }
}

/// Permutation importance with the spread over repeats.
pub fn importance(analysis: &Analysis) -> Plot {
    let columns = ["feature", "mean", "sd", "low", "high"];
    let rows: Vec<Vec<Value>> = analysis
        .importance
        .iter()
        .map(|(name, mean, sd)| vec![json!(name), num(*mean), num(*sd), num(mean - sd), num(mean + sd)])
        .collect();
    let title = format!("Permutation importance (3 repeats, {} validation rows)", analysis.validation_rows);
    let spec = base(
        &title,
        json!({"main": objects(&columns, &rows)}),
        json!([
            {"data": {"name": "main"}, "mark": {"type": "bar", "color": C_TRAIN},
             "encoding": {"y": {"field": "feature", "type": "nominal", "sort": "-x", "title": null},
                          "x": {"field": "mean", "type": "quantitative", "title": "increase in validation MSE when permuted"}}},
            {"data": {"name": "main"}, "mark": {"type": "rule", "color": C_HIGHLIGHT, "strokeWidth": 2},
             "encoding": {"y": {"field": "feature", "type": "nominal", "sort": "-x"},
                          "x": {"field": "low", "type": "quantitative"}, "x2": {"field": "high"}}}
        ]),
    );
    Plot {
        id: "importance".into(),
        title,
        description: "Bars are the mean increase in validation error when one predictor is shuffled; the orange range is ± one standard deviation over 3 repeats. Importance ranks predictive use, not causes.".into(),
        spec,
        table: table(&columns, rows),
    }
}

/// Individual conditional expectation lines, their mean (the partial
/// dependence) and the slice, which fixes the other predictors instead of
/// averaging over them.
pub fn pdp_ice(analysis: &Analysis, slice: &[(f64, f64)]) -> Plot {
    let columns = ["series", "x", "prediction"];
    let mut rows = vec![];
    for (k, line) in analysis.ice.iter().enumerate() {
        for (x, p) in analysis.grid.iter().zip(line) {
            rows.push(vec![json!(format!("ICE {k}")), num(*x), num(*p)]);
        }
    }
    for (x, p) in analysis.grid.iter().zip(&analysis.pdp) {
        rows.push(vec![json!("PDP (mean of ICE)"), num(*x), num(*p)]);
    }
    for (x, p) in slice {
        rows.push(vec![json!("slice at training medians"), num(*x), num(*p)]);
    }
    let spec = base(
        "Partial dependence, ICE and the slice",
        json!({"main": objects(&columns, &rows)}),
        json!([
            {"data": {"name": "main"}, "transform": [{"filter": "indexof(datum.series, 'ICE') == 0"}],
             "mark": {"type": "line", "color": C_TRAIN, "opacity": 0.35, "strokeWidth": 1},
             "encoding": {"x": {"field": "x", "type": "quantitative"}, "y": {"field": "prediction", "type": "quantitative"}, "detail": {"field": "series"}}},
            {"data": {"name": "main"}, "transform": [{"filter": "datum.series == 'PDP (mean of ICE)'"}],
             "mark": {"type": "line", "color": C_VALID, "strokeWidth": 3},
             "encoding": {"x": {"field": "x", "type": "quantitative"}, "y": {"field": "prediction", "type": "quantitative"}}},
            {"data": {"name": "main"}, "transform": [{"filter": "datum.series == 'slice at training medians'"}],
             "mark": {"type": "line", "color": C_SLICE, "strokeDash": [6, 3], "strokeWidth": 2},
             "encoding": {"x": {"field": "x", "type": "quantitative"}, "y": {"field": "prediction", "type": "quantitative"}}}
        ]),
    );
    Plot {
        id: "pdp_ice".into(),
        title: "Partial dependence, ICE and the slice".into(),
        description: format!(
            "Thin lines are {} individual conditional expectations (of {} reference rows), the red line their mean (partial dependence), the dashed line the slice at training medians. The slice is not a PDP.",
            analysis.ice.len(), analysis.reference_rows
        ),
        spec,
        table: table(&columns, rows),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dataset_rows(spec: &Value) -> usize {
        spec["datasets"]["main"].as_array().map(Vec::len).unwrap_or(0)
    }
    use model_lab_engine::experiment::{scenario, ModelId, RunConfig};
    use model_lab_engine::science::analyze;
    use std::sync::{atomic::AtomicBool, Arc};

    fn run() -> Run {
        let mut cfg = RunConfig::default();
        cfg.model = ModelId::parse("boosting").unwrap();
        cfg.tree.trees = 15;
        cfg.seed = 1;
        Run::fit("t".into(), scenario(0, 1, 0.5), cfg, Arc::new(AtomicBool::new(false))).unwrap()
    }
    fn forest() -> Run {
        let mut cfg = RunConfig::default();
        cfg.model = ModelId::parse("forest").unwrap();
        cfg.tree.trees = 10;
        Run::fit("f".into(), scenario(0, 1, 0.5), cfg, Arc::new(AtomicBool::new(false))).unwrap()
    }
    fn fit(run: &Run) -> &Fit {
        match &run.fitted {
            model_lab_engine::experiment::FittedModel::Trees(f) => f,
            _ => unreachable!(),
        }
    }

    #[test]
    fn every_plot_has_a_table_matching_its_data() {
        let r = run();
        let f = forest();
        let a = analyze(fit(&f), 0, 10);
        let rounds = model_lab_engine::byhand::boosting_rounds(&r.data, 0.5, 1, 3).unwrap();
        for p in [
            loss_curve(&r),
            scatter_with_slice(&r, 0, 15, Some(7)),
            byhand_rounds(&rounds, 2),
            oob_curve(fit(&f)),
            importance(&a),
            pdp_ice(&a, &f.slice(0, 10)),
        ] {
            assert_eq!(p.table.rows.len(), dataset_rows(&p.spec), "{}", p.id);
            assert!(!p.description.is_empty());
        }
    }

    #[test]
    fn loss_curve_marks_the_best_stage_and_denominators() {
        let r = run();
        let p = loss_curve(&r);
        let best = fit(&r).best_stage();
        assert_eq!(p.spec["datasets"]["best"][0]["stage"], json!(best));
        let text = p.spec.to_string();
        assert!(text.contains("n=165"), "{text}");
        assert!(text.contains("n=55"), "{text}");
        assert!(p.spec["layer"].as_array().unwrap().iter().any(|l| l["mark"]["type"] == json!("rule")));
    }

    #[test]
    fn specs_use_one_theme() {
        let r = run();
        for p in [loss_curve(&r), scatter_with_slice(&r, 0, 3, None)] {
            assert_eq!(p.spec["config"], theme());
        }
    }

    #[test]
    fn oob_table_carries_coverage_with_rows() {
        let f = forest();
        let p = oob_curve(fit(&f));
        assert_eq!(p.table.columns, vec!["stage", "oob_mse", "oob_coverage", "rows_covered", "rows_train"]);
    }

    #[test]
    fn scatter_rings_the_selected_row() {
        let r = run();
        let p = scatter_with_slice(&r, 0, 15, Some(7));
        let selected = p.spec["datasets"]["selected"].as_array().unwrap();
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0]["row"], json!(7));
        assert!(p.spec["layer"].as_array().unwrap().iter().any(|l| l["mark"]["stroke"] == json!(C_HIGHLIGHT)));
    }
}
