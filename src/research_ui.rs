//! Research projects, inference, resampling and teaching share immutable results.
//! The UI launches explicit jobs; it never turns a changed control into a new test disclosure.
use crate::{
    adapter::{self, Event, ModelAdapter},
    data::Table,
    experiment::{Run, MODELS},
    visuals::{self as v, Plot, Series},
};
use eframe::egui::{self, RichText};
use serde_json::{json, Value};
use std::sync::Arc;

const TABS: [&str; 6] = [
    "Design",
    "Inference",
    "Validation",
    "Project",
    "Teach",
    "Simulate",
];
pub struct Research {
    pub template: Value,
    pub history: Vec<Arc<Value>>,
    pub studies: crate::study_ui::StudyPanel,
    pub disclosures: Vec<Value>,
    pub selected: usize,
    pub tab: usize,
    pub interpreter: String,
    pub project_path: String,
    pub export_path: String,
    pub notes: String,
    pub status: String,
    pub error: String,
    pub pinned_source: bool,
    previous_project: Option<Value>,
    workspace_rows: Vec<usize>,
    source_key: String,
    session: Option<adapter::Session>,
    pending: Option<Value>,
    compare: Option<Value>,
    progress: Value,
    alpha_grid: String,
    missing_tokens_input: String,
    restriction_terms: Vec<bool>,
    restriction_values: String,
    teaching_answers: Vec<String>,
    teaching_revealed: bool,
    presentation: bool,
    test_acknowledged: bool,
}
impl Default for Research {
    fn default() -> Self {
        Self {
            template: Value::Null,
            history: vec![],
            studies: crate::study_ui::StudyPanel::default(),
            disclosures: vec![],
            selected: 0,
            tab: 0,
            interpreter: adapter::default_interpreter(),
            project_path: String::new(),
            export_path: String::new(),
            notes: String::new(),
            status: "Choose the research design, then run an explicit operation.".into(),
            error: String::new(),
            pinned_source: false,
            previous_project: None,
            workspace_rows: vec![],
            source_key: String::new(),
            session: None,
            pending: None,
            compare: None,
            progress: Value::Null,
            alpha_grid: "0.001, 0.01, 0.1, 1".into(),
            missing_tokens_input: String::new(),
            restriction_terms: vec![],
            restriction_values: "0".into(),
            teaching_answers: vec![String::new(); 3],
            teaching_revealed: false,
            presentation: false,
            test_acknowledged: false,
        }
    }
}
impl Research {
    pub fn select_model(&mut self, model: &str) {
        if self.template.is_null() {
            return;
        }
        self.template["model"] = json!(model);
        self.template["inference"]["covariance"] = json!(if model == "logistic" {
            "classical"
        } else {
            "HC3"
        });
        self.template["inference"]["restriction"] = Value::Null;
        self.restriction_terms.clear();
        self.template["resampling"]["kind"] = json!(if model == "logistic" {
            "stratified"
        } else {
            "random"
        });
    }
    #[cfg_attr(not(feature = "acceptance"), allow(dead_code))]
    pub fn show_teacher(&mut self) {
        self.tab = 4;
        self.teaching_revealed = true;
        self.presentation = true;
    }
    pub fn busy(&self) -> bool {
        self.session.is_some() || self.studies.busy()
    }
    pub fn synchronize(&mut self, run: &Run, table: Option<&Table>, revision: u64) {
        if self.pinned_source || self.studies.busy() {
            return;
        }
        let key = format!("{revision}:{}:{:?}", run.fingerprint, run.config);
        if key == self.source_key {
            return;
        }
        let raw = table
            .map(adapter::table_dataset)
            .unwrap_or_else(|| adapter::dataset(&run.data, &run.name));
        self.workspace_rows = if table.is_none() {
            run.partition.train.clone()
        } else {
            vec![]
        };
        let old_data = self.template.get("dataset").cloned();
        let old_roles = self.template.get("roles").cloned();
        self.template = adapter::request_for_run(run, "inference");
        self.missing_tokens_input.clear();
        self.template["dataset"] = raw;
        if old_data.as_ref() == self.template.get("dataset") {
            if let Some(roles) = old_roles {
                self.template["roles"] = roles;
            }
        } else if let Some(table) = table {
            let target = table
                .numeric
                .iter()
                .rposition(|v| *v)
                .unwrap_or(table.headers.len() - 1);
            let mut features = table
                .headers
                .iter()
                .enumerate()
                .filter_map(|(j, n)| (j != target).then_some(n.clone()))
                .collect::<Vec<_>>();
            features.truncate(64);
            self.template["roles"] =
                json!({"target":table.headers[target],"features":features,"categorical":[]});
            self.template.as_object_mut().unwrap().remove("sample_rows");
        }
        self.source_key = key;
        self.restriction_terms.clear();
        self.test_acknowledged = false;
    }
    pub fn poll(&mut self, ctx: &egui::Context) {
        self.studies.poll(ctx);
        let events = self
            .session
            .as_ref()
            .map(|s| s.receiver.try_iter().collect::<Vec<_>>())
            .unwrap_or_default();
        for event in events {
            match event {
                Event::Progress(p) => self.progress = p,
                Event::Failed(error) => {
                    self.error = error;
                    self.status = "The scientific operation was not accepted.".into();
                    self.finish_disclosure("failed or uncertain");
                    self.session = None;
                    self.pending = None;
                    self.compare = None;
                }
                Event::Completed(response) => {
                    let request = self.pending.take().unwrap();
                    let operation = request["operation"].as_str().unwrap_or("").to_owned();
                    self.session = None;
                    if operation == "export" {
                        self.status = format!(
                            "Exported verified research bundle to {}",
                            response["result"]["destination"]
                                .as_str()
                                .unwrap_or("selected folder")
                        );
                    } else {
                        self.finish_disclosure_id(
                            request["request_id"].as_str().unwrap_or(""),
                            "completed",
                        );
                        let reproduction=self.compare.take().map(|expected|{
                            let drift=expected["versions"].as_object().map(|versions|versions.iter().filter(|(key,value)|response["versions"][key.as_str()]!=**value).map(|(key,value)|json!({"package":key,"recorded":value,"current":response["versions"][key]})).collect::<Vec<_>>()).unwrap_or_default();
                            json!({"numerical_match":numerical_match(&expected["result"],&response["result"]),"worker_changed":expected["worker_sha256"]!=response["worker_sha256"],"version_drift":drift})
                        });
                        let reproduced = reproduction
                            .as_ref()
                            .and_then(|r| r["numerical_match"].as_bool());
                        self.status=match reproduced{Some(true)=>"Numerical reproduction passed under the current scientific environment.".into(),Some(false)=>{self.error="The reproduction differed from the saved scientific result. Both versions are retained.".into();"Reproduction difference recorded".into()},None=>"Scientific result completed; request and worker source hashes verified.".into()};
                        let result_hash =
                            adapter::sha256(&serde_json::to_vec(&response["result"]).unwrap());
                        self.history.push(Arc::new(json!({"request":request,"response":response,"result_sha256":result_hash,"reproduced":reproduced,"reproduction":reproduction})));
                        self.selected = self.history.len() - 1;
                        self.tab = if operation == "inference" {
                            1
                        } else if operation == "resample" || operation == "evaluate_test" {
                            2
                        } else {
                            3
                        };
                    }
                }
            }
        }
        if self.busy() {
            ctx.request_repaint_after(std::time::Duration::from_millis(25));
        }
    }
    fn finish_disclosure_id(&mut self, id: &str, status: &str) {
        for d in &mut self.disclosures {
            if d["request_id"] == id {
                d["status"] = json!(status);
            }
        }
    }
    fn finish_disclosure(&mut self, status: &str) {
        if let Some(id) = self
            .pending
            .as_ref()
            .and_then(|p| p["request_id"].as_str())
            .map(str::to_owned)
        {
            self.finish_disclosure_id(&id, status);
        }
    }
    pub fn launch(&mut self, mut request: Value) -> Result<(), String> {
        if self.history.len() >= 12 && request["operation"] != "export" {
            return Err("This project holds 12 completed analyses. Save/export it and start a new project; existing history is retained.".into());
        }
        if self.busy() {
            return Err(
                "A scientific operation is already running. Cancel it or wait for completion."
                    .into(),
            );
        }
        request["request_id"] = json!(adapter::identity());
        let session = adapter::launch(request.clone(), self.interpreter.clone())?;
        if request["operation"] == "evaluate_test" {
            self.disclosures.push(json!({"request_id":request["request_id"],"dataset_key":data_key(&request),"status":"requested; disclosure may occur","model":request["model"],"seed":request["seed"]}));
        }
        self.error.clear();
        self.progress = Value::Null;
        self.status = format!(
            "Running {} in the scientific adapter…",
            request["operation"].as_str().unwrap_or("operation")
        );
        self.pending = Some(request);
        self.session = Some(session);
        Ok(())
    }
    fn start(&mut self, operation: &str) {
        let mut request = self.template.clone();
        request["operation"] = json!(operation);
        let grid = self
            .alpha_grid
            .split(',')
            .map(|s| s.trim().parse::<f64>())
            .collect::<Result<Vec<_>, _>>();
        match grid {
            Ok(values)
                if !values.is_empty() && values.iter().all(|x| x.is_finite() && *x >= 0.) =>
            {
                request["resampling"]["alpha_grid"] = json!(values)
            }
            _ => {
                self.error =
                    "Penalty grid must contain finite nonnegative numbers separated by commas."
                        .into();
                return;
            }
        }
        if let Err(e) = self.launch(request) {
            self.error = e;
        }
    }
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        v::section(ui,"Research and teaching","Explicit designs, independently fitted folds, inferential conventions and reproducible artifacts.");
        ui.horizontal_wrapped(|ui| {
            for (i, title) in TABS.iter().enumerate() {
                if ui
                    .push_id(("research_tab", i), |ui| {
                        ui.selectable_label(self.tab == i, *title)
                    })
                    .inner
                    .clicked()
                {
                    self.tab = i;
                }
            }
        });
        ui.add_space(8.);
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(&self.status).size(12.).color(v::TEAL));
            if self.session.is_some() {
                ui.spinner();
                if ui.button("Cancel operation").clicked() {
                    self.finish_disclosure("canceled or uncertain");
                    self.session = None;
                    self.pending = None;
                    self.compare = None;
                    self.status = "Cancellation requested; no new result accepted.".into();
                }
                ui.label(format!(
                    "{} · {} model fits started",
                    self.progress["phase"].as_str().unwrap_or("Starting Python"),
                    self.progress["fits"].as_u64().unwrap_or(0)
                ));
            }
        });
        if !self.error.is_empty() {
            ui.colored_label(v::RED, &self.error);
        }
        if self.template.is_null() {
            return;
        }
        if self.studies.busy() {
            self.tab = 5;
        }
        match self.tab {
            0 => self.design(ui),
            1 => self.inference_view(ui),
            2 => self.validation_view(ui),
            3 => self.project_view(ui),
            4 => self.teach(ui),
            _ => self.simulation(ui),
        }
    }
    fn apply_missing_tokens(&mut self) {
        self.template["roles"]["missing_tokens"] = json!(self
            .missing_tokens_input
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>());
    }
    fn design(&mut self, ui: &mut egui::Ui) {
        let columns = self.template["dataset"]["columns"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect::<Vec<_>>();
        v::panel().show(ui,|ui|{
            ui.strong(self.template["dataset"]["name"].as_str().unwrap_or("Selected dataset"));
            ui.label(format!("{} observations · source {}",self.template["dataset"]["rows"].as_array().unwrap().len(),if self.pinned_source{"research snapshot"}else{"current workspace"}));
            let mut model=self.template["model"].as_str().unwrap_or("ols").to_owned();
            egui::ComboBox::from_id_salt("research_model").selected_text(&model).show_ui(ui,|ui|{for m in &MODELS{ui.selectable_value(&mut model,m.id.key().to_owned(),m.name);}});
            if self.template["model"]!=model{self.select_model(&model);}
            let mut target=self.template["roles"]["target"].as_str().unwrap_or("").to_owned();
            ui.horizontal(|ui|{ui.label("Response");egui::ComboBox::from_id_salt("research_target").selected_text(&target).show_ui(ui,|ui|{for c in &columns{ui.selectable_value(&mut target,c.clone(),c);}});});
            self.template["roles"]["target"]=json!(target);
            let mut selected=self.template["roles"]["features"].as_array().unwrap().iter().filter_map(Value::as_str).map(str::to_owned).collect::<Vec<_>>();selected.retain(|c|c!=&target);
            ui.label("Predictors (up to 64 input columns)");ui.horizontal_wrapped(|ui|{for c in &columns{if c==&target{continue;}let mut included=selected.contains(c);if ui.checkbox(&mut included,c).changed(){if included{selected.push(c.clone());}else{selected.retain(|n|n!=c);}}}});self.template["roles"]["features"]=json!(selected);
            let mut categorical=self.template["roles"]["categorical"].as_array().map(|v|v.iter().filter_map(Value::as_str).map(str::to_owned).collect::<Vec<_>>()).unwrap_or_default();categorical.retain(|c|selected.contains(c));
            ui.collapsing("Categorical declarations",|ui|{ui.label("Mark numeric-looking codes as categories when their numerical distances have no meaning.");for c in &selected{let mut cat=categorical.contains(c);if ui.checkbox(&mut cat,c).changed(){if cat{categorical.push(c.clone());}else{categorical.retain(|n|n!=c);}}}});self.template["roles"]["categorical"]=json!(categorical);
            for (role,label) in [("group","Group / cluster identity"),("time","Time ordering")]{let prior=self.template["roles"][role].clone();let mut value=self.template["roles"][role].as_str().unwrap_or("None").to_owned();ui.horizontal(|ui|{ui.label(label);egui::ComboBox::from_id_salt(("research_role",role)).selected_text(&value).show_ui(ui,|ui|{ui.selectable_value(&mut value,"None".into(),"None");for c in &columns{ui.selectable_value(&mut value,c.clone(),c);}});});self.template["roles"][role]=if value=="None"{Value::Null}else{json!(value)};if self.template["roles"][role]!=prior&&value!="None"{if let Some(features)=self.template["roles"]["features"].as_array_mut(){features.retain(|c|c!=&value);}}}
            let mut full=self.template.get("sample_rows").is_none();if ui.checkbox(&mut full,"Estimate on all eligible observations").changed(){if full{self.template.as_object_mut().unwrap().remove("sample_rows");}else if !self.workspace_rows.is_empty(){self.template["sample_rows"]=json!(self.workspace_rows);}else{self.error="This imported source has no linked native training partition; use the full eligible sample or define a resampling design.".into();}}
            ui.label(if full{"Inference and direct fits use the full eligible sample. Resampling separately creates its own development/test partition."}else{"Direct fitting and inference use the workspace's saved training rows. Resampling uses all eligible rows to construct its own design."});
            let mut omit=self.template["roles"]["omit_missing_outcome"].as_bool().unwrap_or(false);ui.checkbox(&mut omit,"Explicitly omit rows with missing outcomes and record their identities");self.template["roles"]["omit_missing_outcome"]=json!(omit);
            let mut scale=self.template["config"]["standardize"].as_bool().unwrap_or(true);ui.checkbox(&mut scale,"Standardize encoded predictors within each training sample");self.template["config"]["standardize"]=json!(scale);
            let mut degree=self.template["config"]["degree"].as_i64().unwrap_or(1);ui.horizontal(|ui|{ui.label("Numeric expansion");ui.selectable_value(&mut degree,1,"Linear terms");ui.selectable_value(&mut degree,2,"Squares and pairwise interactions");});self.template["config"]["degree"]=json!(degree);
            let mut formula=self.template["formula"].as_str().unwrap_or("").to_owned();
            ui.label("Formula (optional; predictor names must be selected above)");
            if ui.text_edit_singleline(&mut formula).changed(){self.template["inference"]["restriction"]=Value::Null;}
            self.template["formula"]=json!(formula);
            ui.small("Example: y ~ 1 + a + b^2 + a:b + C(group). Backticks quote column names. No Python code is evaluated.");
            ui.horizontal(|ui|{ui.label("Additional missing tokens");ui.text_edit_singleline(&mut self.missing_tokens_input);});
            self.apply_missing_tokens();
            let model=self.template["model"].as_str().unwrap_or("ols").to_owned();
            for (key,label,lo,hi) in [("alpha","Penalty",0.,1e8),("mix","L1 share",0.,1.),("rate","Learning rate",0.001,1.),("fraction","Training fraction per boosting step",0.05,1.)]{
                let show=match key{"alpha"=>["ridge","lasso","elastic_net"].contains(&model.as_str()),"mix"=>model=="elastic_net","fraction"=>model=="stochastic",_=>["boosting","stochastic"].contains(&model.as_str())};
                if show{let mut value=self.template["config"][key].as_f64().unwrap_or(if key=="fraction"{0.7}else{0.1});ui.horizontal(|ui|{ui.label(label);ui.add(egui::DragValue::new(&mut value).range(lo..=hi).speed(0.01));});self.template["config"][key]=json!(value);}
            }
            for(key,label,default,hi)in[("depth","Maximum tree depth",3,10),("min_leaf","Minimum leaf size",5,1000),("trees","Trees / boosting steps",50,300),("mtry","Features per forest split",1,64)]{
                if ["tree","boosting","stochastic","forest"].contains(&model.as_str())&&(key!="mtry"||model=="forest")&&(key!="trees"||model!="tree"){let mut value=self.template["config"][key].as_i64().unwrap_or(default);ui.horizontal(|ui|{ui.label(label);ui.add(egui::DragValue::new(&mut value).range(1..=hi));});self.template["config"][key]=json!(value);}
            }
            let mut seed=self.template["seed"].as_u64().unwrap_or(4710);ui.horizontal(|ui|{ui.label("Seed");ui.add(egui::DragValue::new(&mut seed).range(0..=1_000_000));});self.template["seed"]=json!(seed);
            ui.label("Numeric medians and categorical modes are learned inside training samples. Encoded categories use a training-defined reference level; held-out unknown levels are reported.");
        });
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    !self.busy(),
                    egui::Button::new("Fit research specification"),
                )
                .clicked()
            {
                self.start("fit");
            }
            if ui.button("Use current workspace data").clicked() {
                self.pinned_source = false;
                self.source_key.clear();
            }
            ui.label("Changing settings does not execute a fit or disclose a test result.");
        });
        ui.collapsing("Scientific interpreter and adapter",|ui|{ui.add(egui::TextEdit::singleline(&mut self.interpreter).desired_width(f32::INFINITY));ui.label(format!("Protocol {} · source {}",adapter::SCHEMA,&adapter::sha256(adapter::WORKER.as_bytes())[..12]));ui.label("The scientific backend exposes final fits and diagnostics; it does not expose internal optimizer replay. Use the native Explore/Optimization views for their actual teaching traces.");});
    }
    fn simulation(&mut self, ui: &mut egui::Ui) {
        let action = self.studies.ui(ui, &mut self.project_path);
        let result = match action {
            Some(crate::study_ui::Action::Save) => self.save(),
            Some(crate::study_ui::Action::Preview) => {
                self.studies.preview(self.interpreter.clone())
            }
            Some(crate::study_ui::Action::Run) => (|| {
                self.save()?;
                self.studies.add_study()?;
                self.save()?;
                self.studies.run_selected(false, self.interpreter.clone())
            })(),
            Some(crate::study_ui::Action::Resume) => {
                self.studies.run_selected(true, self.interpreter.clone())
            }
            Some(crate::study_ui::Action::Inspect) => {
                self.studies.inspect(self.interpreter.clone())
            }
            Some(crate::study_ui::Action::Export) => self.studies.export(self.interpreter.clone()),
            None => Ok(()),
        };
        if let Err(error) = result {
            self.studies.error = error;
        }
    }
    fn current(&self) -> Option<&Arc<Value>> {
        self.history.get(self.selected)
    }
    fn inference_view(&mut self, ui: &mut egui::Ui) {
        v::section(ui,"Inference specification","Covariance and reference distributions are explicit. Ordinary inference is available for unpenalized OLS and logistic regression.");
        let logistic = self.template["model"] == "logistic";
        let eligible = logistic || self.template["model"] == "ols";
        let mut cov = self.template["inference"]["covariance"]
            .as_str()
            .unwrap_or("classical")
            .to_owned();
        let choices = if logistic {
            vec!["classical", "HC0", "cluster"]
        } else {
            vec!["classical", "HC0", "HC1", "HC2", "HC3", "cluster", "HAC"]
        };
        ui.horizontal_wrapped(|ui| {
            ui.label("Covariance");
            egui::ComboBox::from_id_salt("covariance_choice")
                .selected_text(&cov)
                .show_ui(ui, |ui| {
                    for value in choices {
                        ui.selectable_value(&mut cov, value.to_owned(), value);
                    }
                });
        });
        self.template["inference"]["covariance"] = json!(cov);
        let mut confidence =
            100. * (1. - self.template["inference"]["alpha"].as_f64().unwrap_or(0.05));
        ui.horizontal(|ui| {
            ui.label("Confidence level (%)");
            ui.add(
                egui::DragValue::new(&mut confidence)
                    .range(50.0..=99.9)
                    .speed(0.1),
            );
        });
        self.template["inference"]["alpha"] = json!((1. - confidence / 100.).clamp(0.001, 0.5));
        if cov == "HAC" {
            let mut lags = self.template["inference"]["lags"].as_u64().unwrap_or(1);
            ui.horizontal(|ui| {
                ui.label("Bartlett HAC lag order (ordered observations)");
                ui.add(egui::DragValue::new(&mut lags).range(0..=100));
            });
            self.template["inference"]["lags"] = json!(lags);
        }
        if ui
            .add_enabled(
                eligible && !self.busy(),
                egui::Button::new("Estimate and compute inference"),
            )
            .clicked()
        {
            self.start("inference");
        }
        if !eligible {
            ui.colored_label(v::AMBER,"This model does not expose ordinary coefficient inference. Penalized estimates do not inherit OLS standard errors.");
        }
        let Some(analysis) = self.current().cloned() else {
            return;
        };
        let result = &analysis["response"]["result"];
        let Some(table) = result["coefficient_table"].as_array() else {
            ui.label(
                "Select a completed inference result in Project, or run the specification above.",
            );
            return;
        };
        result_identity(ui, &analysis);
        ui.label(format!(
            "n = {} · parameters = {} · {} covariance · {} reference · df {}",
            result["n"],
            result["parameter_count"],
            result["covariance_type"].as_str().unwrap_or(""),
            result["reference_distribution"].as_str().unwrap_or(""),
            result["inference_df"]
                .as_i64()
                .map(|v| v.to_string())
                .unwrap_or_else(|| "not applicable".into())
        ));
        egui::ScrollArea::horizontal()
            .id_salt("inference_table_scroll")
            .show(ui, |ui| {
                egui::Grid::new("inference_table")
                    .striped(true)
                    .min_col_width(80.)
                    .show(ui, |ui| {
                        for title in [
                            "Term",
                            "Estimate",
                            "Std. error",
                            "Statistic",
                            "p value",
                            "Lower",
                            "Upper",
                        ] {
                            ui.strong(title);
                        }
                        ui.end_row();
                        for row in table {
                            ui.label(row["term"].as_str().unwrap_or(""));
                            for key in [
                                "estimate",
                                "std_error",
                                "statistic",
                                "p_value",
                                "lower",
                                "upper",
                            ] {
                                ui.monospace(number(&row[key]));
                            }
                            ui.end_row();
                        }
                    });
            });
        coefficient_intervals(ui, table);
        if let Some(effects) = result["marginal_effects"].as_array() {
            v::section(
                ui,
                "Average continuous marginal effects",
                result["marginal_effect_scope"].as_str().unwrap_or(""),
            );
            for effect in effects {
                ui.label(format!(
                    "{}: {} · SE {} · interval [{}, {}]",
                    effect["term"].as_str().unwrap_or(""),
                    number(&effect["effect"]),
                    number(&effect["std_error"]),
                    number(&effect["lower"]),
                    number(&effect["upper"])
                ));
            }
        }
        if let Some(test) = result.get("joint_test") {
            ui.label(format!(
                "Joint {} = {} · p = {} · restrictions {}",
                test["distribution"].as_str().unwrap_or("test"),
                number(&test["statistic"]),
                number(&test["p_value"]),
                test["restrictions"]
            ));
        }
        ui.collapsing("Joint restrictions for the next inference run",|ui|{
            let aligned=self.template.get("formula")==analysis["request"].get("formula")&&self.template["roles"]==analysis["request"]["roles"]&&self.template["dataset"]==analysis["request"]["dataset"]&&self.template["model"]==analysis["request"]["model"]&&self.template["config"]==analysis["request"]["config"]&&self.template.get("sample_rows")==analysis["request"].get("sample_rows");
            if !aligned{ui.label("Restore the same specification and sample before defining restrictions from this table.");return;}
            self.restriction_terms.resize(table.len(),false);ui.label("Choose coefficients and a common null value. Each checked term contributes one row to R beta = q.");
            for(j,row)in table.iter().enumerate(){ui.checkbox(&mut self.restriction_terms[j],row["term"].as_str().unwrap_or(""));}
            ui.add(egui::TextEdit::singleline(&mut self.restriction_values).hint_text("Null value, e.g. 0"));
            if ui.button("Set restrictions").clicked(){match self.restriction_values.parse::<f64>(){Ok(value)if value.is_finite()=>{let indices=self.restriction_terms.iter().enumerate().filter_map(|(j,&v)|v.then_some(j)).collect::<Vec<_>>();let restriction_matrix=indices.iter().map(|&j|(0..table.len()).map(|k|if k==j{1.}else{0.}).collect::<Vec<_>>()).collect::<Vec<_>>();self.template["inference"]["restriction"]=if indices.is_empty(){Value::Null}else{json!({"R":restriction_matrix,"q":vec![value;indices.len()]})};},_=>self.error="Enter a finite null value.".into()}}
        });
        for line in result["assumptions"].as_array().unwrap_or(&vec![]) {
            ui.label(
                RichText::new(line.as_str().unwrap_or(""))
                    .size(12.)
                    .color(v::MUTED),
            );
        }
    }
    fn validation_view(&mut self, ui: &mut egui::Ui) {
        v::section(ui,"Resampling and model selection","A final test partition is reserved before tuning. Inner tuning stays within each outer training fold.");
        let mut kind = self.template["resampling"]["kind"]
            .as_str()
            .unwrap_or("random")
            .to_owned();
        ui.horizontal_wrapped(|ui| {
            for key in ["random", "stratified", "group", "temporal"] {
                ui.selectable_value(&mut kind, key.to_owned(), key);
            }
        });
        self.template["resampling"]["kind"] = json!(kind);
        ui.horizontal_wrapped(|ui| {
            for (key, label, lo, hi) in [
                ("folds", "Outer folds", 2, 10),
                ("inner_folds", "Inner folds", 2, 10),
                ("repeats", "Repeats", 1, 5),
                ("gap", "Time gap (blocks)", 0, 50),
            ] {
                let mut value = self.template["resampling"][key].as_u64().unwrap_or(lo);
                ui.label(label);
                ui.add(egui::DragValue::new(&mut value).range(lo..=hi));
                self.template["resampling"][key] = json!(value);
            }
        });
        let mut fraction = self.template["resampling"]["test_fraction"]
            .as_f64()
            .unwrap_or(0.2);
        ui.horizontal(|ui| {
            ui.label("Final test fraction");
            ui.add(egui::Slider::new(&mut fraction, 0.1..=0.4));
        });
        self.template["resampling"]["test_fraction"] = json!(fraction);
        ui.horizontal(|ui| {
            ui.label("Candidate penalties");
            ui.add(egui::TextEdit::singleline(&mut self.alpha_grid).desired_width(300.));
        });
        ui.label("The grid applies to ridge, lasso and elastic net. Other model settings remain fixed and explicitly recorded.");
        let disclosed = self
            .disclosures
            .iter()
            .filter(|v| v["dataset_key"] == data_key(&self.template))
            .count();
        if disclosed > 0 {
            ui.colored_label(v::AMBER,format!("This dataset has {disclosed} recorded test-evaluation request(s) in this project. Later evaluations are not fresh validation."));
            ui.checkbox(
                &mut self.test_acknowledged,
                "Record another test evaluation with this prior disclosure visible",
            );
        }
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    !self.busy(),
                    egui::Button::new("Run resampling; keep test unscored"),
                )
                .clicked()
            {
                self.start("resample");
            }
            if ui
                .add_enabled(
                    !self.busy() && (disclosed == 0 || self.test_acknowledged),
                    egui::Button::new("Evaluate reserved test (record disclosure)"),
                )
                .clicked()
            {
                self.start("evaluate_test");
            }
        });
        let Some(analysis) = self.current().cloned() else {
            return;
        };
        let result = &analysis["response"]["result"];
        let Some(folds) = result["folds"].as_array() else {
            ui.label("Select a completed resampling result in Project, or run the design above.");
            return;
        };
        result_identity(ui, &analysis);
        v::panel().show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                v::metric(
                    ui,
                    "Weighted outer loss",
                    &number(&result["cv_loss"]),
                    v::TEAL,
                );
                v::metric(
                    ui,
                    "Fold-loss SD",
                    &number(&result["fold_loss_sd"]),
                    v::BLUE,
                );
                v::metric(
                    ui,
                    "Final test loss",
                    &if result["test_loss"].is_null() {
                        "Not scored".into()
                    } else {
                        number(&result["test_loss"])
                    },
                    v::AMBER,
                );
            });
        });
        ui.label(
            result["fold_variation_interpretation"]
                .as_str()
                .unwrap_or(""),
        );
        let lines = [Series::line(
            "Outer held-out loss",
            folds
                .iter()
                .enumerate()
                .map(|(i, f)| (i as f64 + 1., f["validation_loss"].as_f64().unwrap_or(0.)))
                .collect(),
            v::TEAL,
        )];
        (Plot {
            id: "research_cv_loss",
            x_label: "Completed outer fold",
            y_label: if analysis["request"]["model"] == "logistic" {
                "Log loss"
            } else {
                "Mean squared error"
            },
            height: 220.,
            series: &lines,
            selected: None,
            vertical: None,
            y_range: None,
        })
        .show(ui);
        fold_map(
            ui,
            result,
            analysis["request"]["dataset"]["rows"]
                .as_array()
                .map_or(0, Vec::len),
        );
        ui.label(format!(
            "Final selected settings: {}",
            result["selected_config"]
        ));
        ui.collapsing("Fold records and preprocessing audit", |ui| {
            for fold in folds {
                ui.collapsing(
                    format!(
                        "Repeat {} / fold {} · loss {}",
                        fold["repeat"],
                        fold["fold"],
                        number(&fold["validation_loss"])
                    ),
                    |ui| {
                        ui.label(format!(
                            "Training observations: {}",
                            fold["training_rows"].as_array().map_or(0, Vec::len)
                        ));
                        ui.label(format!(
                            "Validation observations: {}",
                            fold["validation_rows"].as_array().map_or(0, Vec::len)
                        ));
                        ui.label(format!(
                            "Training-defined means: {}",
                            fold["preprocessing"]["design_mean"]
                        ));
                        ui.label(format!(
                            "Training-defined scales: {}",
                            fold["preprocessing"]["design_scale"]
                        ));
                        ui.label(format!("Chosen settings: {}", fold["selected_config"]));
                    },
                );
            }
        });
    }
    fn project_view(&mut self, ui: &mut egui::Ui) {
        v::section(ui,"Research project","Save specifications, exact results, comparison history, test disclosures and teaching responses together.");
        ui.add(
            egui::TextEdit::singleline(&mut self.project_path)
                .hint_text("Absolute path to project.json")
                .desired_width(f32::INFINITY),
        );
        ui.horizontal_wrapped(|ui| {
            if ui.button("Save project").clicked() {
                if let Err(e) = self.save() {
                    self.error = e;
                }
            }
            if ui
                .add_enabled(!self.busy(), egui::Button::new("Open project"))
                .clicked()
            {
                if let Err(e) = self.load() {
                    self.error = e;
                }
            }
        });
        ui.horizontal_wrapped(|ui|{
            if ui.add_enabled(!self.busy(),egui::Button::new("Start new project")).clicked(){let previous=json!({"project":self.to_json(),"project_path":self.project_path});let interpreter=self.interpreter.clone();*self=Self::default();self.interpreter=interpreter;self.previous_project=Some(previous);self.status="New project started. Previous unsaved state can be restored with Undo new project.".into();}
            if ui.add_enabled(self.previous_project.is_some(),egui::Button::new("Undo new project")).clicked(){if let Some(value)=self.previous_project.take(){self.project_path=value["project_path"].as_str().unwrap_or("").into();if let Err(e)=self.restore(value["project"].clone()){self.error=e;}}}
        });
        ui.label("Annotations");
        ui.add(
            egui::TextEdit::multiline(&mut self.notes)
                .desired_rows(3)
                .desired_width(f32::INFINITY),
        );
        ui.add_space(8.);
        for (i, entry) in self.history.iter().enumerate() {
            let r = &entry["response"]["result"];
            let label = format!(
                "{} · {} · {} · {}",
                i + 1,
                entry["request"]["model"].as_str().unwrap_or(""),
                entry["request"]["operation"].as_str().unwrap_or(""),
                entry["request"]["dataset"]["name"]
                    .as_str()
                    .unwrap_or("Dataset")
            );
            if ui.selectable_label(self.selected == i, label).clicked() {
                self.selected = i;
            }
            if let Some(loss) = r["cv_loss"].as_f64() {
                ui.label(format!(
                    "Outer loss {} · final test {}",
                    v::number(loss),
                    if r["test_loss"].is_null() {
                        "not scored".into()
                    } else {
                        number(&r["test_loss"])
                    }
                ));
            }
        }
        let Some(analysis) = self.current().cloned() else {
            ui.label("Completed scientific operations appear here. Stored results are distinct from changed settings.");
            return;
        };
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(!self.busy(), egui::Button::new("Reproduce selected result"))
                .clicked()
            {
                self.compare = Some(analysis["response"].clone());
                if let Err(e) = self.launch(analysis["request"].clone()) {
                    self.compare = None;
                    self.error = e;
                }
            }
            if ui.button("Teach this result").clicked() {
                self.tab = 4;
            }
        });
        ui.add(
            egui::TextEdit::singleline(&mut self.export_path)
                .hint_text("Absolute path to a new export folder")
                .desired_width(f32::INFINITY),
        );
        if ui
            .add_enabled(
                !self.busy(),
                egui::Button::new("Export reproducible research and teaching bundle"),
            )
            .clicked()
        {
            let mut request = analysis["request"].clone();
            request["operation"] = json!("export");
            request["source_request"] = analysis["request"].clone();
            request["source_response"] = analysis["response"].clone();
            request["source_wire"] =
                json!(serde_json::to_string_pretty(&analysis["request"]).unwrap());
            request["export_path"] = json!(self.export_path.trim());
            if let Err(e) = self.launch(request) {
                self.error = e;
            }
        }
        ui.label("Exports contain selected data, exact requests/results, executable reproduction, coefficient/fold tables, PDF/SVG figures, LaTeX report and separate student/instructor materials.");
        if !self.disclosures.is_empty() {
            ui.collapsing("Test disclosure history", |ui| {
                for d in &self.disclosures {
                    ui.label(format!(
                        "{} · {} · {}",
                        d["request_id"].as_str().unwrap_or(""),
                        d["model"].as_str().unwrap_or(""),
                        d["status"].as_str().unwrap_or("")
                    ));
                }
            });
        }
        if self.history.len() > 1 {
            ui.collapsing("Matched comparison audit", |ui| {
                let key = data_key(&analysis["request"]);
                for entry in &self.history {
                    let same_data = data_key(&entry["request"]) == key;
                    let a = &analysis["response"]["result"];
                    let b = &entry["response"]["result"];
                    let fold_rows = |value: &Value| {
                        value["folds"].as_array().map(|folds| {
                            folds
                                .iter()
                                .map(|fold| {
                                    json!([
                                        fold["repeat"],
                                        fold["fold"],
                                        fold["training_rows"],
                                        fold["validation_rows"]
                                    ])
                                })
                                .collect::<Vec<_>>()
                        })
                    };
                    let matched = same_data
                        && (analysis["request"]["model"] == "logistic")
                            == (entry["request"]["model"] == "logistic")
                        && fold_rows(a) == fold_rows(b)
                        && a.get("training_rows") == b.get("training_rows")
                        && a.get("development_rows") == b.get("development_rows")
                        && a.get("test_rows") == b.get("test_rows")
                        && analysis["request"]["roles"] == entry["request"]["roles"]
                        && analysis["request"]["operation"] == entry["request"]["operation"];
                    ui.label(format!(
                        "{} / {}: {}",
                        entry["request"]["model"].as_str().unwrap_or(""),
                        entry["request"]["operation"].as_str().unwrap_or(""),
                        if matched {
                            "matching data, roles, operation and sample partitions"
                        } else {
                            "not a matched comparison"
                        }
                    ));
                }
            });
        }
    }
    pub fn save(&mut self) -> Result<(), String> {
        if self.project_path.trim().is_empty() {
            return Err("Choose a project path.".into());
        }
        self.studies
            .prepare_save(std::path::Path::new(self.project_path.trim()))?;
        let value = self.to_json();
        let bytes = serde_json::to_vec_pretty(&value).map_err(|e| e.to_string())?;
        if bytes.len() > 128 * 1024 * 1024 {
            return Err("Project exceeds 128 MiB. Export selected results individually.".into());
        }
        let path = std::path::PathBuf::from(self.project_path.trim());
        let temporary = path.with_extension("json.partial");
        std::fs::write(&temporary, bytes).map_err(|e| e.to_string())?;
        std::fs::rename(&temporary, &path).map_err(|e| e.to_string())?;
        self.status = format!("Saved research project: {}", path.display());
        Ok(())
    }
    pub fn to_json(&self) -> Value {
        json!({"schema_version":2,"simulation":self.studies.to_json(),"engine_version":env!("CARGO_PKG_VERSION"),"template":self.template,"history":self.history,"test_disclosures":self.disclosures,"selected":self.selected,"notes":self.notes,"teaching_answers":self.teaching_answers,"teaching_revealed":self.teaching_revealed,"alpha_grid":self.alpha_grid,"workspace_rows":self.workspace_rows})
    }
    pub fn restore(&mut self, value: Value) -> Result<(), String> {
        if self.busy() {
            return Err(
                "Cancel or complete active operations before restoring another project.".into(),
            );
        }
        if value["schema_version"] != 1 && value["schema_version"] != 2 {
            return Err("Unsupported research project schema.".into());
        }
        let template = value["template"].clone();
        crate::adapter::ScientificAdapter.validate(&template)?;
        let history = value["history"]
            .as_array()
            .ok_or("Missing project history")?
            .clone();
        if history.len() > 12 {
            return Err("Project history exceeds 12 completed analyses.".into());
        }
        for entry in &history {
            let request = &entry["request"];
            crate::adapter::ScientificAdapter.validate(request)?;
            if !["fit", "inference", "resample", "evaluate_test"]
                .contains(&request["operation"].as_str().unwrap_or(""))
            {
                return Err("Saved history contains a non-analysis operation.".into());
            }
            crate::adapter::validate_payload(request, &entry["response"])?;
            let wire = serde_json::to_vec_pretty(request).map_err(|e| e.to_string())?;
            if entry["response"]["request_sha256"].as_str() != Some(&adapter::sha256(&wire))
                || entry["response"]["request_id"] != request["request_id"]
                || entry["response"]["status"] != "completed"
                || entry["result_sha256"].as_str()
                    != Some(&adapter::sha256(
                        &serde_json::to_vec(&entry["response"]["result"]).unwrap(),
                    ))
            {
                return Err("Saved analysis identity/hash validation failed.".into());
            }
        }
        self.workspace_rows =
            serde_json::from_value(value["workspace_rows"].clone()).unwrap_or_default();
        self.template = template;
        self.missing_tokens_input = self.template["roles"]["missing_tokens"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
        self.history = history.into_iter().map(Arc::new).collect();
        self.studies
            .restore(&value["simulation"], &self.project_path)?;
        self.disclosures = value["test_disclosures"]
            .as_array()
            .ok_or("Missing test disclosure history")?
            .clone();
        self.selected = value["selected"].as_u64().unwrap_or(0) as usize;
        self.selected = self.selected.min(self.history.len().saturating_sub(1));
        self.notes = value["notes"].as_str().unwrap_or("").into();
        self.alpha_grid = value["alpha_grid"]
            .as_str()
            .unwrap_or("0.001, 0.01, 0.1, 1")
            .into();
        self.teaching_answers = serde_json::from_value(value["teaching_answers"].clone())
            .unwrap_or_else(|_| vec![String::new(); 3]);
        self.teaching_answers.resize(3, String::new());
        self.teaching_answers.truncate(3);
        self.teaching_revealed = value["teaching_revealed"].as_bool().unwrap_or(false);
        self.pinned_source = true;
        self.error.clear();
        self.status="Research project restored; source pinned to its saved specification. No code was executed.".into();
        Ok(())
    }
    fn load(&mut self) -> Result<(), String> {
        let path = std::path::PathBuf::from(self.project_path.trim());
        if std::fs::metadata(&path).map_err(|e| e.to_string())?.len() > 128 * 1024 * 1024 {
            return Err("Project exceeds 128 MiB.".into());
        }
        let value = serde_json::from_slice(&std::fs::read(&path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        self.restore(value)
    }
    fn teach(&mut self, ui: &mut egui::Ui) {
        let Some(analysis) = self.current().cloned() else {
            ui.label("Complete or select a scientific analysis to teach its actual results.");
            return;
        };
        let result = &analysis["response"]["result"];
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut self.presentation, "Presentation text");
            ui.checkbox(&mut self.teaching_revealed, "Show instructor explanations");
        });
        result_identity(ui, &analysis);
        let size = if self.presentation { 21. } else { 15. };
        let prompts: Vec<(String, String)> = if let Some(table) =
            result["coefficient_table"].as_array()
        {
            let row = &table[1];
            vec![(format!("{} has estimate {} and interval [{}, {}]. Interpret the parameter and its uncertainty.",row["term"].as_str().unwrap_or("Coefficient"),number(&row["estimate"]),number(&row["lower"]),number(&row["upper"])),format!("The table uses {} covariance and the {} reference. Its validity requires the displayed sampling and specification assumptions; a confidence procedure is not a posterior probability for a fixed parameter.",result["covariance_type"].as_str().unwrap_or(""),result["reference_distribution"].as_str().unwrap_or(""))),
            ("What changes when clustering is substantively appropriate?".into(),"Cluster scores aggregate dependent observations. Independence is required across the relevant clusters, and few clusters can make conventional approximations unreliable. Changing a menu choice does not justify the dependence assumption.".into()),
            ("Does this uncertainty account for specification searching or establish a causal effect?".into(),"No. The calculation conditions on the specified design and preprocessing. Selection uncertainty and causal identification require separate arguments and procedures.".into())]
        } else if let Some(folds) = result["folds"].as_array() {
            let f = &folds[0];
            vec![(format!("The first outer fold uses {} training and {} validation rows. Which observations determine its means and imputations?",f["training_rows"].as_array().unwrap().len(),f["validation_rows"].as_array().unwrap().len()),"Only that fold's training observations. Inner-fold preprocessing is refitted again using each inner training sample.".into()),
            (format!("Weighted outer loss is {}. What does this quantity evaluate?",number(&result["cv_loss"])),"It evaluates the specified fitting and tuning procedure on outer held-out data. It does not turn the descriptive fold SD into a valid uncertainty interval.".into()),
            (format!("Final test disclosed: {}. What changes after its result has been inspected?",result["test_revealed"]),"Subsequent tuning can use information learned from the test. Repeating its evaluation does not create a fresh independent assessment; the project records disclosures, including uncertain attempts.".into())]
        } else {
            vec![("Which observations determined this model?".into(),"Inspect the exact training_rows and preprocessing records. A final backend fit does not expose an internal optimizer trace.".into()),("Does a small training loss establish generalization?".into(),"No. Use an appropriate held-out or resampling design for the target population and dependence structure.".into())]
        };
        for (i, (prompt, answer)) in prompts.iter().enumerate() {
            v::panel().show(ui, |ui| {
                ui.label(
                    RichText::new(format!("{}. {}", i + 1, prompt))
                        .size(size)
                        .strong(),
                );
                ui.add(
                    egui::TextEdit::multiline(&mut self.teaching_answers[i])
                        .hint_text("Learner explanation")
                        .desired_rows(2)
                        .desired_width(f32::INFINITY),
                );
                if self.teaching_revealed {
                    ui.label(RichText::new(answer).size(size - 1.).color(v::TEAL));
                }
            });
            ui.add_space(8.);
        }
        if result.get("folds").is_some() {
            fold_map(
                ui,
                result,
                analysis["request"]["dataset"]["rows"]
                    .as_array()
                    .unwrap()
                    .len(),
            );
        }
        if let Some(table) = result["coefficient_table"].as_array() {
            coefficient_intervals(ui, table);
        }
        ui.label("The saved project retains learner responses. Export creates a student worksheet and a separate instructor key.");
    }
}

fn data_key(request: &Value) -> String {
    adapter::sha256(&serde_json::to_vec(&request["dataset"]).unwrap())
}
fn number(v: &Value) -> String {
    v.as_f64()
        .map(v::number)
        .unwrap_or_else(|| "Not available".into())
}
fn result_identity(ui: &mut egui::Ui, analysis: &Value) {
    let request = &analysis["request"];
    ui.add_space(10.);
    ui.label(
        RichText::new(format!(
            "Recorded result: {} · {} · {}",
            request["model"].as_str().unwrap_or(""),
            request["operation"].as_str().unwrap_or(""),
            request["dataset"]["name"].as_str().unwrap_or("Dataset")
        ))
        .strong(),
    );
    ui.label(
        RichText::new(format!(
            "Request {} · {} backend · {}",
            request["request_id"].as_str().unwrap_or(""),
            analysis["response"]["backend"].as_str().unwrap_or(""),
            analysis["response"]["engine_version"]
                .as_str()
                .unwrap_or("")
        ))
        .size(10.)
        .color(v::MUTED),
    );
    if let Some(warnings) = analysis["response"]["warnings"].as_array() {
        for warning in warnings {
            ui.colored_label(v::AMBER, warning.as_str().unwrap_or(""));
        }
    }
    if let Some(profile) = analysis["response"].get("data_profile") {
        ui.collapsing("Estimation sample and data quality", |ui| {
            ui.label(format!(
                "Input observations {} · eligible outcomes {} · omitted row identities {}",
                profile["input_rows"], profile["eligible_outcomes"], profile["omitted_rows"]
            ));
            if let Some(columns) = profile["columns"].as_array() {
                for column in columns {
                    ui.label(format!(
                        "{} · {} · {} missing · {} distinct",
                        column["name"].as_str().unwrap_or(""),
                        column["role"].as_str().unwrap_or(""),
                        column["missing"],
                        column["distinct_nonmissing"]
                    ));
                }
            }
        });
    }
    if let Some(reproduction) = analysis.get("reproduction").filter(|v| !v.is_null()) {
        ui.label(format!(
            "Numerical reproduction: {} · worker source changed: {} · version drift: {}",
            reproduction["numerical_match"],
            reproduction["worker_changed"],
            reproduction["version_drift"]
        ));
    }
}
fn coefficient_intervals(ui: &mut egui::Ui, rows: &[Value]) {
    v::section(
        ui,
        "Coefficient intervals",
        "Intervals concern the specified estimator, covariance and sampling assumptions.",
    );
    let width = ui.available_width().max(200.);
    if rows.len() > 60 {
        ui.label("Showing the first 60 coefficients; exports retain the complete table.");
    }
    let height = 45. + rows.len().min(60) as f32 * 26.;
    let (response, p) = ui.allocate_painter(egui::vec2(width, height), egui::Sense::hover());
    let rect = response.rect;
    let left = rect.left() + 140.;
    let right = rect.right() - 60.;
    let lo = rows
        .iter()
        .filter_map(|r| r["lower"].as_f64())
        .fold(0., f64::min);
    let hi = rows
        .iter()
        .filter_map(|r| r["upper"].as_f64())
        .fold(0., f64::max);
    let range = (hi - lo).max(1e-12);
    let x = |v: f64| left + ((v - lo) / range) as f32 * (right - left);
    p.line_segment(
        [
            egui::pos2(x(0.), rect.top()),
            egui::pos2(x(0.), rect.bottom() - 25.),
        ],
        egui::Stroke::new(1., v::MUTED),
    );
    for (i, row) in rows.iter().take(60).enumerate() {
        let y = rect.top() + 15. + i as f32 * 26.;
        p.text(
            egui::pos2(rect.left(), y),
            egui::Align2::LEFT_CENTER,
            row["term"].as_str().unwrap_or(""),
            egui::FontId::proportional(11.),
            v::INK,
        );
        let a = row["lower"].as_f64().unwrap_or(0.);
        let b = row["upper"].as_f64().unwrap_or(0.);
        let point = row["estimate"].as_f64().unwrap_or(0.);
        p.line_segment(
            [egui::pos2(x(a), y), egui::pos2(x(b), y)],
            egui::Stroke::new(2., v::TEAL),
        );
        p.circle_filled(egui::pos2(x(point), y), 3.5, v::TEAL);
    }
    p.text(
        egui::pos2(left, rect.bottom()),
        egui::Align2::LEFT_BOTTOM,
        v::number(lo),
        egui::FontId::proportional(11.),
        v::MUTED,
    );
    p.text(
        egui::pos2(right, rect.bottom()),
        egui::Align2::RIGHT_BOTTOM,
        v::number(hi),
        egui::FontId::proportional(11.),
        v::MUTED,
    );
}
fn fold_map(ui: &mut egui::Ui, result: &Value, n: usize) {
    let Some(folds) = result["folds"].as_array() else {
        return;
    };
    v::section(ui,"Observation identities across folds","Blue: training. Amber: validation. Red: reserved test. Gray: unused or gap. Positions retain original row order. Above 400 rows, each tile stacks exact role proportions for consecutive rows.");
    let height = 30. + folds.len().min(50) as f32 * 21.;
    let (response, p) = ui.allocate_painter(
        egui::vec2(ui.available_width().max(200.), height),
        egui::Sense::hover(),
    );
    let rect = response.rect;
    let left = rect.left() + 80.;
    let w = (rect.right() - left) / n.max(1) as f32;
    let test = result["test_rows"].as_array().unwrap();
    for (k, fold) in folds.iter().take(50).enumerate() {
        let y = rect.top() + k as f32 * 21.;
        p.text(
            egui::pos2(rect.left(), y + 8.),
            egui::Align2::LEFT_CENTER,
            format!("{} / {}", fold["repeat"], fold["fold"]),
            egui::FontId::proportional(10.),
            v::MUTED,
        );
        let train = fold["training_rows"].as_array().unwrap();
        let valid = fold["validation_rows"].as_array().unwrap();
        let mut roles = vec![0u8; n];
        for (rows, role) in [(train, 1u8), (valid, 2u8), (test, 3u8)] {
            for value in rows {
                if let Some(i) = value.as_u64() {
                    if let Some(slot) = roles.get_mut(i as usize) {
                        *slot = role;
                    }
                }
            }
        }
        let bin_size = n.div_ceil(400).max(1);
        for (bin, chunk) in roles.chunks(bin_size).enumerate() {
            let mut counts = [0usize; 4];
            for role in chunk {
                counts[*role as usize] += 1;
            }
            let mut top = y;
            for (role, count) in counts.iter().enumerate() {
                if *count == 0 {
                    continue;
                }
                let h = 15. * *count as f32 / chunk.len() as f32;
                let color = match role {
                    3 => v::RED,
                    2 => v::AMBER,
                    1 => v::BLUE,
                    _ => egui::Color32::from_gray(55),
                };
                p.rect_filled(
                    egui::Rect::from_min_size(
                        egui::pos2(left + (bin * bin_size) as f32 * w, top),
                        egui::vec2(chunk.len() as f32 * w, h),
                    ),
                    0.,
                    color,
                );
                top += h;
            }
        }
    }
}
pub fn numerical_match(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(a), Value::Number(b)) => {
            let x = a.as_f64().unwrap();
            let y = b.as_f64().unwrap();
            x.is_finite() && y.is_finite() && (x - y).abs() <= 1e-7 * (1. + x.abs())
        }
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| numerical_match(a, b))
        }
        (Value::Object(a), Value::Object(b)) => {
            a.len() == b.len()
                && a.iter()
                    .all(|(k, v)| b.get(k).is_some_and(|other| numerical_match(v, other)))
        }
        _ => a == b,
    }
}

#[cfg(test)]
mod editing_tests {
    use super::*;
    #[test]
    fn missing_token_editing_retains_separator_between_frames() {
        let mut state = Research::default();
        state.template = json!({"roles":{}});
        state.missing_tokens_input = "NA,".into();
        state.apply_missing_tokens();
        assert_eq!(state.missing_tokens_input, "NA,");
        state.missing_tokens_input.push_str(" NULL");
        state.apply_missing_tokens();
        assert_eq!(
            state.template["roles"]["missing_tokens"],
            json!(["NA", "NULL"])
        );
    }
}
