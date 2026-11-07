//! Native simulation laboratories; immutable scientific records, separate learning state.
use crate::{
    adapter::{self, Event},
    study::*,
    visuals::{self as v, Plot, Series},
};
use eframe::egui::{self, RichText};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::Instant,
};

#[derive(Clone, Debug)]
pub enum Action {
    Preview,
    Run,
    Resume,
    Inspect,
    Export,
    Save,
}
pub struct StudyPanel {
    pub draft: StudySpec,
    pub entries: Vec<StudyRef>,
    pub selected: usize,
    pub stage: usize,
    pub documents: BTreeMap<String, Arc<StudyDocument>>,
    pub project_file: PathBuf,
    pub status: String,
    pub error: String,
    pub sample: Option<Arc<Value>>,
    pub inspection: Option<Arc<Value>>,
    pub replicate: usize,
    pub method_id: String,
    pub audience: String,
    pub export_path: String,
    pub reveal: bool,
    pub presentation: bool,
    pub draft_prediction: String,
    session: Option<adapter::Session>,
    pending: Option<Value>,
    started: Option<Instant>,
    pub cancel_ack_ms: Option<f64>,
    pub cancel_exit_ms: Option<f64>,
    shutdown: Option<(Arc<std::sync::atomic::AtomicBool>, Instant)>,
    pub frame_ms: Vec<f64>,
}
impl Default for StudyPanel {
    fn default() -> Self {
        Self {
            draft: StudySpec::default(),
            entries: vec![],
            selected: 0,
            stage: 0,
            documents: BTreeMap::new(),
            project_file: PathBuf::new(),
            status: "Choose a generating mechanism, specify the study and inspect a sample.".into(),
            error: String::new(),
            sample: None,
            inspection: None,
            replicate: 0,
            method_id: String::new(),
            audience: "student".into(),
            export_path: String::new(),
            reveal: false,
            presentation: false,
            draft_prediction: String::new(),
            session: None,
            pending: None,
            started: None,
            cancel_ack_ms: None,
            cancel_exit_ms: None,
            shutdown: None,
            frame_ms: vec![],
        }
    }
}
impl StudyPanel {
    pub fn busy(&self) -> bool {
        self.session.is_some() || self.shutdown.is_some()
    }
    pub fn current(&self) -> Option<&StudyRef> {
        self.entries.get(self.selected)
    }
    pub fn current_document(&self) -> Option<&Arc<StudyDocument>> {
        self.current().and_then(|r| self.documents.get(&r.id))
    }
    pub fn prepare_save(&mut self, path: &Path) -> Result<(), String> {
        if !path.is_absolute() || !path.parent().is_some_and(Path::is_dir) {
            return Err("Choose an absolute project path in an existing folder.".into());
        }
        if self.project_file.as_os_str().is_empty() && path.exists() {
            return Err(
                "Open an existing project before overwriting it, or choose a new filename.".into(),
            );
        }
        if self.project_file != path && !self.project_file.as_os_str().is_empty() {
            if self.busy() {
                return Err("Cancel or complete the running study before Save As.".into());
            }
            if path.exists() {
                return Err("Save As preserves existing work; choose a new filename.".into());
            }
            let store = format!(
                "{}.studies",
                path.file_stem()
                    .ok_or("Missing project name")?
                    .to_string_lossy()
            );
            let mut entries = self.entries.clone();
            for (old, new) in self.entries.iter().zip(entries.iter_mut()) {
                new.relative_path = format!("{store}/{}.json", new.id);
                let source = old.path(&self.project_file)?;
                let destination = new.path(path)?;
                if source.exists() {
                    old.load(&self.project_file)?;
                    std::fs::create_dir_all(destination.parent().unwrap())
                        .map_err(|e| e.to_string())?;
                    let bytes = std::fs::read(&source).map_err(|e| e.to_string())?;
                    if destination.exists() {
                        if std::fs::read(&destination).map_err(|e| e.to_string())? != bytes {
                            return Err(
                                "Save As would overwrite a different study artifact.".into()
                            );
                        }
                    } else {
                        use std::io::Write;
                        let mut f = std::fs::OpenOptions::new()
                            .write(true)
                            .create_new(true)
                            .open(destination)
                            .map_err(|e| e.to_string())?;
                        f.write_all(&bytes).map_err(|e| e.to_string())?;
                    }
                }
            }
            self.entries = entries;
        }
        self.project_file = path.to_owned();
        Ok(())
    }
    pub fn to_json(&self) -> Value {
        json!({"draft":self.draft,"studies":self.entries,"selected":self.selected,"stage":self.stage,"draft_prediction":self.draft_prediction})
    }
    pub fn restore(&mut self, value: &Value, project: &str) -> Result<(), String> {
        let mut restored = Self::default();
        if let Some(draft) = value.get("draft") {
            restored.draft = serde_json::from_value(draft.clone()).map_err(|e| e.to_string())?;
        }
        if let Some(entries) = value.get("studies") {
            restored.entries =
                serde_json::from_value(entries.clone()).map_err(|e| e.to_string())?;
        }
        if restored.entries.len() > 12 {
            return Err("Projects retain at most 12 simulation studies.".into());
        }
        let path = PathBuf::from(project.trim());
        let mut size = 0u64;
        let mut ids = std::collections::BTreeSet::new();
        for entry in &restored.entries {
            entry.spec.validate()?;
            if serde_json::from_str::<Value>(&entry.spec_wire).map_err(|e| e.to_string())?
                != serde_json::to_value(&entry.spec).unwrap()
                || !ids.insert(&entry.id)
            {
                return Err("Invalid or duplicate saved study specification.".into());
            }
            let checkpoint = entry.path(&path)?;
            if checkpoint.exists() {
                size += std::fs::metadata(&checkpoint)
                    .map_err(|e| e.to_string())?
                    .len();
                if size > 128 * 1024 * 1024 {
                    return Err("Study artifacts exceed the 128 MiB project allowance.".into());
                }
                let doc = entry.load(&path)?;
                restored.documents.insert(entry.id.clone(), Arc::new(doc));
            }
        }
        restored.project_file = path;
        restored.selected = value["selected"].as_u64().unwrap_or(0) as usize;
        restored.selected = restored
            .selected
            .min(restored.entries.len().saturating_sub(1));
        restored.method_id = restored
            .current()
            .map(|e| e.spec.methods[0].id.clone())
            .unwrap_or_default();
        restored.stage = (value["stage"].as_u64().unwrap_or(0) as usize).min(4);
        restored.draft_prediction = value["draft_prediction"].as_str().unwrap_or("").into();
        restored.status="Simulation project restored. No embedded source was executed. Keep the associated .studies folder with the project.".into();
        *self = restored;
        Ok(())
    }
    pub fn add_study(&mut self) -> Result<(), String> {
        if self.busy() {
            return Err("A study operation is already running.".into());
        }
        if self.entries.len() >= 12 {
            return Err("This project holds 12 studies. Save it and start a new project.".into());
        }
        self.draft.validate()?;
        let store = format!(
            "{}.studies",
            self.project_file
                .file_stem()
                .ok_or("Save the project before running a study")?
                .to_string_lossy()
        );
        let mut entry = StudyRef::new(self.draft.clone(), &store)?;
        entry.answers[0] = self.draft_prediction.clone();
        self.entries.push(entry);
        self.selected = self.entries.len() - 1;
        self.inspection = None;
        self.method_id = self.entries[self.selected].spec.methods[0].id.clone();
        Ok(())
    }
    pub fn launch(&mut self, mut request: Value, interpreter: String) -> Result<(), String> {
        if self.busy() {
            return Err("An operation is already running.".into());
        }
        request["request_id"] = json!(adapter::identity());
        self.session = Some(adapter::launch(request.clone(), interpreter)?);
        self.pending = Some(request);
        self.started = Some(Instant::now());
        self.error.clear();
        self.status = "Running the recorded simulation operation locally…".into();
        Ok(())
    }
    pub fn preview(&mut self, interpreter: String) -> Result<(), String> {
        self.draft.validate()?;
        let q = json!({"schema_version":2,"request_id":"preview","operation":"study_preview","study_spec":self.draft,"spec_wire":self.draft.wire()});
        self.launch(q, interpreter)
    }
    pub fn run_selected(&mut self, resume: bool, interpreter: String) -> Result<(), String> {
        let mut q = self
            .current()
            .ok_or("Choose a study")?
            .request(&self.project_file, "simulate")?;
        q["resume"] = json!(resume);
        self.launch(q, interpreter)
    }
    pub fn inspect(&mut self, interpreter: String) -> Result<(), String> {
        let mut q = self
            .current()
            .ok_or("Choose a study")?
            .request(&self.project_file, "study_inspect")?;
        q["replicate"] = json!(self.replicate);
        q["method_id"] = json!(self.method_id);
        self.launch(q, interpreter)
    }
    pub fn export(&mut self, interpreter: String) -> Result<(), String> {
        let mut q = self
            .current()
            .ok_or("Choose a study")?
            .request(&self.project_file, "study_export")?;
        q["replicate"] = json!(self.replicate);
        q["audience"] = json!(self.audience);
        q["export_path"] = json!(self.export_path.trim());
        self.launch(q, interpreter)
    }
    fn accept_checkpoint(&mut self, value: Value) -> Result<(), String> {
        let doc: StudyDocument = serde_json::from_value(value).map_err(|e| e.to_string())?;
        let entry = self
            .entries
            .iter()
            .find(|r| r.id == doc.study_id)
            .ok_or("Checkpoint belongs to another study")?;
        doc.validate(entry)?;
        self.status = format!(
            "{} / {} replications complete · {} backend-fit slots consumed · {}",
            doc.summary.completed_replications,
            doc.spec.replications,
            doc.spent(),
            doc.status
        );
        self.documents.insert(doc.study_id.clone(), Arc::new(doc));
        Ok(())
    }
    pub fn poll(&mut self, ctx: &egui::Context) {
        if self
            .shutdown
            .as_ref()
            .is_some_and(|(done, _)| done.load(std::sync::atomic::Ordering::Acquire))
        {
            let (_, start) = self.shutdown.take().unwrap();
            self.cancel_exit_ms = Some(start.elapsed().as_secs_f64() * 1000.);
            self.reload_selected();
        }
        let events = self
            .session
            .as_ref()
            .map(|s| s.receiver.try_iter().collect::<Vec<_>>())
            .unwrap_or_default();
        for event in events {
            match event {
                Event::Progress(mut p) => {
                    if let Some(doc) = p
                        .as_object_mut()
                        .and_then(|v| v.remove("checkpoint_document"))
                    {
                        if let Err(e) = self.accept_checkpoint(doc) {
                            self.error = e;
                            self.session = None;
                        }
                    }
                }
                Event::Completed(mut response) => {
                    let op = response["operation"].as_str().unwrap_or("").to_owned();
                    let mut result = response.as_object_mut().unwrap().remove("result").unwrap();
                    match op.as_str() {
                        "simulate" => {
                            if let Err(e) = self.accept_checkpoint(result) {
                                self.error = e;
                            }
                            self.stage = 2;
                        }
                        "study_preview" => {
                            result["preview_spec_wire"] = self
                                .pending
                                .as_ref()
                                .map(|q| q["spec_wire"].clone())
                                .unwrap_or(Value::Null);
                            self.sample = Some(Arc::new(result));
                            self.stage = 1;
                            self.status="Sample generated from the draft recipe; no fitting or test disclosure occurred.".into();
                        }
                        "study_inspect" => {
                            self.inspection = Some(Arc::new(result));
                            self.stage = 3;
                            self.status="Replication regenerated; observation fingerprint and numerical results verified.".into();
                        }
                        "study_export" => {
                            self.status = format!(
                                "{} packet exported to {}",
                                result["audience"].as_str().unwrap_or("Selected"),
                                result["destination"].as_str().unwrap_or("")
                            );
                        }
                        _ => {}
                    }
                    self.session = None;
                    self.pending = None;
                }
                Event::Failed(e) => {
                    self.error = e;
                    self.session = None;
                    self.pending = None;
                    self.reload_selected();
                }
            }
        }
        if self.busy() {
            ctx.request_repaint_after(std::time::Duration::from_millis(30));
        }
    }
    fn reload_selected(&mut self) {
        if let Some(entry) = self.current() {
            if let Ok(doc) = entry.load(&self.project_file) {
                self.documents.insert(entry.id.clone(), Arc::new(doc));
            }
        }
    }
    pub fn cancel(&mut self) {
        let started = Instant::now();
        if let Some(session) = &self.session {
            self.shutdown = Some((session.finished.clone(), started));
        }
        self.session = None;
        self.pending = None;
        self.cancel_ack_ms = Some(started.elapsed().as_secs_f64() * 1000.);
        self.status="Cancellation requested. Committed results remain in the project; unfinished reservations are charged conservatively on resume.".into();
        self.reload_selected();
    }
    pub fn ui(&mut self, ui: &mut egui::Ui, project_path: &mut String) -> Option<Action> {
        let started = Instant::now();
        let mut action = None;
        v::section(ui,"Assumptions and simulation","Known-truth experiments, repeated-sampling uncertainty and lessons from the same recorded study.");
        ui.label(RichText::new(&self.status).color(v::TEAL));
        if !self.error.is_empty() {
            ui.colored_label(v::RED, &self.error);
        }
        ui.horizontal_wrapped(|ui| {
            for (i, title) in [
                "Specify",
                "Sample",
                "Results",
                "Replication",
                "Teach & export",
            ]
            .iter()
            .enumerate()
            {
                if ui.selectable_label(self.stage == i, *title).clicked() {
                    self.stage = i;
                }
            }
            if self.busy() {
                ui.spinner();
                if ui.button("Cancel study").clicked() {
                    self.cancel();
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label("Project file");
            ui.add_enabled(
                !self.busy(),
                egui::TextEdit::singleline(project_path)
                    .hint_text("Absolute path to a new project.json")
                    .desired_width(400.),
            );
            if ui
                .add_enabled(!self.busy(), egui::Button::new("Save project"))
                .clicked()
            {
                action = Some(Action::Save);
            }
        });
        ui.small("Keep the associated .studies folder with this project. Save As copies its study artifacts; exports are separate reviewable packets.");
        if !self.entries.is_empty() {
            let selected = self.selected;
            egui::ComboBox::from_id_salt("simulation_history")
                .selected_text(format!(
                    "Study {}: {}",
                    self.selected + 1,
                    self.entries[self.selected].spec.scenario.kind
                ))
                .show_ui(ui, |ui| {
                    for (i, e) in self.entries.iter().enumerate() {
                        ui.selectable_value(
                            &mut self.selected,
                            i,
                            format!(
                                "{} · {} · {} repetitions",
                                i + 1,
                                e.spec.scenario.kind,
                                e.spec.replications
                            ),
                        );
                    }
                });
            if selected != self.selected {
                self.inspection = None;
                self.method_id = self.entries[self.selected].spec.methods[0].id.clone();
                self.replicate = 0;
            }
        }
        match self.stage {
            0 => self.design(ui, &mut action),
            1 => self.sample_view(ui),
            2 => self.results(ui, &mut action),
            3 => self.replication(ui, &mut action),
            _ => self.teach(ui, &mut action),
        }
        self.frame_ms.push(started.elapsed().as_secs_f64() * 1000.);
        if self.frame_ms.len() > 1000 {
            self.frame_ms.drain(..500);
        }
        action
    }
    fn design(&mut self, ui: &mut egui::Ui, action: &mut Option<Action>) {
        ui.add_enabled_ui(!self.busy(),|ui|{
            let previous=self.draft.scenario.kind.clone();
            egui::ComboBox::from_id_salt("scenario_kind").selected_text(&previous).show_ui(ui,|ui|{for kind in["heteroskedasticity","confounding","clustering"]{ui.selectable_value(&mut self.draft.scenario.kind,kind.into(),kind);}});
            if previous!=self.draft.scenario.kind{let seed=self.draft.seed;self.draft=StudySpec::for_scenario(&self.draft.scenario.kind);self.draft.seed=seed;self.sample=None;self.draft_prediction.clear();}
            ui.label("Study aim");ui.add(egui::TextEdit::multiline(&mut self.draft.aim).desired_rows(2).desired_width(f32::INFINITY));
            let s=&mut self.draft.scenario;
            ui.label(RichText::new(match s.kind.as_str(){"confounding"=>"D = X + c U + nu; Y = tau D + X + gamma U + sigma epsilon. U is hidden from observed analyses.","clustering"=>"Y = 1 + beta X + sigma [sqrt(rho) B_group + sqrt(1-rho) epsilon]. Independent groups share a group error.",_=>"Y = 1 + beta X + sigma sqrt(1 + h X^2) epsilon. X and epsilon are independent standard normals."}).color(v::AMBER));
            ui.horizontal_wrapped(|ui|{
                if s.kind=="clustering"{ui.label("Groups");ui.add(egui::DragValue::new(&mut s.groups).range(5..=50));ui.label("Observations/group");ui.add(egui::DragValue::new(&mut s.group_size).range(10..=40));s.n=s.groups*s.group_size;ui.label(format!("n = {}",s.n));}
                else{ui.label("Observations");ui.add(egui::DragValue::new(&mut s.n).range(50..=2000));}
            });
            if s.kind!="confounding"{ui.horizontal(|ui|{ui.label("Target");ui.selectable_value(&mut self.draft.estimand,"regression_coefficient".into(),"Slope beta");ui.selectable_value(&mut self.draft.estimand,"regression_intercept".into(),"Intercept");});}
            let parameters=match s.kind.as_str(){"confounding"=>vec![("tau",-3.,3.),("c",-2.,2.),("gamma",-2.,2.),("sigma",0.1,3.)],"clustering"=>vec![("beta",-3.,3.),("rho",0.,0.9),("sigma",0.1,3.)],_=>vec![("beta",-3.,3.),("h",0.,4.),("sigma",0.1,3.)]};
            ui.horizontal_wrapped(|ui|{for(name,lo,hi)in parameters{ui.label(name);ui.add(egui::DragValue::new(s.parameters.get_mut(name).unwrap()).range(lo..=hi).speed(0.05));}});
            ui.label("Method variants (at most four). Only OLS exposes coefficient intervals.");
            let available=if s.kind=="confounding"{vec![MethodSpec::interval("observed","Observed OLS / HC3","HC3",false),MethodSpec::interval("oracle","Simulator-only oracle / HC3","HC3",true),MethodSpec::predictive("ridge"),MethodSpec::predictive("forest")]}
                else{let mut m=vec![MethodSpec::interval("classical","OLS classical","classical",false),MethodSpec::interval("robust","OLS HC3","HC3",false)];if s.kind=="clustering"{m.push(MethodSpec::interval("cluster","OLS cluster","cluster",false));}m.extend([MethodSpec::predictive("ridge"),MethodSpec::predictive("forest")]);m};
            ui.horizontal_wrapped(|ui|{for method in available{let mut selected=self.draft.methods.iter().any(|m|m.id==method.id);if ui.checkbox(&mut selected,&method.label).changed(){if selected{self.draft.methods.push(method);}else{self.draft.methods.retain(|m|m.id!=method.id);}}}});
            ui.horizontal_wrapped(|ui|{ui.label("Replications");ui.add(egui::DragValue::new(&mut self.draft.replications).range(2..=500));ui.label("Seed");ui.add(egui::DragValue::new(&mut self.draft.seed).range(0..=u32::MAX as u64));});
            ui.label(format!("Preflight: {} backend fits / {} allowed; {} second deadline. Inference counts both prediction and inferential fits.",self.draft.planned_fits(),self.draft.max_fits,self.draft.seconds));
            let validation=self.draft.validate();if let Err(e)=&validation{ui.colored_label(v::RED,e);}
            ui.label("Prediction before seeing results");ui.add(egui::TextEdit::multiline(&mut self.draft_prediction).desired_rows(2).desired_width(f32::INFINITY));
            ui.horizontal(|ui|{if ui.add_enabled(validation.is_ok(),egui::Button::new("Inspect generated sample")).clicked(){*action=Some(Action::Preview);}if ui.add_enabled(validation.is_ok(),egui::Button::new("Freeze and run new study")).clicked(){*action=Some(Action::Run);}});
            ui.small("Changing this draft never changes a recorded study. Run creates a new immutable specification. Scientific backends do not expose an optimizer trace.");
        });
    }
    fn sample_view(&mut self, ui: &mut egui::Ui) {
        let Some(sample) = self.sample.clone() else {
            ui.label("Inspect a generated sample from Specify first.");
            return;
        };
        ui.label(
            "Synthetic observations; this scatter plot shows association, not identification.",
        );
        if sample["preview_spec_wire"].as_str() != Some(&self.draft.wire()) {
            ui.colored_label(v::AMBER,"Recorded sample belongs to an earlier draft. Inspect a new sample to see changed settings.");
        }
        draw_data(ui, &sample["observed_data"], None, "study_sample");
        ui.checkbox(&mut self.reveal, "Reveal simulator target (instructor)");
        if self.reveal {
            ui.colored_label(
                v::AMBER,
                format!(
                    "Known simulator target: {}. {}",
                    sample["simulator_truth"],
                    sample["truth_scope"].as_str().unwrap_or("")
                ),
            );
        }
        ui.collapsing("Observation identities and first 15 rows", |ui| {
            ui.label(format!(
                "Data SHA-256: {}",
                sample["data_sha256"].as_str().unwrap_or("")
            ));
            if let Some(rows) = sample["observed_data"]["rows"].as_array() {
                for (i, row) in rows.iter().take(15).enumerate() {
                    ui.monospace(format!("{i}: {row}"));
                }
            }
        });
    }
    fn results(&mut self, ui: &mut egui::Ui, action: &mut Option<Action>) {
        let Some(doc) = self.current_document().cloned() else {
            ui.label("A saved study's results appear here as replications commit.");
            return;
        };
        identity(ui, &doc);
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(!self.busy(), egui::Button::new("Resume unfinished study"))
                .clicked()
            {
                *action = Some(Action::Resume);
            }
            if ui
                .add_enabled(
                    !self.busy(),
                    egui::Button::new("Copy specification to draft"),
                )
                .clicked()
            {
                self.draft = doc.spec.clone();
                self.stage = 0;
            }
        });
        if let Some(reason) = &doc.stop_reason {
            ui.colored_label(v::AMBER, reason);
        }
        for m in &doc.summary.methods {
            v::panel().show(ui,|ui|{
                ui.strong(&m.label);ui.label(format!("{} successful / {} attempted; {} failed",m.completed,m.attempted,m.failed));
                egui::Grid::new(("study_metrics",&m.method_id)).num_columns(3).show(ui,|ui|{
                    ui.strong("Quantity");ui.strong("Estimate ± Monte Carlo SE");ui.strong("Denominator");ui.end_row();
                    for(name,metric)in[("Bias",&m.bias),("Coefficient MSE",&m.mse),("Coverage among valid intervals",&m.coverage_valid_intervals),("Valid interval AND covering target",&m.valid_and_covering),("Mean interval width",&m.interval_width),("Failure rate",&m.failure_rate)]{metric_row(ui,name,metric);}
                    for(name,metric)in &m.prediction{metric_row(ui,&format!("Prediction MSE: {name}"),metric);}
                });
                ui.label(format!("Empirical SD: {}; Monte Carlo SE approximation: {}",optional(m.empirical_sd),optional(m.empirical_sd_mc_se)));
                if m.empirical_sd.is_some(){ui.small("SD uncertainty uses a fourth-moment delta approximation; small studies require caution.");}
            });
        }
        ui.small("Monte Carlo SE describes uncertainty from finite replications. It is different from a coefficient standard error or a confidence interval. A plug-in zero MC SE at 0%/100% coverage does not establish certainty.");
        let series = doc
            .spec
            .methods
            .iter()
            .enumerate()
            .map(|(index, m)| {
                let points = doc
                    .records
                    .iter()
                    .filter_map(|r| {
                        r.methods
                            .iter()
                            .find(|v| v.method_id == m.id && v.status == "completed")
                            .and_then(|v| v.estimate.map(|estimate| (estimate, r.replicate as f64)))
                    })
                    .collect();
                Series::points(
                    m.label.clone(),
                    points,
                    vec![],
                    v::PALETTE[index % v::PALETTE.len()],
                    false,
                )
            })
            .collect::<Vec<_>>();
        let truth = doc.spec.truth();
        (Plot {
            id: "study_sampling",
            x_label: "Coefficient estimate (synthetic units)",
            y_label: "Replication identity",
            height: 260.,
            series: &series,
            selected: None,
            vertical: Some(truth),
            y_range: None,
        })
        .show(ui);
        egui::ComboBox::from_id_salt("interval_method")
            .selected_text(&self.method_id)
            .show_ui(ui, |ui| {
                for m in &doc.spec.methods {
                    if m.operation == "inference" {
                        ui.selectable_value(&mut self.method_id, m.id.clone(), &m.label);
                    }
                }
            });
        interval_plot(ui, &doc, &self.method_id, &mut self.replicate);
        ui.collapsing("Paired method comparisons", |ui| {
            for pair in &doc.summary.paired {
                ui.label(format!("{} minus {}", pair["method_a"], pair["method_b"]));
                ui.label(format!(
                    "Coefficient difference: {}",
                    pair["estimate_difference"]
                ));
                ui.label(format!(
                    "Prediction differences: {}",
                    pair["prediction_difference"]
                ));
                ui.small(pair["scope"].as_str().unwrap_or(""));
            }
        });
        ui.collapsing("Failed estimates and warnings", |ui| {
            for r in &doc.records {
                for m in &r.methods {
                    if m.status == "failed" {
                        ui.colored_label(
                            v::RED,
                            format!(
                                "Replication {} · {} · {}",
                                r.replicate + 1,
                                m.method_id,
                                m.error.as_deref().unwrap_or("Unknown failure")
                            ),
                        );
                    }
                    for warning in &m.warnings {
                        ui.label(format!(
                            "Replication {} · {}: {}",
                            r.replicate + 1,
                            m.method_id,
                            warning
                        ));
                    }
                }
            }
        });
    }
    fn replication(&mut self, ui: &mut egui::Ui, action: &mut Option<Action>) {
        let Some(entry) = self.current().cloned() else {
            ui.label("Run a study before inspecting a replication.");
            return;
        };
        ui.label("Regenerate one recorded result with the same source and random streams. Inspection consumes its own recorded fit reservation.");
        let mut display = self.replicate + 1;
        ui.horizontal(|ui| {
            ui.label("Replication");
            ui.add(egui::DragValue::new(&mut display).range(1..=entry.spec.replications));
            egui::ComboBox::from_id_salt("inspect_method")
                .selected_text(&self.method_id)
                .show_ui(ui, |ui| {
                    for m in &entry.spec.methods {
                        ui.selectable_value(&mut self.method_id, m.id.clone(), &m.label);
                    }
                });
            if ui
                .add_enabled(!self.busy(), egui::Button::new("Regenerate and verify"))
                .clicked()
            {
                *action = Some(Action::Inspect);
            }
        });
        self.replicate = display - 1;
        let Some(result) = self.inspection.clone() else {
            return;
        };
        ui.colored_label(
            v::TEAL,
            format!(
                "Verified replication {} · data {}",
                result["replicate"].as_u64().unwrap_or(0) + 1,
                result["data_sha256"].as_str().unwrap_or("")
            ),
        );
        draw_data(
            ui,
            &result["observed_data"],
            Some(&result["analysis_result"]["predictions"]),
            "study_replication",
        );
        ui.label(
            "Observed points and fitted predictions. No backend optimizer trace is available.",
        );
        if let Some(table) = result["analysis_result"]["coefficient_table"].as_array() {
            egui::Grid::new("replication_coefficients")
                .striped(true)
                .show(ui, |ui| {
                    for title in ["Term", "Estimate", "SE", "Lower", "Upper"] {
                        ui.strong(title);
                    }
                    ui.end_row();
                    for r in table {
                        ui.label(r["term"].as_str().unwrap_or(""));
                        for key in ["estimate", "std_error", "lower", "upper"] {
                            ui.label(optional(r[key].as_f64()));
                        }
                        ui.end_row();
                    }
                });
        }
    }
    fn teach(&mut self, ui: &mut egui::Ui, action: &mut Option<Action>) {
        let Some(entry) = self.current().cloned() else {
            ui.label("Run or select a study to teach its actual results.");
            return;
        };
        let (prompt, answer, transfer) = lesson(&entry.spec.scenario.kind);
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.presentation, "Presentation text");
            ui.checkbox(&mut self.reveal, "Instructor explanations");
        });
        let size = if self.presentation { 21. } else { 15. };
        ui.label(RichText::new(prompt).size(size));
        self.entries[self.selected].answers.resize(2, String::new());
        ui.add(
            egui::TextEdit::multiline(&mut self.entries[self.selected].answers[0])
                .desired_rows(3)
                .desired_width(f32::INFINITY),
        );
        if self.reveal {
            ui.label(RichText::new(answer).size(size).color(v::AMBER));
            if let Some(doc) = self.current_document() {
                identity(ui, doc);
                for m in &doc.summary.methods {
                    ui.label(format!(
                        "{}: bias {}; coverage {} ({} valid intervals); {} failures",
                        m.label,
                        optional(m.bias.value),
                        optional(m.coverage_valid_intervals.value),
                        m.coverage_valid_intervals.n,
                        m.failed
                    ));
                }
            }
        }
        ui.label(RichText::new(transfer).size(size));
        ui.add(
            egui::TextEdit::multiline(&mut self.entries[self.selected].answers[1])
                .desired_rows(3)
                .desired_width(f32::INFINITY),
        );
        if ui.button("Save learning responses").clicked() {
            *action = Some(Action::Save);
        }
        ui.separator();
        ui.strong("Audience-specific export");
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("study_audience")
                .selected_text(&self.audience)
                .show_ui(ui, |ui| {
                    for a in ["student", "instructor", "research"] {
                        ui.selectable_value(&mut self.audience, a.into(), a);
                    }
                });
            ui.add(
                egui::TextEdit::singleline(&mut self.export_path)
                    .hint_text("New absolute folder")
                    .desired_width(360.),
            );
            if ui
                .add_enabled(!self.busy(), egui::Button::new("Export packet"))
                .clicked()
            {
                *action = Some(Action::Export);
            }
        });
        ui.label(if self.audience=="student"{"Student packet: observed data and worksheet only. No hidden U, generator seed, true coefficients or instructor key. This is not an examination-security mechanism."}else{"Research/instructor packet includes the generator, known truth, seeds, answers, exact source, vector figures and executable reproduction. Review before sharing."});
    }
}
fn interval_plot(ui: &mut egui::Ui, doc: &StudyDocument, method_id: &str, selected: &mut usize) {
    let method = doc
        .spec
        .methods
        .iter()
        .find(|m| m.id == method_id && m.operation == "inference")
        .or_else(|| doc.spec.methods.iter().find(|m| m.operation == "inference"));
    let Some(method) = method else {
        return;
    };
    let rows = doc
        .records
        .iter()
        .filter_map(|r| {
            r.methods
                .iter()
                .find(|m| m.method_id == method.id)
                .map(|m| (r.replicate, m))
        })
        .take(60)
        .collect::<Vec<_>>();
    if rows.is_empty() {
        return;
    }
    ui.label(format!("Actual 95% coefficient intervals: {}. First 60 attempted replications; click a row to select it. Red intervals miss the simulator target.",method.label));
    let truth = doc.spec.truth();
    let mut lo = truth;
    let mut hi = truth;
    for (_, m) in &rows {
        if let (Some(a), Some(b)) = (m.lower, m.upper) {
            lo = lo.min(a);
            hi = hi.max(b);
        }
    }
    let pad = ((hi - lo) * 0.06).max(1e-6);
    lo -= pad;
    hi += pad;
    let (response, p) = ui.allocate_painter(
        egui::vec2(
            ui.available_width().max(250.),
            rows.len() as f32 * 15. + 28.,
        ),
        egui::Sense::click(),
    );
    let r = response.rect;
    let left = r.left() + 65.;
    let width = (r.right() - left - 8.).max(1.);
    let x = |v: f64| left + ((v - lo) / (hi - lo)) as f32 * width;
    p.line_segment(
        [
            egui::pos2(x(truth), r.top()),
            egui::pos2(x(truth), r.bottom() - 20.),
        ],
        egui::Stroke::new(1., v::MUTED),
    );
    for (i, (id, m)) in rows.iter().enumerate() {
        let y = r.top() + i as f32 * 15. + 7.;
        p.text(
            egui::pos2(r.left(), y),
            egui::Align2::LEFT_CENTER,
            format!("{}", id + 1),
            egui::FontId::proportional(10.),
            if *id == *selected { v::AMBER } else { v::MUTED },
        );
        if let (Some(a), Some(b), Some(estimate)) = (m.lower, m.upper, m.estimate) {
            let color = if a <= truth && truth <= b {
                v::BLUE
            } else {
                v::RED
            };
            p.line_segment(
                [egui::pos2(x(a), y), egui::pos2(x(b), y)],
                egui::Stroke::new(1.5, color),
            );
            p.circle_filled(egui::pos2(x(estimate), y), 2., color);
        } else {
            p.text(
                egui::pos2(left, y),
                egui::Align2::LEFT_CENTER,
                "failed / no interval",
                egui::FontId::proportional(10.),
                v::RED,
            );
        }
    }
    for (value, align) in [(lo, egui::Align2::LEFT_TOP), (hi, egui::Align2::RIGHT_TOP)] {
        p.text(
            egui::pos2(x(value), r.bottom() - 16.),
            align,
            v::number(value),
            egui::FontId::proportional(10.),
            v::MUTED,
        );
    }
    if response.clicked() {
        if let Some(pos) = response.interact_pointer_pos() {
            let i = ((pos.y - r.top()) / 15.).floor().max(0.) as usize;
            if let Some((id, _)) = rows.get(i) {
                *selected = *id;
            }
        }
    }
}
fn identity(ui: &mut egui::Ui, doc: &StudyDocument) {
    ui.label(format!(
        "Recorded study {} · {} · {} / {} replications · {}",
        doc.study_id,
        doc.spec.scenario.kind,
        doc.summary.completed_replications,
        doc.spec.replications,
        doc.status
    ));
    ui.small(format!("Frozen specification SHA-256: {}", doc.spec_sha256));
    ui.colored_label(v::AMBER,"The target is known because this is a declared simulation. It does not establish identification in imported research data.");
}
fn optional(value: Option<f64>) -> String {
    value
        .map(v::number)
        .unwrap_or_else(|| "not available".into())
}
fn metric_row(ui: &mut egui::Ui, label: &str, metric: &Metric) {
    ui.label(label);
    ui.label(if metric.n == 0 {
        "not applicable / no valid estimates".into()
    } else {
        format!("{} ± {}", optional(metric.value), optional(metric.mc_se))
    });
    ui.label(metric.n.to_string());
    ui.end_row();
}
fn draw_data(ui: &mut egui::Ui, data: &Value, predictions: Option<&Value>, id: &'static str) {
    let Some(rows) = data["rows"].as_array() else {
        return;
    };
    let columns = data["columns"].as_array().unwrap();
    let target = columns
        .iter()
        .position(|v| v == "y")
        .unwrap_or(columns.len() - 1);
    let x = 0;
    let points = rows
        .iter()
        .filter_map(|r| Some((r[x].as_f64()?, r[target].as_f64()?)))
        .collect();
    let identities = (0..rows.len()).collect::<Vec<_>>();
    let mut series = vec![Series::points(
        "Observed",
        points,
        identities.clone(),
        v::BLUE,
        false,
    )];
    if let Some(values) = predictions.and_then(Value::as_array) {
        let points = rows
            .iter()
            .zip(values)
            .filter_map(|(r, y)| Some((r[x].as_f64()?, y.as_f64()?)))
            .collect();
        series.push(Series::points("Fitted", points, identities, v::TEAL, true));
    }
    (Plot {
        id,
        x_label: columns[x].as_str().unwrap_or("Predictor"),
        y_label: "Outcome / fitted outcome",
        height: 280.,
        series: &series,
        selected: None,
        vertical: None,
        y_range: None,
    })
    .show(ui);
}
pub fn lesson(kind: &str) -> (&'static str, &'static str, &'static str) {
    match kind{
    "confounding"=>("Will more observations or robust standard errors remove bias for the simulator's treatment effect?","The simulator supplies tau. U affects both treatment and outcome; an observed regression need not identify tau. The oracle uses information unavailable to an ordinary analyst. A covariance correction does not remove confounding.","Set c or gamma to zero. Explain what changes and why this does not verify exogeneity in a real dataset."),
    "clustering"=>("What changes when observations share a group-level error? What population will predictions serve?","Dependence affects targets differently. Here X is independent across observations, so group dependence can affect the intercept much more strongly than the slope. Inspect both targets. Existing-group and new-group risks answer different questions; few clusters can make conventional inference unreliable.","Set rho to zero, then change the number of groups. Interpret coverage, failures and Monte Carlo uncertainty together."),
    _=>("Will changing error variance change the target slope, the OLS point estimate, or the reliability of its interval?","HC3 changes covariance estimation, not the OLS point estimate. Coverage is itself estimated from finite replications; it need not equal exactly 95%.","Set h to zero, then increase sample size. Predict which differences should diminish and inspect the resulting evidence."),
}
}

impl Drop for StudyPanel {
    fn drop(&mut self) {
        if let Some(session) = &self.session {
            session
                .cancel
                .store(true, std::sync::atomic::Ordering::Relaxed);
            let start = Instant::now();
            while !session.finished.load(std::sync::atomic::Ordering::Acquire)
                && start.elapsed() < std::time::Duration::from_secs(2)
            {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
    }
}
