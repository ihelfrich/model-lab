//! Native computational teaching workspace. Family-specific state stays in engines;
//! this shell owns selection, asynchronous jobs, linked views and lesson progress.
use crate::{
    data::{Data, Table},
    experiment::{self, FittedModel, ModelId, PenaltyPoint, Run, RunConfig, Task},
    linear::{self, LinearMethod},
    python, teaching,
    visuals::{self as v, Plot, Series},
};
use eframe::egui::{self, Color32, RichText, Vec2};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
};

const VIEWS: [&str; 8] = [
    "Explore",
    "Coefficients",
    "Optimization",
    "Learn",
    "Code",
    "Data",
    "Research",
    "Decisions",
];
type FitResult = (u64, Result<Run, String>);
type PathResult = (String, Result<Vec<PenaltyPoint>, String>);
#[cfg(feature = "acceptance")]
struct StudyNativeCheck {
    step: usize,
    frames: usize,
    directory: PathBuf,
    started: std::time::Instant,
}
pub struct Workbench {
    decision: crate::decision::Decision,
    research: crate::research_ui::Research,
    table: Option<Table>,
    target_column: usize,
    feature_columns: Vec<bool>,
    surface_cache: Option<(String, f64, f64, f64, Vec<(usize, usize, f64)>)>,
    cfg: RunConfig,
    data: Data,
    data_name: String,
    scenario: usize,
    noise: f64,
    custom: bool,
    run: Option<Arc<Run>>,
    revision: u64,
    receiver: Option<mpsc::Receiver<FitResult>>,
    cancel: Arc<AtomicBool>,
    due: Option<f64>,
    stage: usize,
    feature: usize,
    selected: usize,
    view: usize,
    threshold: f64,
    standardized: bool,
    playing: bool,
    last_tick: f64,
    status: String,
    error: String,
    pinned: Option<(Arc<Run>, usize)>,
    path: Option<(String, Vec<PenaltyPoint>)>,
    path_rx: Option<mpsc::Receiver<PathResult>>,
    path_cancel: Arc<AtomicBool>,
    quiz_answer: Vec<Option<usize>>,
    quiz_checked: Vec<bool>,
    lesson_step: usize,
    code: python::Workspace,
    code_revision: u64,
    load_path: String,
    save_path: String,
    legacy: crate::ui::Lab,
    legacy_open: bool,
    #[cfg(feature = "acceptance")]
    smoke_frame: usize,
    #[cfg(feature = "acceptance")]
    smoke_image: bool,
    #[cfg(feature = "acceptance")]
    research_smoke_state: Option<(usize, usize, PathBuf)>,
    #[cfg(feature = "acceptance")]
    capture: Option<crate::acceptance::CaptureGate>,
    #[cfg(feature = "acceptance")]
    study_smoke_state: Option<StudyNativeCheck>,
}
impl Workbench {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let legacy = crate::ui::Lab::new(cc);
        cc.egui_ctx.set_style_of(egui::Theme::Dark, v::style());
        let mut app = Self {
            decision: crate::decision::Decision::default(),
            research: crate::research_ui::Research::default(),
            table: None,
            target_column: 0,
            feature_columns: vec![],
            surface_cache: None,
            cfg: RunConfig::default(),
            data: experiment::scenario(0, 4710, 0.8),
            data_name: experiment::SCENARIOS[0].0.into(),
            scenario: 0,
            noise: 0.8,
            custom: false,
            run: None,
            revision: 1,
            receiver: None,
            cancel: Arc::new(AtomicBool::new(false)),
            due: None,
            stage: 0,
            feature: 0,
            selected: 0,
            view: 0,
            threshold: 0.5,
            standardized: false,
            playing: false,
            last_tick: 0.,
            status: "Preparing the first experiment".into(),
            error: String::new(),
            pinned: None,
            path: None,
            path_rx: None,
            path_cancel: Arc::new(AtomicBool::new(false)),
            quiz_answer: vec![None; 4],
            quiz_checked: vec![false; 4],
            lesson_step: 0,
            code: python::Workspace::default(),
            code_revision: 0,
            load_path: String::new(),
            save_path: String::new(),
            legacy,
            legacy_open: false,
            #[cfg(feature = "acceptance")]
            smoke_frame: 0,
            #[cfg(feature = "acceptance")]
            smoke_image: false,
            #[cfg(feature = "acceptance")]
            research_smoke_state: None,
            #[cfg(feature = "acceptance")]
            capture: None,
            #[cfg(feature = "acceptance")]
            study_smoke_state: None,
        };
        if let Some(path) = std::env::args().skip_while(|a| a != "--decision-evidence").nth(1) {
            app.decision.load(path);
            app.view = 7;
        }
        app.fit();
        app
    }
    fn fit(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        self.cancel = Arc::new(AtomicBool::new(false));
        self.due = None;
        self.error.clear();
        self.playing = false;
        let (tx, rx) = mpsc::channel();
        self.receiver = Some(rx);
        let data = self.data.clone();
        let cfg = self.cfg.clone();
        let name = self.data_name.clone();
        let cancel = self.cancel.clone();
        let revision = self.revision;
        self.status = format!("Fitting {}…", self.cfg.model.descriptor().name);
        std::thread::spawn(move || {
            let result = crate::adapter::NativeAdapter.fit(name, data, cfg, cancel);
            let _ = tx.send((revision, result));
        });
    }
    fn dirty(&mut self, time: f64) {
        self.revision += 1;
        self.cancel.store(true, Ordering::Relaxed);
        self.path_cancel.store(true, Ordering::Relaxed);
        self.due = Some(time + 0.2);
        self.playing = false;
        self.status = "Settings changed · preparing a new fit".into();
    }
    fn poll(&mut self, ctx: &egui::Context) {
        if let Some(receiver) = &self.receiver {
            if let Ok((revision, result)) = receiver.try_recv() {
                self.receiver = None;
                if revision == self.revision {
                    match result {
                        Ok(run) => {
                            self.cfg = run.config.clone();
                            self.stage = run.last_stage();
                            self.feature = self.feature.min(run.data.names.len() - 1);
                            self.selected = self.selected.min(run.data.y.len() - 1);
                            self.status = format!(
                                "{} training · {} validation observations",
                                run.partition.train.len(),
                                run.partition.valid.len()
                            );
                            self.run = Some(Arc::new(run));
                            self.code_revision = 0;
                        }
                        Err(e) => {
                            self.error = e;
                            self.status = "The requested fit was not accepted".into();
                        }
                    }
                }
            }
        }
        if let Some(receiver) = &self.path_rx {
            if let Ok((key, result)) = receiver.try_recv() {
                self.path_rx = None;
                match result {
                    Ok(path) => self.path = Some((key, path)),
                    Err(e) => {
                        if !e.contains("canceled") {
                            self.error = e;
                        }
                    }
                }
            }
        }
        self.code.poll();
        self.research.poll(ctx);
        let time = ctx.input(|i| i.time);
        if self.due.is_some_and(|due| time >= due) {
            self.fit();
        }
        if self.playing && time - self.last_tick > 0.55 {
            self.last_tick = time;
            if let Some(run) = &self.run {
                if self.stage < run.last_stage() {
                    self.stage += 1;
                } else {
                    self.playing = false;
                }
            }
        }
        if self.receiver.is_some()
            || self.path_rx.is_some()
            || self.code.running
            || self.playing
            || self.due.is_some()
        {
            ctx.request_repaint_after(std::time::Duration::from_millis(25));
        }
    }
    fn select_model(&mut self, model: ModelId, time: f64) {
        self.cfg.model = model;
        self.lesson_step = 0;
        self.quiz_answer = vec![None; 4];
        self.quiz_checked = vec![false; 4];
        if !self.custom {
            if model.descriptor().task == Task::BinaryClassification && self.scenario != 3 {
                self.scenario = 3;
                self.generate_data();
            } else if model.descriptor().task == Task::Regression && self.scenario == 3 {
                self.scenario = 0;
                self.generate_data();
            }
        }
        self.dirty(time);
    }
    fn generate_data(&mut self) {
        self.table = None;
        self.data = experiment::scenario(self.scenario, self.cfg.seed, self.noise);
        self.data_name = experiment::SCENARIOS[self.scenario].0.into();
        self.custom = false;
        self.cfg.tree.mtry = self.cfg.tree.mtry.min(self.data.names.len()).max(1);
    }
    fn sidebar(&mut self, root: &mut egui::Ui) {
        let ctx = root.ctx().clone();
        egui::Panel::left("model_catalogue").exact_size(190.).resizable(false).show(root,|ui|{
            ui.add_space(16.);ui.label(RichText::new("MODEL LAB").size(20.).strong().color(v::TEAL));ui.label(RichText::new("Computational teaching").size(11.).color(v::MUTED));ui.add_space(22.);
            for(group,models)in[("Linear models",&experiment::MODELS[..5]),("Tree ensembles",&experiment::MODELS[5..])]{
                ui.label(RichText::new(group).size(11.).color(v::MUTED));ui.add_space(3.);
                if self.view==6&&self.research.tab==5{ui.small("Simulation methods are selected inside Specify.");}
                for descriptor in models{let selected=if self.view==6{self.research.template["model"]==descriptor.id.key()}else{self.cfg.model==descriptor.id};let response=ui.add_sized([ui.available_width(),39.],egui::Button::new(RichText::new(descriptor.name).size(13.)).selected(selected));if response.on_hover_text(descriptor.summary).clicked(){if self.view==6&&self.research.tab==5{self.research.tab=0;self.research.select_model(descriptor.id.key());}else if self.view==6{self.research.select_model(descriptor.id.key());}else{self.select_model(descriptor.id,ctx.input(|i|i.time));}}}
                ui.add_space(17.);
            }
            ui.separator();ui.add_space(7.);if ui.button("Open tree laboratory ↗").on_hover_text("The complete specialist workspace from version 0.5, including its original 100-question bank and tree diagrams.").clicked(){self.legacy_open=true;}
            ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT),|ui|{ui.label(RichText::new(format!("Local workspace · {}",env!("CARGO_PKG_VERSION"))).size(11.).color(v::MUTED));});
        });
    }
    fn controls(&mut self, root: &mut egui::Ui) {
        let ctx = root.ctx().clone();
        let time = ctx.input(|i| i.time);
        let mut changed = false;
        egui::Panel::right("experiment_controls").exact_size(252.).resizable(false).show(root,|ui|{egui::ScrollArea::vertical().show(ui,|ui|{
            ui.add_space(15.);ui.label(RichText::new("Experiment").size(18.).strong());ui.add_space(7.);
            ui.label(RichText::new("Dataset").color(v::MUTED).size(12.));let old=self.scenario;
            egui::ComboBox::from_id_salt("data_scenario").width(220.).selected_text(if self.custom{self.data_name.as_str()}else{experiment::SCENARIOS[self.scenario].0}).show_ui(ui,|ui|{for(i,(name,_))in experiment::SCENARIOS.iter().enumerate(){ui.selectable_value(&mut self.scenario,i,*name);}});
            if old!=self.scenario{self.generate_data();changed=true;}
            if !self.custom{ui.label(RichText::new(experiment::SCENARIOS[self.scenario].1).size(11.).color(v::MUTED));if self.scenario!=3{ui.add_space(4.);ui.label("Outcome noise");if ui.add(egui::Slider::new(&mut self.noise,0.0..=3.).fixed_decimals(2)).changed(){self.generate_data();changed=true;}}}
            if ui.button("Import numeric CSV…").clicked(){match crate::ui::choose_path(false){Ok(Some(path))=>self.load_csv(path),Ok(None)=>{},Err(e)=>self.error=e}}
            ui.add_space(15.);ui.separator();ui.add_space(8.);
            match self.cfg.model{
                ModelId::Linear(method)=>{
                    ui.label(RichText::new("Model controls").strong());
                    if method.penalized(){ui.label("Penalty strength α");changed|=ui.add(egui::Slider::new(&mut self.cfg.linear.alpha,0.0001..=100.).logarithmic(true).show_value(false)).changed();
                        changed|=ui.add(egui::DragValue::new(&mut self.cfg.linear.alpha).range(0.0..=10000.).speed(0.01).max_decimals(5)).changed();
                        ui.label(RichText::new("Larger α increases regularization. α=0 removes the penalty.").size(11.).color(v::MUTED));}
                    if method==LinearMethod::ElasticNet{ui.add_space(6.);ui.label("L1 mixture");changed|=ui.add(egui::Slider::new(&mut self.cfg.linear.mix,0.0..=1.).fixed_decimals(2)).changed();ui.label(RichText::new("0 = ridge · 1 = lasso").size(11.).color(v::MUTED));}
                    ui.add_space(7.);changed|=ui.checkbox(&mut self.cfg.linear.standardize,"Standardize predictors").changed();ui.label(RichText::new("Means and scales come from training rows only. The intercept is unpenalized.").size(11.).color(v::MUTED));
                    if method==LinearMethod::Logistic{ui.add_space(8.);ui.label("Decision threshold");ui.add(egui::Slider::new(&mut self.threshold,0.05..=0.95).fixed_decimals(2));ui.label(RichText::new("Changes decisions, not the probability fit.").size(11.).color(v::MUTED));}
                },
                _=>{
                    ui.label("Maximum depth");changed|=ui.add(egui::Slider::new(&mut self.cfg.tree.depth,1..=8)).changed();ui.label("Minimum leaf rows");changed|=ui.add(egui::Slider::new(&mut self.cfg.tree.min_leaf,1..=30)).changed();
                    if self.cfg.model!=ModelId::Tree{ui.label("Number of trees");changed|=ui.add(egui::Slider::new(&mut self.cfg.tree.trees,1..=200)).changed();}
                    if matches!(self.cfg.model,ModelId::Boosting|ModelId::Stochastic){ui.label("Learning rate");changed|=ui.add(egui::Slider::new(&mut self.cfg.tree.rate,0.01..=1.).logarithmic(true)).changed();}
                    if self.cfg.model==ModelId::Stochastic{ui.label("Sample fraction");changed|=ui.add(egui::Slider::new(&mut self.cfg.tree.fraction,0.1..=1.)).changed();}
                    if self.cfg.model==ModelId::Forest{ui.label("Features per split");changed|=ui.add(egui::Slider::new(&mut self.cfg.tree.mtry,1..=self.data.names.len())).changed();}
                }
            }
            ui.add_space(15.);ui.separator();ui.add_space(7.);ui.label(RichText::new("Training design").strong());
            ui.label("Validation fraction");changed|=ui.add(egui::Slider::new(&mut self.cfg.validation,0.1..=0.5).fixed_decimals(2)).changed();ui.horizontal(|ui|{ui.label("Seed");if ui.add(egui::DragValue::new(&mut self.cfg.seed).range(0..=1000000)).changed(){if !self.custom{self.generate_data();}changed=true;}});
            ui.add_space(10.);if ui.add_sized([ui.available_width(),35.],egui::Button::new("Fit experiment").fill(Color32::from_rgb(35,82,78))).clicked(){self.revision+=1;self.fit();changed=false;}
            if self.receiver.is_some(){ui.horizontal(|ui|{ui.spinner();ui.label("Fitting in background");});}
            ui.add_space(16.);ui.collapsing("Save and reproduce",|ui|{
                ui.label(RichText::new("Saved experiments contain the selected numeric data. Choose a destination you intend to use.").size(11.).color(v::MUTED));
                ui.add(egui::TextEdit::singleline(&mut self.save_path).hint_text("Path to experiment.json"));
                if ui.button("Save experiment").clicked(){self.save_run();}
                ui.add(egui::TextEdit::singleline(&mut self.load_path).hint_text("CSV or saved JSON path"));
                if ui.button("Load file").clicked(){self.load_file(PathBuf::from(self.load_path.trim()));}
            });
        });});
        if changed {
            self.dirty(time);
        }
    }
    fn load_csv(&mut self, path: PathBuf) {
        let result = std::fs::metadata(&path)
            .map_err(|e| e.to_string())
            .and_then(|m| {
                if m.len() > 32 * 1024 * 1024 {
                    Err("CSV exceeds 32MB.".into())
                } else {
                    std::fs::read_to_string(&path).map_err(|e| e.to_string())
                }
            })
            .and_then(|text| {
                Table::parse_research(
                    &path.file_name().unwrap_or_default().to_string_lossy(),
                    &text,
                )
            });
        match result {
            Ok(table) => {
                self.target_column = table.numeric.iter().rposition(|&v| v).unwrap_or(0);
                self.feature_columns = table
                    .numeric
                    .iter()
                    .enumerate()
                    .map(|(j, &v)| v && j != self.target_column)
                    .collect();
                self.table = Some(table);
                self.revision += 1;
                self.view = 5;
                self.apply_columns();
            }
            Err(e) => self.error = e,
        }
    }
    fn apply_columns(&mut self) {
        if self.table.as_ref().is_some_and(|t| t.rows.len() > 5000) {
            if let (Some(run), Some(table)) = (&self.run, &self.table) {
                self.research.pinned_source = false;
                self.research.synchronize(run, Some(table), self.revision);
                self.research.pinned_source = true;
                self.view = 6;
                self.error.clear();
                self.status =
                    "CSV loaded into Research; native teaching remains limited to 5,000 rows."
                        .into();
            }
            return;
        }
        let Some(table) = &self.table else {
            return;
        };
        let features = self
            .feature_columns
            .iter()
            .enumerate()
            .filter_map(|(j, &v)| (v && j != self.target_column).then_some(j))
            .collect::<Vec<_>>();
        match table.dataset(self.target_column, &features) {
            Ok(data) => {
                self.data = data;
                self.data_name = table.name.clone();
                self.custom = true;
                self.cfg.tree.mtry = self.cfg.tree.mtry.min(self.data.names.len()).max(1);
                self.revision += 1;
                self.fit();
            }
            Err(e) => self.error = e,
        }
    }

    fn save_run(&mut self) {
        let Some(run) = &self.run else {
            return;
        };
        let path = PathBuf::from(self.save_path.trim());
        if self.save_path.trim().is_empty() {
            self.error = "Enter a file path for the saved experiment.".into();
            return;
        }
        match serde_json::to_vec_pretty(&run.to_json())
            .map_err(|e| e.to_string())
            .and_then(|bytes| std::fs::write(&path, bytes).map_err(|e| e.to_string()))
        {
            Ok(()) => self.status = format!("Saved experiment: {}", path.display()),
            Err(e) => self.error = e,
        }
    }
    fn load_file(&mut self, path: PathBuf) {
        if path.extension().is_some_and(|s| s == "json") {
            let result = std::fs::metadata(&path)
                .map_err(|e| e.to_string())
                .and_then(|m| {
                    if m.len() > 32 * 1024 * 1024 {
                        Err("Saved experiment exceeds 32MB.".into())
                    } else {
                        std::fs::read_to_string(&path).map_err(|e| e.to_string())
                    }
                })
                .and_then(|s| serde_json::from_str(&s).map_err(|e| e.to_string()))
                .and_then(|v| Run::from_json(&v, Arc::new(AtomicBool::new(false))));
            match result {
                Ok(run) => {
                    self.cfg = run.config.clone();
                    self.data = run.data.clone();
                    self.data_name = run.name.clone();
                    self.custom = true;
                    self.cancel.store(true, Ordering::Relaxed);
                    self.revision += 1;
                    self.receiver = None;
                    self.due = None;
                    self.stage = run.last_stage();
                    self.run = Some(Arc::new(run));
                    self.table = None;
                    self.status = "Saved experiment reproduced and verified".into();
                    self.code_revision = 0;
                }
                Err(e) => self.error = e,
            }
        } else {
            self.load_csv(path);
        }
    }
    fn header(&mut self, root: &mut egui::Ui) {
        egui::Panel::top("workspace_header")
            .exact_size(108.)
            .show(root, |ui| {
                ui.add_space(10.);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(if self.view == 7 {
                            "Decision landscape"
                        } else if self.view == 6 {
                            "Research workspace"
                        } else {
                            self.cfg.model.descriptor().name
                        })
                        .size(25.)
                        .strong(),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let task = if self.view == 7 {
                            "CONDITIONAL SCENARIOS"
                        } else if self.view == 6 {
                            "SCIENTIFIC ANALYSIS"
                        } else if self.cfg.model.descriptor().task == Task::Regression {
                            "REGRESSION"
                        } else {
                            "BINARY CLASSIFICATION"
                        };
                        ui.label(RichText::new(task).size(10.).color(v::TEAL));
                    });
                });
                ui.label(
                    RichText::new(if self.view == 7 {
                        "Observed evidence and explicit decision assumptions"
                    } else if self.view == 6 {
                        "Explicit estimation, validation and reproducible teaching"
                    } else {
                        self.cfg.model.descriptor().summary
                    })
                    .size(13.)
                    .color(v::MUTED),
                );
                ui.add_space(7.);
                ui.horizontal_wrapped(|ui| {
                    for (i, name) in VIEWS.iter().enumerate() {
                        if ui
                            .push_id(("view", i), |ui| ui.selectable_label(self.view == i, *name))
                            .inner
                            .clicked()
                        {
                            self.view = i;
                        }
                    }
                });
            });
    }
    fn footer(&self, root: &mut egui::Ui) {
        egui::Panel::bottom("workspace_status")
            .exact_size(28.)
            .show(root, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(if self.view == 7 { "Scenario accounting; no fitted model active in this view" } else { &self.status }).size(11.).color(v::MUTED));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new("All computation stays on this computer")
                                .size(11.)
                                .color(v::MUTED),
                        );
                    });
                });
            });
    }
    fn playback(&mut self, ui: &mut egui::Ui, run: &Run) {
        ui.horizontal_wrapped(|ui| {
            if ui
                .button(if self.playing { "Pause" } else { "▶ Replay" })
                .clicked()
            {
                if self.stage == run.last_stage() {
                    self.stage = 0;
                }
                self.playing = !self.playing;
                self.last_tick = ui.input(|i| i.time);
            }
            if ui
                .add_enabled(self.stage > 0, egui::Button::new("Previous"))
                .clicked()
            {
                self.stage -= 1;
                self.playing = false;
            }
            if ui
                .add_enabled(self.stage < run.last_stage(), egui::Button::new("Next"))
                .clicked()
            {
                self.stage += 1;
                self.playing = false;
            }
            let before = self.stage;
            ui.add(egui::Slider::new(&mut self.stage, 0..=run.last_stage()).show_value(false));
            if before != self.stage {
                self.playing = false;
            }
            ui.label(
                RichText::new(format!("State {} / {}", self.stage, run.last_stage()))
                    .monospace()
                    .size(12.),
            );
            if ui.button("Final fit").clicked() {
                self.stage = run.last_stage();
                self.playing = false;
            }
        });
        let s = &run.trace[self.stage];
        ui.label(
            RichText::new(format!(
                "{} · {}",
                run.config.model.descriptor().stage_name,
                s.label
            ))
            .size(12.)
            .color(v::MUTED),
        );
    }
    fn explore(&mut self, ui: &mut egui::Ui, run: &Arc<Run>) {
        let stage = self.stage;
        let predictions = run.predictions(stage);
        let (train, valid) = run.scores(stage, self.threshold);
        v::panel().show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                let loss = if run.task() == Task::Regression {
                    "Training MSE"
                } else {
                    "Training log loss"
                };
                v::metric(ui, loss, &v::number(train.loss), v::TEAL);
                ui.add_space(20.);
                v::metric(
                    ui,
                    if run.task() == Task::Regression {
                        "Validation MSE"
                    } else {
                        "Validation log loss"
                    },
                    &v::number(valid.loss),
                    v::AMBER,
                );
                ui.add_space(20.);
                if run.task() == Task::Regression {
                    v::metric(
                        ui,
                        "Validation R²",
                        &valid.r2.map(v::number).unwrap_or("Undefined".into()),
                        v::BLUE,
                    );
                } else {
                    v::metric(
                        ui,
                        "Validation accuracy",
                        &format!("{:.1}%", valid.accuracy.unwrap() * 100.),
                        v::BLUE,
                    );
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Pin this state").clicked() {
                        self.pinned = Some((run.clone(), stage));
                    }
                });
            });
        });
        ui.add_space(9.);
        self.playback(ui, run);
        ui.add_space(5.);
        ui.horizontal(|ui| {
            ui.label("Inspect feature");
            egui::ComboBox::from_id_salt("explore_feature")
                .selected_text(&run.data.names[self.feature])
                .show_ui(ui, |ui| {
                    for (j, name) in run.data.names.iter().enumerate() {
                        ui.selectable_value(&mut self.feature, j, name);
                    }
                });
        });
        let training = run
            .partition
            .train
            .iter()
            .map(|&i| (run.data.x[i][self.feature], run.data.y[i]))
            .collect();
        let validation = run
            .partition
            .valid
            .iter()
            .map(|&i| (run.data.x[i][self.feature], run.data.y[i]))
            .collect();
        let mut series = vec![
            Series::points(
                "Training",
                training,
                run.partition.train.clone(),
                v::BLUE,
                false,
            ),
            Series::points(
                "Validation",
                validation,
                run.partition.valid.clone(),
                v::AMBER,
                true,
            ),
            Series::line("Fitted slice", run.slice(self.feature, stage), v::TEAL),
        ];
        if run.task() == Task::BinaryClassification {
            let (lo, hi) = run.domain(self.feature);
            series.push(Series::line(
                "Decision threshold",
                vec![(lo, self.threshold), (hi, self.threshold)],
                v::VIOLET,
            ));
        }
        if let Some((pin, s)) = &self.pinned {
            if run.matched(pin) {
                series.push(Series::line(
                    format!("Pinned: {}", pin.config.model.descriptor().name),
                    pin.slice(self.feature, *s),
                    v::RED,
                ));
            }
        }
        v::section(ui,"Predictions and observations","Click an observation to follow its numerical prediction. The line fixes other predictors at their training medians.");
        if let Some(row) = (Plot {
            id: "prediction_slice",
            x_label: &run.data.names[self.feature],
            y_label: if run.task() == Task::Regression {
                "Response / fitted response"
            } else {
                "Class / predicted probability"
            },
            height: 315.,
            series: &series,
            selected: Some(self.selected),
            vertical: None,
            y_range: if run.task() == Task::BinaryClassification {
                Some((-0.08, 1.08))
            } else {
                None
            },
        })
        .show(ui)
        {
            self.selected = row;
        }
        if let Some((pin, s)) = &self.pinned {
            if run.matched(pin) {
                let (_, pv) = pin.scores(*s, self.threshold);
                ui.label(
                    RichText::new(format!(
                        "Matched pinned comparison: {} · validation loss {} (current {})",
                        pin.config.model.descriptor().name,
                        v::number(pv.loss),
                        v::number(valid.loss)
                    ))
                    .color(v::RED)
                    .size(12.),
                );
            } else {
                ui.label(RichText::new("Pinned experiment uses different data, task or rows; no matched score comparison is reported.").size(12.).color(v::AMBER));
            }
        }
        v::panel().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("Observation {}", self.selected + 1)).strong());
                let mut observation = self.selected + 1;
                if ui
                    .add(egui::DragValue::new(&mut observation).range(1..=run.data.y.len()))
                    .changed()
                {
                    self.selected = observation - 1;
                }
                ui.label(if run.partition.train.contains(&self.selected) {
                    "Training row"
                } else {
                    "Validation row"
                });
            });
            ui.horizontal_wrapped(|ui| {
                v::metric(
                    ui,
                    "Observed",
                    &v::number(run.data.y[self.selected]),
                    v::INK,
                );
                ui.add_space(25.);
                v::metric(
                    ui,
                    if run.task() == Task::Regression {
                        "Prediction"
                    } else {
                        "Probability of class 1"
                    },
                    &v::number(predictions[self.selected]),
                    v::TEAL,
                );
                ui.add_space(25.);
                v::metric(
                    ui,
                    "Residual y − prediction",
                    &v::number(run.data.y[self.selected] - predictions[self.selected]),
                    v::AMBER,
                );
            });
            if let Some((intercept, beta)) = run.coefficients(stage) {
                let sum = intercept
                    + beta
                        .iter()
                        .zip(&run.data.x[self.selected])
                        .map(|(b, x)| b * x)
                        .sum::<f64>();
                ui.add_space(5.);
                ui.label(
                    RichText::new(format!(
                        "{} = intercept {} + Σ coefficient × value",
                        if run.task() == Task::Regression {
                            "Prediction"
                        } else {
                            "Log-odds score"
                        },
                        v::number(intercept)
                    ))
                    .monospace()
                    .size(12.),
                );
                ui.horizontal_wrapped(|ui| {
                    for (j, b) in beta.iter().enumerate() {
                        ui.label(
                            RichText::new(format!(
                                "{}: {} × {} = {}",
                                run.data.names[j],
                                v::number(*b),
                                v::number(run.data.x[self.selected][j]),
                                v::number(b * run.data.x[self.selected][j])
                            ))
                            .size(12.)
                            .color(v::PALETTE[j % 6]),
                        );
                    }
                });
                if run.task() == Task::BinaryClassification {
                    ui.label(
                        RichText::new(format!(
                            "sigmoid({}) = {} → predicted class {} at threshold {:.2}",
                            v::number(sum),
                            v::number(predictions[self.selected]),
                            usize::from(predictions[self.selected] >= self.threshold),
                            self.threshold
                        ))
                        .color(v::TEAL),
                    );
                }
            }
        });
        if run.task() == Task::BinaryClassification {
            self.classification_diagnostics(ui, run, &predictions, &valid);
        } else {
            v::section(ui,"Residual diagnostics","A systematic pattern can indicate that the model misses structure. This plot uses actual fitted values for each row.");
            let residual_series = [
                Series::points(
                    "Training residual",
                    run.partition
                        .train
                        .iter()
                        .map(|&i| (predictions[i], run.data.y[i] - predictions[i]))
                        .collect(),
                    run.partition.train.clone(),
                    v::BLUE,
                    false,
                ),
                Series::points(
                    "Validation residual",
                    run.partition
                        .valid
                        .iter()
                        .map(|&i| (predictions[i], run.data.y[i] - predictions[i]))
                        .collect(),
                    run.partition.valid.clone(),
                    v::AMBER,
                    true,
                ),
            ];
            if let Some(row) = (Plot {
                id: "residual_diagnostics",
                x_label: "Fitted response",
                y_label: "Residual",
                height: 245.,
                series: &residual_series,
                selected: Some(self.selected),
                vertical: None,
                y_range: None,
            })
            .show(ui)
            {
                self.selected = row;
            }
            ui.label(
                RichText::new(format!(
                    "Validation RMSE {} · MAE {}",
                    v::number(valid.rmse.unwrap()),
                    v::number(valid.mae.unwrap())
                ))
                .size(12.)
                .color(v::MUTED),
            );
        }
    }
    fn classification_diagnostics(
        &mut self,
        ui: &mut egui::Ui,
        run: &Run,
        predictions: &[f64],
        valid: &experiment::Metrics,
    ) {
        v::section(ui,"Classification diagnostics","Decisions use the selected threshold. Probability scores do not change when the threshold moves.");
        let c = valid.confusion.unwrap();
        v::panel().show(ui, |ui| {
            egui::Grid::new("confusion")
                .spacing(Vec2::new(25., 10.))
                .show(ui, |ui| {
                    ui.label("Validation rows");
                    ui.label("Predicted 0");
                    ui.label("Predicted 1");
                    ui.end_row();
                    ui.label("Actual 0");
                    ui.colored_label(v::TEAL, c[0][0].to_string());
                    ui.colored_label(v::AMBER, c[0][1].to_string());
                    ui.end_row();
                    ui.label("Actual 1");
                    ui.colored_label(v::AMBER, c[1][0].to_string());
                    ui.colored_label(v::TEAL, c[1][1].to_string());
                    ui.end_row();
                });
            ui.label(format!(
                "Brier score {} · ROC AUC {}",
                v::number(valid.brier.unwrap()),
                valid.auc.map(v::number).unwrap_or("Undefined".into())
            ));
        });
        let mut calibration = Vec::new();
        for bin in 0..8 {
            let ids = run
                .partition
                .valid
                .iter()
                .copied()
                .filter(|&i| ((predictions[i] * 8.).floor() as usize).min(7) == bin)
                .collect::<Vec<_>>();
            if !ids.is_empty() {
                calibration.push((
                    ids.iter().map(|&i| predictions[i]).sum::<f64>() / ids.len() as f64,
                    ids.iter().map(|&i| run.data.y[i]).sum::<f64>() / ids.len() as f64,
                ));
            }
        }
        let series = [
            Series::line("Perfect calibration", vec![(0., 0.), (1., 1.)], v::MUTED),
            Series::line("Validation bins", calibration.clone(), v::TEAL),
            Series::points("", calibration, vec![], v::TEAL, false),
        ];
        (Plot {
            id: "calibration",
            x_label: "Mean predicted probability in bin",
            y_label: "Observed fraction of class 1",
            height: 245.,
            series: &series,
            selected: None,
            vertical: None,
            y_range: Some((-0.04, 1.04)),
        })
        .show(ui);
        ui.label(RichText::new("Eight equal-width probability bins; empty bins are omitted. Small-bin variation is not a confidence interval.").size(11.).color(v::MUTED));
    }
    fn path_key(run: &Run) -> String {
        format!(
            "{}:{}:{:?}:{}:{}:{}",
            run.fingerprint,
            run.config.model.key(),
            run.partition,
            run.config.linear.standardize,
            run.config.linear.mix,
            run.config.linear.tolerance
        )
    }
    fn start_path(&mut self, run: Arc<Run>) {
        self.path_cancel.store(true, Ordering::Relaxed);
        self.path_cancel = Arc::new(AtomicBool::new(false));
        let cancel = self.path_cancel.clone();
        let key = Self::path_key(&run);
        let (tx, rx) = mpsc::channel();
        self.path_rx = Some(rx);
        std::thread::spawn(move || {
            let result = experiment::penalty_path(&run, cancel);
            let _ = tx.send((key, result));
        });
    }
    fn coefficients(&mut self, ui: &mut egui::Ui, run: &Arc<Run>) {
        debug_assert_eq!(
            run.config.model.descriptor().coefficients,
            run.coefficients(self.stage).is_some()
        );
        let Some((intercept, original)) = run.coefficients(self.stage) else {
            v::section(ui,"A tree model has no linear coefficient vector","Open the specialist tree laboratory for split diagrams, importance and full tree diagnostics.");
            if let FittedModel::Trees(f) = &run.fitted {
                let mut rules = Vec::new();
                if self.stage > 0 {
                    f.trees[self.stage - 1].rules(&run.data.names, String::new(), &mut rules);
                }
                for rule in rules.iter().take(100) {
                    ui.monospace(rule);
                }
            }
            return;
        };
        v::section(
            ui,
            "Coefficient interpretation",
            "The selected state and preprocessing convention determine the values below.",
        );
        self.playback(ui, run);
        ui.checkbox(
            &mut self.standardized,
            "Show coefficients per training standard deviation",
        );
        let values = if self.standardized {
            if let FittedModel::Linear(f) = &run.fitted {
                f.step(self.stage).beta.clone()
            } else {
                original.clone()
            }
        } else {
            original.clone()
        };
        v::panel().show(ui, |ui| {
            v::coefficient_bars(ui, &run.data.names, &values, &mut self.feature);
            ui.separator();
            ui.label(format!(
                "Intercept in original predictor units: {}",
                v::number(intercept)
            ));
        });
        if let FittedModel::Linear(f) = &run.fitted {
            ui.label(
                RichText::new(format!(
                    "Numerical predictor rank {} / {} · maximum stationarity residual {}",
                    f.rank,
                    run.data.names.len(),
                    v::number(f.step(self.stage).stationarity)
                ))
                .size(12.)
                .color(v::MUTED),
            );
            ui.add_space(8.);
            egui::Grid::new("coefficient_table")
                .striped(true)
                .spacing(Vec2::new(20., 8.))
                .show(ui, |ui| {
                    for h in [
                        "Feature",
                        "Original slope",
                        "Training mean",
                        "Training scale",
                    ] {
                        ui.label(RichText::new(h).strong());
                    }
                    ui.end_row();
                    for (j, name) in run.data.names.iter().enumerate() {
                        ui.label(name);
                        ui.monospace(v::number(original[j]));
                        ui.monospace(v::number(f.transform.means[j]));
                        ui.monospace(v::number(f.transform.scales[j]));
                        ui.end_row();
                    }
                });
        }
        if run.config.model.descriptor().penalty_path {
            v::section(ui,"Regularization path","Each point is a separate fitted model on this experiment’s training partition. The x axis is log₁₀(α).");
            let key = Self::path_key(run);
            let available = self.path.as_ref().is_some_and(|p| p.0 == key);
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(
                        self.path_rx.is_none(),
                        egui::Button::new(if available {
                            "Recompute path"
                        } else {
                            "Trace penalty path"
                        }),
                    )
                    .clicked()
                {
                    self.start_path(run.clone());
                }
                if self.path_rx.is_some() {
                    ui.spinner();
                    ui.label("Fitting the penalty grid in background");
                }
            });
            if let Some((stored, points)) = &self.path {
                if *stored == key {
                    let series = run
                        .data
                        .names
                        .iter()
                        .enumerate()
                        .map(|(j, name)| {
                            Series::line(
                                name.clone(),
                                points
                                    .iter()
                                    .map(|p| {
                                        (
                                            p.alpha.log10(),
                                            if self.standardized {
                                                p.standardized[j]
                                            } else {
                                                p.original[j]
                                            },
                                        )
                                    })
                                    .collect(),
                                v::PALETTE[j % 6],
                            )
                        })
                        .collect::<Vec<_>>();
                    (Plot {
                        id: "penalty_coefficient_path",
                        x_label: "log₁₀(penalty α)",
                        y_label: if self.standardized {
                            "Standardized coefficient"
                        } else {
                            "Original-unit coefficient"
                        },
                        height: 290.,
                        series: &series,
                        selected: None,
                        vertical: if run.config.linear.alpha > 0. {
                            Some(run.config.linear.alpha.log10())
                        } else {
                            None
                        },
                        y_range: None,
                    })
                    .show(ui);
                    let errors = [
                        Series::line(
                            "Training MSE",
                            points
                                .iter()
                                .map(|p| (p.alpha.log10(), p.train_loss))
                                .collect(),
                            v::TEAL,
                        ),
                        Series::line(
                            "Validation MSE",
                            points
                                .iter()
                                .map(|p| (p.alpha.log10(), p.valid_loss))
                                .collect(),
                            v::AMBER,
                        ),
                    ];
                    (Plot {
                        id: "penalty_validation",
                        x_label: "log₁₀(penalty α)",
                        y_label: "Mean squared error",
                        height: 235.,
                        series: &errors,
                        selected: None,
                        vertical: if run.config.linear.alpha > 0. {
                            Some(run.config.linear.alpha.log10())
                        } else {
                            None
                        },
                        y_range: None,
                    })
                    .show(ui);
                    let failed = points.iter().filter(|p| !p.converged).count();
                    if failed > 0 {
                        ui.colored_label(v::AMBER,format!("{failed} path fits did not meet the convergence tolerance. Their plotted values are provisional and excluded from automatic selection."));
                    }
                    if let Some(best) = points
                        .iter()
                        .filter(|p| p.converged)
                        .min_by(|a, b| a.valid_loss.total_cmp(&b.valid_loss))
                    {
                        let alpha = best.alpha;
                        let best_loss = best.valid_loss;
                        ui.horizontal(|ui| {
                            ui.label(format!(
                                "Best converged grid value: α {} · validation MSE {}",
                                v::number(alpha),
                                v::number(best_loss)
                            ));
                            if ui.button("Use this α").clicked() {
                                self.cfg.linear.alpha = alpha;
                                self.dirty(ui.input(|i| i.time));
                            }
                        });
                    }
                }
            }
        } else if run.task() == Task::BinaryClassification {
            ui.add_space(12.);
            ui.label("A logistic slope changes log odds. Exponentiating it gives an odds ratio; the corresponding probability change depends on the starting score.");
        }
    }
    fn optimization(&mut self, ui: &mut egui::Ui, run: &Run) {
        v::section(ui,"Computation and convergence","Replay actual recorded states. Iteration numbers are solver iterations; no interpolated optimizer states are fabricated.");
        self.playback(ui, run);
        let s = &run.trace[self.stage];
        v::panel().show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                v::metric(ui, "Objective", &v::number(s.objective), v::TEAL);
                ui.add_space(20.);
                v::metric(ui, "Actual iteration", &s.iteration.to_string(), v::BLUE);
                if let Some(residual) = s.stationarity {
                    ui.add_space(20.);
                    v::metric(ui, "Stationarity / KKT", &v::number(residual), v::AMBER);
                }
            });
        });
        // Linear fits record the objective they minimise. Tree ensembles record
        // training MSE/2 per stage; a forest minimises no such quantity, so the
        // label must not call it an objective.
        let objective_label = match &run.fitted {
            FittedModel::Linear(_) => "Training objective",
            FittedModel::Trees(_) => "Training loss (MSE/2, not a minimised objective)",
        };
        let series = [Series::line(
            objective_label,
            run.trace
                .iter()
                .map(|s| (s.iteration as f64, s.objective))
                .collect(),
            v::TEAL,
        )];
        (Plot {
            id: "objective_path",
            x_label: "Actual projection / solver iteration / tree",
            y_label: objective_label,
            height: 265.,
            series: &series,
            selected: None,
            vertical: Some(s.iteration as f64),
            y_range: None,
        })
        .show(ui);
        let metric = if run.task() == Task::Regression {
            "Mean squared error"
        } else {
            "Log loss"
        };
        let errors = [
            Series::line(
                "Training",
                run.trace
                    .iter()
                    .map(|s| (s.iteration as f64, s.train_loss))
                    .collect(),
                v::TEAL,
            ),
            Series::line(
                "Validation",
                run.trace
                    .iter()
                    .map(|s| (s.iteration as f64, s.valid_loss))
                    .collect(),
                v::AMBER,
            ),
        ];
        (Plot {
            id: "loss_path",
            x_label: "Actual projection / solver iteration / tree",
            y_label: metric,
            height: 240.,
            series: &errors,
            selected: None,
            vertical: Some(s.iteration as f64),
            y_range: None,
        })
        .show(ui);
        if let FittedModel::Linear(fit) = &run.fitted {
            ui.label(RichText::new(if fit.converged{"Final fit satisfied its numerical convergence check."}else{"Final fit did not establish convergence. Inspect the warning and residual before using the estimates."}).color(if fit.converged{v::TEAL}else{v::AMBER}));
            if run.data.names.len() >= 2 {
                self.objective_surface(ui, run, fit);
            }
        }
    }
    fn objective_surface(&mut self, ui: &mut egui::Ui, run: &Run, fit: &linear::LinearFit) {
        let scale_note = if fit.config.standardize {
            "The first two standardized coefficients vary"
        } else {
            "The first two centered (unstandardized) coefficients vary"
        };
        v::section(ui,"A slice through the objective",&format!("{scale_note}; the intercept and all other coefficients stay at the selected state."));
        let state = fit.step(self.stage);
        let first = state.beta[0];
        let second = state.beta[1];
        let end = fit.step(run.last_stage());
        let extent = first
            .abs()
            .max(second.abs())
            .max(end.beta[0].abs())
            .max(end.beta[1].abs())
            .max(0.7)
            * 1.5;
        let key = format!(
            "{}:{}:{}:{}",
            self.revision,
            self.stage,
            run.fingerprint,
            run.config.model.key()
        );
        let resolution = 33;
        if self.surface_cache.as_ref().is_none_or(|c| c.0 != key) {
            let x = run
                .partition
                .train
                .iter()
                .map(|&i| fit.transform.row(&run.data.x[i]))
                .collect::<Vec<_>>();
            let y = run
                .partition
                .train
                .iter()
                .map(|&i| run.data.y[i])
                .collect::<Vec<_>>();
            let mut cells = Vec::new();
            let mut lo = f64::INFINITY;
            let mut hi = f64::NEG_INFINITY;
            for j in 0..resolution {
                for k in 0..resolution {
                    let mut b = state.beta.clone();
                    b[0] = -extent + 2. * extent * k as f64 / (resolution - 1) as f64;
                    b[1] = -extent + 2. * extent * j as f64 / (resolution - 1) as f64;
                    let value = linear::objective(&x, &y, state.intercept, &b, &fit.config);
                    lo = lo.min(value);
                    hi = hi.max(value);
                    cells.push((k, j, value));
                }
            }
            self.surface_cache = Some((key, extent, lo, hi, cells));
        }
        let (_, _, lo, hi, cells) = self.surface_cache.as_ref().unwrap();
        let width = ui.available_width().min(620.);
        let (response, p) = ui.allocate_painter(Vec2::new(width, 350.), egui::Sense::hover());
        let rect = response.rect;
        let graph = egui::Rect::from_min_max(
            rect.min + Vec2::new(62., 20.),
            rect.max - Vec2::new(20., 45.),
        );
        let cw = graph.width() / resolution as f32;
        let ch = graph.height() / resolution as f32;
        for &(k, j, value) in cells {
            let t = ((value - lo) / (hi - lo).max(1e-14)).sqrt().clamp(0., 1.) as f32;
            let color = Color32::from_rgb(
                (25. + t * 52.) as u8,
                (114. - t * 75.) as u8,
                (116. - t * 42.) as u8,
            );
            let min = egui::Pos2::new(
                graph.left() + k as f32 * cw,
                graph.bottom() - (j + 1) as f32 * ch,
            );
            p.rect_filled(
                egui::Rect::from_min_size(min, Vec2::new(cw + 0.5, ch + 0.5)),
                0.,
                color,
            );
        }
        let map = |a: f64, b: f64| {
            egui::Pos2::new(
                graph.left() + ((a + extent) / (2. * extent)) as f32 * graph.width(),
                graph.bottom() - ((b + extent) / (2. * extent)) as f32 * graph.height(),
            )
        };
        p.line_segment(
            [map(-extent, 0.), map(extent, 0.)],
            egui::Stroke::new(0.7, v::MUTED),
        );
        p.line_segment(
            [map(0., -extent), map(0., extent)],
            egui::Stroke::new(0.7, v::MUTED),
        );
        p.circle_filled(map(first, second), 5., v::AMBER);
        p.circle_stroke(map(first, second), 9., egui::Stroke::new(1.5, v::INK));
        for k in 0..=4 {
            let value = -extent + 2. * extent * k as f64 / 4.;
            let a = map(value, -extent);
            let b = map(-extent, value);
            p.text(
                a + Vec2::new(0., 7.),
                egui::Align2::CENTER_TOP,
                format!("{value:.1}"),
                egui::FontId::proportional(11.),
                v::MUTED,
            );
            p.text(
                b - Vec2::new(8., 0.),
                egui::Align2::RIGHT_CENTER,
                format!("{value:.1}"),
                egui::FontId::proportional(11.),
                v::MUTED,
            );
        }
        p.text(
            egui::Pos2::new(graph.center().x, rect.bottom()),
            egui::Align2::CENTER_BOTTOM,
            format!("{} coefficient", run.data.names[0]),
            egui::FontId::proportional(12.),
            v::MUTED,
        );
        p.text(
            rect.min,
            egui::Align2::LEFT_TOP,
            format!("{} coefficient", run.data.names[1]),
            egui::FontId::proportional(11.),
            v::MUTED,
        );
        ui.label(RichText::new(format!("Teal: lower objective ({}) · dark: higher ({}) · amber: selected state. This is a conditional surface, not a projection of the complete optimizer trajectory.",v::number(*lo),v::number(*hi))).size(11.).color(v::MUTED));
    }
    fn learn(&mut self, ui: &mut egui::Ui, run: &Run) {
        let ModelId::Linear(method) = run.config.model else {
            v::section(ui,"Tree and ensemble lessons","Use the complete tree laboratory for its guided experiments and 100 original practice questions.");
            if ui.button("Open tree laboratory").clicked() {
                self.legacy_open = true;
            }
            return;
        };
        let lesson = teaching::lesson(method);
        v::section(ui, lesson.title, lesson.objective);
        v::panel().show(ui, |ui| {
            ui.label(
                RichText::new(lesson.equation)
                    .monospace()
                    .size(15.)
                    .color(v::TEAL),
            );
        });
        ui.add_space(12.);
        ui.horizontal_wrapped(|ui| {
            for (i, section) in lesson.sections.iter().enumerate() {
                if ui
                    .selectable_label(
                        self.lesson_step == i,
                        format!("{}  {}", i + 1, section.title),
                    )
                    .clicked()
                {
                    self.lesson_step = i;
                }
            }
        });
        self.lesson_step = self.lesson_step.min(lesson.sections.len() - 1);
        let section = &lesson.sections[self.lesson_step];
        v::section(ui, section.title, "");
        ui.label(section.body);
        ui.add_space(10.);
        v::panel().show(ui, |ui| {
            ui.label(
                RichText::new("Try it in this experiment")
                    .strong()
                    .color(v::AMBER),
            );
            ui.label(section.experiment);
            if ui.button("Explore the current fit").clicked() {
                self.view = 0;
            }
        });
        if let FittedModel::Linear(fit) = &run.fitted {
            let row = self.selected.min(run.data.y.len() - 1);
            let state = fit.step(self.stage);
            let z = fit.transform.row(&run.data.x[row]);
            let eta = state.intercept + z.iter().zip(&state.beta).map(|(x, b)| x * b).sum::<f64>();
            let prediction = run.predict(&run.data.x[row], self.stage);
            v::section(
                ui,
                "Arithmetic from the selected observation",
                "These values come from the current fitted experiment and recorded state.",
            );
            v::panel().show(ui, |ui| {
                ui.label(format!(
                    "Observation {} · recorded state {} · observed outcome {}",
                    row + 1,
                    self.stage,
                    v::number(run.data.y[row])
                ));
                ui.monospace(format!(
                    "Centered-design intercept = {}",
                    v::number(state.intercept)
                ));
                for (j, name) in run.data.names.iter().enumerate() {
                    ui.monospace(format!(
                        "{}: transformed x {} × coefficient {} = {}",
                        name,
                        v::number(z[j]),
                        v::number(state.beta[j]),
                        v::number(z[j] * state.beta[j])
                    ));
                }
                ui.label(format!("Intercept + contributions = {}", v::number(eta)));
                if method == LinearMethod::Logistic {
                    let loss = linear::logistic_loss(eta, run.data.y[row]);
                    ui.label(format!(
                        "sigmoid({}) = {} · observed-label log loss {}",
                        v::number(eta),
                        v::number(prediction),
                        v::number(loss)
                    ));
                    ui.label(format!(
                        "At threshold {}, predicted class = {}",
                        v::number(self.threshold),
                        usize::from(prediction >= self.threshold)
                    ));
                } else {
                    let residual = run.data.y[row] - prediction;
                    ui.label(format!(
                        "Residual = {} − {} = {} · squared residual = {}",
                        v::number(run.data.y[row]),
                        v::number(prediction),
                        v::number(residual),
                        v::number(residual * residual)
                    ));
                }
                if method.penalized() {
                    let l1 = state.beta.iter().map(|v| v.abs()).sum::<f64>();
                    let l2 = state.beta.iter().map(|v| v * v).sum::<f64>();
                    let mix = if method == LinearMethod::Ridge {
                        0.
                    } else if method == LinearMethod::Lasso {
                        1.
                    } else {
                        fit.config.mix
                    };
                    let penalty = fit.config.alpha * (mix * l1 + (1. - mix) * l2 / 2.);
                    ui.label(format!(
                        "Training objective {} = mean squared error / 2 + penalty {}",
                        v::number(state.objective),
                        v::number(penalty)
                    ));
                    ui.label("The penalty uses transformed slopes and excludes the intercept.");
                }
            });
        }
        ui.add_space(20.);
        v::section(
            ui,
            "Reasoning checks",
            "Choose an answer, then check the explanation. These are original practice questions.",
        );
        for (i, q) in lesson.questions.iter().enumerate() {
            v::panel().show(ui, |ui| {
                ui.label(RichText::new(format!("{}. {}", i + 1, q.prompt)).strong());
                for (j, option) in q.options.iter().enumerate() {
                    ui.radio_value(&mut self.quiz_answer[i], Some(j), *option);
                }
                if ui
                    .add_enabled(
                        self.quiz_answer[i].is_some(),
                        egui::Button::new("Check answer"),
                    )
                    .clicked()
                {
                    self.quiz_checked[i] = true;
                }
                if self.quiz_checked[i] {
                    let correct = self.quiz_answer[i] == Some(q.answer);
                    ui.colored_label(
                        if correct { v::TEAL } else { v::AMBER },
                        if correct {
                            "Correct."
                        } else {
                            "Review the reasoning."
                        },
                    );
                    ui.label(q.explanation);
                }
            });
            ui.add_space(8.);
        }
    }
    fn code_view(&mut self, ui: &mut egui::Ui, run: &Run) {
        v::section(ui,"Executable Python","The generated program uses this experiment’s actual data and row partition. Library fits are distinct from native solver traces.");
        if self.code_revision != self.revision && !self.code.running {
            self.code.draft = crate::codegen::python(run);
            self.code_revision = self.revision;
        }
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    !self.code.running,
                    egui::Button::new("Run Python locally").fill(Color32::from_rgb(35, 82, 78)),
                )
                .clicked()
            {
                self.code.launch(
                    self.code.draft.clone(),
                    run.data.clone(),
                    run.config.model.descriptor().name.into(),
                );
            }
            if ui.button("Restore generated code").clicked() {
                self.code.draft = crate::codegen::python(run);
            }
            if ui.button("Copy code").clicked() {
                ui.ctx().copy_text(self.code.draft.clone());
            }
            if ui
                .add_enabled(self.code.running, egui::Button::new("Stop"))
                .clicked()
            {
                if let Some(s) = &mut self.code.session {
                    s.stop();
                }
                self.code.running = false;
            }
            ui.checkbox(&mut self.code.step, "Pause after library fit");
            if self.code.waiting && ui.button("Continue Python").clicked() {
                if let Some(s) = &mut self.code.session {
                    match s.next() {
                        Ok(()) => self.code.waiting = false,
                        Err(e) => self.code.message = e,
                    }
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label("Python interpreter");
            ui.add(egui::TextEdit::singleline(&mut self.code.interpreter).desired_width(430.));
        });
        ui.label(RichText::new("Code executes with your local account’s file access. Only run code you intend to execute.").size(11.).color(v::MUTED));
        egui::ScrollArea::both()
            .id_salt("linear_code_editor")
            .max_height(350.)
            .show(ui, |ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut self.code.draft)
                        .font(egui::TextStyle::Monospace)
                        .code_editor()
                        .desired_width(f32::INFINITY)
                        .desired_rows(18),
                );
            });
        ui.label(RichText::new(&self.code.message).color(v::TEAL));
        if !self.code.predictions.is_empty() {
            let selected = self.selected.min(self.code.predictions.len() - 1);
            ui.label(format!(
                "Python prediction for observation {}: {}",
                selected + 1,
                v::number(self.code.predictions[selected])
            ));
        }
        ui.collapsing("Python output", |ui| {
            for line in &self.code.output {
                ui.monospace(line);
            }
        });
        if !self.code.train.is_empty() {
            let s = [
                Series::line("Python training", self.code.train.clone(), v::TEAL),
                Series::line("Python validation", self.code.valid.clone(), v::AMBER),
            ];
            (Plot {
                id: "python_results",
                x_label: "Completed library fit",
                y_label: &self.code.metric_label,
                height: 210.,
                series: &s,
                selected: None,
                vertical: None,
                y_range: None,
            })
            .show(ui);
        }
    }
    fn data_view(&mut self, ui: &mut egui::Ui, run: &Run) {
        v::section(
            ui,
            &run.name,
            "The table retains observation identity and the actual train/validation assignment.",
        );
        ui.label(format!(
            "{} rows · {} predictors · response: {}",
            run.data.y.len(),
            run.data.names.len(),
            run.data.target
        ));
        ui.label(
            RichText::new(format!(
                "{} · {} partition",
                run.fingerprint,
                if run.partition.stratified {
                    "Class-stratified"
                } else {
                    "Seeded random"
                }
            ))
            .monospace()
            .size(11.)
            .color(v::MUTED),
        );
        if let Some(table) = &self.table {
            let mut apply = false;
            v::panel().show(ui,|ui|{
                ui.strong("Column roles");
                egui::ComboBox::from_id_salt("response_column").selected_text(&table.headers[self.target_column]).show_ui(ui,|ui|{
                    for (j,name) in table.headers.iter().enumerate(){if table.numeric[j]{ui.selectable_value(&mut self.target_column,j,name);}}
                });
                ui.label("Select numeric predictors; the response is always excluded.");
                ui.horizontal_wrapped(|ui|{for(j,name)in table.headers.iter().enumerate(){if table.numeric[j]&&j!=self.target_column{ui.checkbox(&mut self.feature_columns[j],name);}}});
                if ui.button("Apply column roles and fit").clicked(){apply=true;}
                ui.label(RichText::new("Non-numeric columns are not encoded. Missing or nonfinite values are never silently imputed.").size(11.).color(v::MUTED));
            });
            if apply {
                self.apply_columns();
            }
        }

        ui.add_space(10.);
        egui::ScrollArea::horizontal()
            .id_salt("data_horizontal")
            .show(ui, |ui| {
                egui::Grid::new("data_table")
                    .striped(true)
                    .min_col_width(80.)
                    .show(ui, |ui| {
                        ui.strong("Observation");
                        ui.strong("Partition");
                        for name in &run.data.names {
                            ui.strong(name);
                        }
                        ui.strong(&run.data.target);
                        ui.end_row();
                        for i in 0..run.data.y.len().min(200) {
                            if ui
                                .selectable_label(self.selected == i, format!("{}", i + 1))
                                .clicked()
                            {
                                self.selected = i;
                            }
                            ui.colored_label(
                                if run.partition.train.contains(&i) {
                                    v::BLUE
                                } else {
                                    v::AMBER
                                },
                                if run.partition.train.contains(&i) {
                                    "Training"
                                } else {
                                    "Validation"
                                },
                            );
                            for value in &run.data.x[i] {
                                ui.monospace(v::number(*value));
                            }
                            ui.monospace(v::number(run.data.y[i]));
                            ui.end_row();
                        }
                    });
            });
        if run.data.y.len() > 200 {
            ui.label(RichText::new("Table preview shows the first 200 rows. The numerical fit uses the complete selected dataset.").size(11.).color(v::MUTED));
        }
    }

    #[cfg(feature = "acceptance")]
    fn study_workflow_smoke(&mut self, ctx: &egui::Context) {
        ctx.request_repaint_after(std::time::Duration::from_millis(20));
        let mut state = self
            .study_smoke_state
            .take()
            .unwrap_or_else(|| StudyNativeCheck {
                step: 0,
                frames: 0,
                directory: std::env::temp_dir().join(format!(
                    "model-lab-native-study-{}",
                    crate::adapter::identity()
                )),
                started: std::time::Instant::now(),
            });
        self.view = 6;
        self.research.tab = 5;
        let result = (|| -> Result<(), String> {
            if state.started.elapsed() > std::time::Duration::from_secs(150) {
                return Err("Native study workflow deadline exceeded.".into());
            }
            if !self.research.studies.error.is_empty() {
                return Err(self.research.studies.error.clone());
            }
            if self.research.studies.busy() {
                return Ok(());
            }
            state.frames += 1;
            match state.step {
                0 => {
                    std::fs::create_dir(&state.directory).map_err(|e| e.to_string())?;
                    self.research.project_path = state
                        .directory
                        .join("Native study.json")
                        .to_string_lossy()
                        .into_owned();
                    self.research.studies.draft.replications = 4;
                    self.research.studies.draft_prediction="Automated fixture, not a human response: HC3 changes uncertainty, not the point estimate.".into();
                    self.research.save()?;
                    state.step = 1;
                    state.frames = 0;
                }
                1 if state.frames >= 3 => {
                    self.research
                        .studies
                        .preview(self.research.interpreter.clone())?;
                    state.step = 2;
                    state.frames = 0;
                }
                2 if state.frames >= 3 => {
                    self.research.studies.add_study()?;
                    self.research.save()?;
                    self.research
                        .studies
                        .run_selected(false, self.research.interpreter.clone())?;
                    state.step = 3;
                    state.frames = 0;
                }
                3 if state.frames >= 4 => {
                    if self
                        .research
                        .studies
                        .current_document()
                        .is_none_or(|d| d.status != "completed")
                    {
                        return Err("Native study did not complete.".into());
                    }
                    self.research.studies.replicate = 0;
                    self.research.studies.method_id =
                        self.research.studies.current().unwrap().spec.methods[0]
                            .id
                            .clone();
                    self.research
                        .studies
                        .inspect(self.research.interpreter.clone())?;
                    state.step = 4;
                    state.frames = 0;
                }
                4 if state.frames >= 3 => {
                    if !self
                        .research
                        .studies
                        .inspection
                        .as_ref()
                        .is_some_and(|v| v["verified"] == true)
                    {
                        return Err("Native replication inspection failed.".into());
                    }
                    self.research.studies.stage = 4;
                    self.research.studies.reveal = true;
                    self.research.save()?;
                    let saved: serde_json::Value = serde_json::from_slice(
                        &std::fs::read(&self.research.project_path).map_err(|e| e.to_string())?,
                    )
                    .map_err(|e| e.to_string())?;
                    self.research.restore(saved)?;
                    self.research.studies.stage = 4;
                    self.research.studies.reveal = true;
                    state.step = 5;
                    state.frames = 0;
                }
                5 if state.frames >= 3 => {
                    self.research.studies.audience = "student".into();
                    self.research.studies.export_path = state
                        .directory
                        .join("Student packet")
                        .to_string_lossy()
                        .into_owned();
                    self.research
                        .studies
                        .export(self.research.interpreter.clone())?;
                    state.step = 6;
                    state.frames = 0;
                }
                6 if state.frames >= 3 => {
                    if state.directory.join("Student packet/study.json").exists()
                        || !state
                            .directory
                            .join("Student packet/worksheet.txt")
                            .exists()
                    {
                        return Err("Native student export capability check failed.".into());
                    }
                    self.research.studies.audience = "research".into();
                    self.research.studies.export_path = state
                        .directory
                        .join("Research packet")
                        .to_string_lossy()
                        .into_owned();
                    self.research
                        .studies
                        .export(self.research.interpreter.clone())?;
                    state.step = 7;
                    state.frames = 0;
                }
                7 if state.frames >= 3 => {
                    if !state
                        .directory
                        .join("Research packet/reproduce.py")
                        .exists()
                    {
                        return Err("Native research export missing.".into());
                    }
                    self.research.studies.stage = 2;
                    state.step = 8;
                    state.frames = 0;
                }
                _ => {}
            }
            Ok(())
        })();
        if let Err(error) = result {
            if let Some(path) = crate::acceptance::argument("--report") {
                let _ = crate::acceptance::write(
                    &path,
                    &serde_json::json!({"status":"failed","workflow_status":"failed","step":state.step,"error":error,"capture_status":"not_attempted"}),
                );
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        for event in ctx.input(|i| i.events.clone()) {
            if let egui::Event::Screenshot { image, .. } = event {
                if let Some(path) = crate::acceptance::argument("--screenshot") {
                    self.smoke_image = crate::acceptance::save_png(&path, &image).is_ok();
                }
            }
        }
        if state.step == 8 && state.frames >= 3 {
            let gate = self.capture.get_or_insert_with(Default::default);
            if !self.smoke_image {
                gate.tick(&ctx);
            }
            if self.smoke_image || gate.expired() {
                if let Some(path) = crate::acceptance::argument("--report") {
                    let report = serde_json::json!({"status":if self.smoke_image{"passed"}else{"partial"},"workflow_status":"passed","application_liveness":true,"os":std::env::consts::OS,"engine_version":env!("CARGO_PKG_VERSION"),"views_built":["Specify","Sample","Results","Replication","Teach & export"],"actual_worker_jobs":["preview","simulate","regenerate","student-export","research-export"],"project_v2_restored":true,"instructor_view_built":true,"capture_status":if self.smoke_image{"captured"}else{"timed_out"},"screenshot":self.smoke_image,"capture_attempts":gate.attempts,"scope":"Automated native workflow; not a participant study or accessibility certification."});
                    let _ = crate::acceptance::write(&path, &report);
                }
                let _ = std::fs::remove_dir_all(&state.directory);
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
        self.study_smoke_state = Some(state);
    }
    #[cfg(feature = "acceptance")]
    fn research_workflow_smoke(&mut self, ctx: &egui::Context) {
        ctx.request_repaint_after(std::time::Duration::from_millis(16));
        let (mut step, mut frames, directory) =
            self.research_smoke_state.take().unwrap_or_else(|| {
                (
                    0,
                    0,
                    std::env::temp_dir().join(format!(
                        "model-lab-native-research-{}",
                        crate::adapter::identity()
                    )),
                )
            });
        let result = (|| -> Result<(), String> {
            if !self.research.error.is_empty() {
                return Err(self.research.error.clone());
            }
            self.view = 6;
            if self.research.busy() {
                return Ok(());
            }
            frames += 1;
            match step {
                0 => {
                    let Some(run) = self.run.as_ref() else {
                        return Ok(());
                    };
                    self.research
                        .synchronize(run, self.table.as_ref(), self.revision);
                    if frames >= 3 {
                        std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
                        let mut q = self.research.template.clone();
                        q["operation"] = serde_json::json!("inference");
                        self.research.launch(q)?;
                        step = 1;
                        frames = 0;
                    }
                }
                1 if frames >= 3 => {
                    self.research.select_model("ridge");
                    self.research.tab = 2;
                    let mut q = self.research.template.clone();
                    q["operation"] = serde_json::json!("resample");
                    self.research.launch(q)?;
                    step = 2;
                    frames = 0;
                }
                2 if frames >= 3 => {
                    self.research.tab = 3;
                    self.research.project_path = directory
                        .join("native-project.json")
                        .to_string_lossy()
                        .into_owned();
                    self.research.save()?;
                    let value = serde_json::from_slice(
                        &std::fs::read(&self.research.project_path).map_err(|e| e.to_string())?,
                    )
                    .map_err(|e| e.to_string())?;
                    self.research.restore(value)?;
                    step = 3;
                    frames = 0;
                }
                3 if frames >= 3 => {
                    self.research.show_teacher();
                    step = 4;
                    frames = 0;
                }
                4 if frames >= 3 => {
                    self.research.tab = 3;
                    let analysis = &self.research.history[0];
                    let mut q = analysis["request"].clone();
                    q["operation"] = serde_json::json!("export");
                    q["source_request"] = analysis["request"].clone();
                    q["source_response"] = analysis["response"].clone();
                    q["source_wire"] = serde_json::json!(serde_json::to_string_pretty(
                        &analysis["request"]
                    )
                    .unwrap());
                    q["export_path"] = serde_json::json!(directory.join("Native research export"));
                    self.research.launch(q)?;
                    step = 5;
                    frames = 0;
                }
                5 if frames >= 3 => {
                    if !directory
                        .join("Native research export/figure.pdf")
                        .is_file()
                    {
                        return Err("Native export did not produce the research figure.".into());
                    }
                    self.research.selected = 0;
                    self.research.template = self.research.history[0]["request"].clone();
                    self.research.status = "Native research and teaching workflow verified.".into();
                    self.research.tab = 1;
                    step = 6;
                    frames = 0;
                }
                6 if frames == 3 => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(
                        egui::UserData::default(),
                    ));
                }
                _ => {}
            }
            Ok(())
        })();
        if let Err(error) = result {
            if let Some(path) = crate::acceptance::argument("--report") {
                let _ = crate::acceptance::write(
                    &path,
                    &serde_json::json!({"status":"failed","error":error,"step":step}),
                );
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        for event in ctx.input(|i| i.events.clone()) {
            if let egui::Event::Screenshot { image, .. } = event {
                if let Some(path) = crate::acceptance::argument("--screenshot") {
                    self.smoke_image = crate::acceptance::save_png(&path, &image).is_ok();
                }
            }
        }
        if step == 6 && !self.smoke_image {
            let gate = self.capture.get_or_insert_with(Default::default);
            gate.tick(&ctx);
            if gate.expired() {
                if let Some(path) = crate::acceptance::argument("--report") {
                    let _ = crate::acceptance::write(
                        &path,
                        &serde_json::json!({"status":"partial","workflow_status":"passed","application_liveness":true,"capture_status":"timed_out","screenshot":false}),
                    );
                }
                let _ = std::fs::remove_dir_all(&directory);
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
        if step == 6 && self.smoke_image {
            if let Some(path) = crate::acceptance::argument("--report") {
                let report = serde_json::json!({"status":"passed","os":std::env::consts::OS,"engine_version":env!("CARGO_PKG_VERSION"),"research_views":["Design","Inference","Validation","Project","Teach"],"actual_worker_jobs":["inference","nested-resampling","research-export"],"project_saved_and_restored":true,"completed_analyses":self.research.history.len(),"instructor_view_rendered":true,"screenshot":true});
                if crate::acceptance::write(&path, &report).is_err() {
                    return;
                }
            }
            let _ = std::fs::remove_dir_all(&directory);
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        self.research_smoke_state = Some((step, frames, directory));
    }
}
impl Workbench {
    /// Native smoke-test workflows driven by `--workbench-ui-smoke-test`,
    /// `--research-ui-smoke-test` and `--study-ui-smoke-test`.
    #[cfg(feature = "acceptance")]
    fn smoke_hooks(&mut self, ctx: &egui::Context) {
        if std::env::args().any(|a| a == "--study-ui-smoke-test") && self.run.is_some() {
            self.study_workflow_smoke(&ctx);
        }
        if std::env::args().any(|a| a == "--research-ui-smoke-test") && self.run.is_some() {
            self.research_workflow_smoke(&ctx);
        }
        if std::env::args().any(|a| a == "--workbench-ui-smoke-test") && self.run.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
            let model_frames = VIEWS.len() * 2;
            let total = experiment::MODELS.len() * model_frames;
            if self.receiver.is_none() && self.due.is_none() {
                self.smoke_frame += 1;
                if self.smoke_frame <= total {
                    self.view = ((self.smoke_frame - 1) % model_frames) / 2;
                    if self.smoke_frame % model_frames == 0 && self.smoke_frame < total {
                        let model = experiment::MODELS[self.smoke_frame / model_frames].id;
                        self.select_model(model, ctx.input(|i| i.time));
                        self.fit();
                    }
                }
                if self.smoke_frame == total + 1 {
                    self.view = 0;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(
                        egui::UserData::default(),
                    ));
                }
            }
            if self.smoke_frame > total && !self.smoke_image {
                let gate = self.capture.get_or_insert_with(Default::default);
                gate.tick(&ctx);
                if gate.expired() {
                    if let Some(path) = crate::acceptance::argument("--report") {
                        let _ = crate::acceptance::write(
                            &path,
                            &serde_json::json!({"status":"partial","workflow_status":"passed","application_liveness":true,"views_rendered":VIEWS,"capture_status":"timed_out","screenshot":false,"native_frames":self.smoke_frame}),
                        );
                    }
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
            for event in ctx.input(|i| i.events.clone()) {
                if let egui::Event::Screenshot { image, .. } = event {
                    if let Some(path) = crate::acceptance::argument("--screenshot") {
                        self.smoke_image = crate::acceptance::save_png(&path, &image).is_ok();
                    }
                }
            }
            if self.smoke_frame > total + 1 && self.smoke_image {
                if let Some(path) = crate::acceptance::argument("--report") {
                    let report = serde_json::json!({"status":"passed","os":std::env::consts::OS,"views_rendered":VIEWS,"models_rendered":experiment::MODELS.iter().map(|m|m.id.key()).collect::<Vec<_>>(),"screenshot":true,"native_frames":self.smoke_frame,"model":self.cfg.model.key()});
                    if crate::acceptance::write(&path, &report).is_err() {
                        self.error = "Could not write the UI verification report.".into();
                        return;
                    }
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }
    #[cfg(not(feature = "acceptance"))]
    fn smoke_hooks(&mut self, _ctx: &egui::Context) {}
}
impl eframe::App for Workbench {
    fn ui(&mut self, root: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = root.ctx().clone();
        if self.legacy_open {
            self.research.poll(&ctx);
            self.legacy.ui(root, frame);
            egui::Window::new("Model workbench")
                .id(egui::Id::new("return_to_workbench"))
                .anchor(egui::Align2::RIGHT_TOP, [-15., 8.])
                .resizable(false)
                .collapsible(false)
                .show(&ctx, |ui| {
                    if ui.button("Return to model workspace").clicked() {
                        self.legacy_open = false;
                        ctx.set_style_of(egui::Theme::Dark, v::style());
                    }
                });
            return;
        }
        self.poll(&ctx);
        if self.view != 7 { self.sidebar(root); }
        if self.view < 6 {
            self.controls(root);
        }
        self.header(root);
        self.footer(root);
        egui::CentralPanel::default().frame(egui::Frame::new().fill(Color32::from_rgb(15,21,30)).inner_margin(18.)).show(root,|ui|{
            if !self.error.is_empty(){v::panel().show(ui,|ui|{ui.colored_label(v::RED,&self.error);});ui.add_space(8.);}
            if let Some(run)=self.run.clone(){
                if self.view!=7 && (run.config!=self.cfg||self.receiver.is_some()||self.due.is_some()){ui.label(RichText::new(format!("Showing the previous fitted experiment: {}. New settings are being prepared.",run.config.model.descriptor().name)).size(12.).color(v::AMBER));}
                if let Some(warning)=run.warning(){ui.colored_label(v::AMBER,warning);}
                if !ctx.egui_wants_keyboard_input(){ctx.input(|i|{if i.key_pressed(egui::Key::ArrowLeft){self.stage=self.stage.saturating_sub(1);self.playing=false;}if i.key_pressed(egui::Key::ArrowRight){self.stage=(self.stage+1).min(run.last_stage());self.playing=false;}});}
                egui::ScrollArea::vertical().id_salt(("workbench_content",self.view)).auto_shrink([false,false]).show(ui,|ui|{match self.view{0=>self.explore(ui,&run),1=>self.coefficients(ui,&run),2=>self.optimization(ui,&run),3=>self.learn(ui,&run),4=>self.code_view(ui,&run),5=>self.data_view(ui,&run),7=>self.decision.ui(ui),_=>{self.research.synchronize(&run,self.table.as_ref(),self.revision);self.research.ui(ui);}}});
            }else{ui.centered_and_justified(|ui|{ui.spinner();});}
        });
        self.smoke_hooks(&ctx);
    }
}
impl Drop for Workbench {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        self.path_cancel.store(true, Ordering::Relaxed);
    }
}
