use crate::{
    data::Table,
    examples, instrument,
    model::{self, Config, Fit, Method, Node},
    playback, python, quiz, science, studio,
};
use eframe::egui::{self, Color32, Pos2, Rect, Stroke, Vec2};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
};

const BLUE: Color32 = Color32::from_rgb(145, 181, 255);
const ORANGE: Color32 = Color32::from_rgb(255, 183, 112);
const GREEN: Color32 = Color32::from_rgb(107, 235, 188);
const GREY: Color32 = Color32::from_rgb(156, 165, 182);
const RED: Color32 = Color32::from_rgb(255, 119, 129);

pub struct Lab {
    table: Table,
    target: usize,
    features: Vec<bool>,
    cfg: Config,
    fitted: Option<Fit>,
    stage: usize,
    feature: usize,
    selected_row: usize,
    tab: usize,
    error: String,
    status: String,
    file_path: String,
    receiver: Option<mpsc::Receiver<Result<Fit, String>>>,
    cancel: Arc<AtomicBool>,
    before: Vec<f64>,
    after: Vec<f64>,
    playing: bool,
    last_tick: f64,
    motion: bool,
    seconds_per_tree: f64,
    animation_start: f64,
    quiz_index: usize,
    quiz_answers: Vec<[bool; 4]>,
    quiz_checked: Vec<bool>,
    example_index: usize,
    example_lesson: String,
    python: python::Workspace,
    follow_code: bool,
    scenario: usize,
    refit_due: Option<f64>,
    guess: Option<(usize, usize, i8)>,
    guess_feedback: String,
    fit_revision: u64,
    slice_cache: studio::SliceCache,
    transition_from: usize,
    analysis: Option<science::Analysis>,
    analysis_rx: Option<mpsc::Receiver<(u64, science::Analysis)>>,
    pinned: Option<(Fit, usize)>,
    challenge: Option<usize>,
    clock_time: f64,
    settings_layer: usize,
    quiz_topic: String,
    quiz_order: Vec<usize>,
    quiz_shuffle: u64,
    quiz_review_only: bool,
    #[cfg(feature = "acceptance")]
    smoke_frames: usize,
    #[cfg(feature = "acceptance")]
    smoke_screenshot: bool,
}
impl Lab {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        cc.egui_ctx.set_theme(egui::ThemePreference::Dark);
        cc.egui_ctx
            .set_style_of(egui::Theme::Dark, instrument::style());
        let mut app = Self {
            table: Table::demo(0),
            target: 2,
            features: vec![true, false, false],
            cfg: Config {
                trees: 30,
                depth: 1,
                rate: 0.2,
                ..Default::default()
            },
            fitted: None,
            stage: 0,
            feature: 0,
            selected_row: 0,
            tab: 6,
            error: String::new(),
            status: "Ready".into(),
            file_path: String::new(),
            receiver: None,
            cancel: Arc::new(AtomicBool::new(false)),
            before: vec![],
            after: vec![],
            playing: false,
            last_tick: 0.0,
            motion: true,
            seconds_per_tree: 0.1,
            animation_start: -1.0,
            quiz_index: 0,
            quiz_answers: vec![[false; 4]; quiz::questions().len()],
            quiz_checked: vec![false; quiz::questions().len()],
            example_index: 0,
            example_lesson: String::new(),
            python: python::Workspace::default(),
            follow_code: true,
            scenario: 0,
            refit_due: None,
            guess: None,
            guess_feedback: String::new(),
            fit_revision: 0,
            slice_cache: studio::SliceCache::default(),
            transition_from: 0,
            analysis: None,
            analysis_rx: None,
            pinned: None,
            challenge: None,
            clock_time: 0.0,
            settings_layer: 1,
            quiz_topic: "All topics".into(),
            quiz_order: (0..quiz::questions().len()).collect(),
            quiz_shuffle: 0,
            quiz_review_only: false,
            #[cfg(feature = "acceptance")]
            smoke_frames: 0,
            #[cfg(feature = "acceptance")]
            smoke_screenshot: false,
        };
        app.start_fit();
        app
    }
    fn invalidate(&mut self) {
        self.fitted = None;
        self.before.clear();
        self.after.clear();
        self.stage = 0;
        self.feature = 0;
        self.playing = false;
        self.guess = None;
        self.guess_feedback.clear();
    }
    fn set_table(&mut self, table: Table) {
        self.example_lesson.clear();
        self.target = table.numeric.iter().rposition(|&b| b).unwrap_or(0);
        self.features = table
            .numeric
            .iter()
            .enumerate()
            .map(|(j, &v)| v && j != self.target)
            .collect();
        self.table = table;
        self.invalidate();
        self.error.clear();
        self.status = "Data loaded. Choose predictors and fit.".into();
    }
    fn load_file(&mut self, path: PathBuf) {
        let result = std::fs::metadata(&path)
            .map_err(|e| e.to_string())
            .and_then(|m| {
                if m.len() > 16 * 1024 * 1024 {
                    Err("CSV exceeds the 16 MB teaching-edition file limit.".into())
                } else {
                    std::fs::read_to_string(&path).map_err(|e| e.to_string())
                }
            })
            .and_then(|s| {
                Table::parse(&path.file_name().unwrap_or_default().to_string_lossy(), &s)
            });
        match result {
            Ok(t) => {
                self.file_path = path.to_string_lossy().into();
                self.set_table(t);
            }
            Err(e) => self.error = e,
        }
    }
    fn start_fit(&mut self) {
        if self.receiver.is_some() {
            return;
        }
        self.refit_due = None;
        let features: Vec<_> = self
            .features
            .iter()
            .enumerate()
            .filter_map(|(i, &b)| (b && i != self.target).then_some(i))
            .collect();
        let data = match self.table.dataset(self.target, &features) {
            Ok(d) => d,
            Err(e) => {
                self.error = e;
                return;
            }
        };
        self.cfg.mtry = self.cfg.mtry.clamp(1, data.names.len());
        let cfg = self.cfg.clone();
        let (tx, rx) = mpsc::channel();
        self.cancel = Arc::new(AtomicBool::new(false));
        let cancel = self.cancel.clone();
        self.receiver = Some(rx);
        self.playing = false;
        self.error.clear();
        self.status = "Fitting on training rows…".into();
        std::thread::spawn(move || {
            let result = model::fit(data, cfg, cancel);
            let _ = tx.send(result);
        });
    }
    fn refresh_stage(&mut self) {
        self.refresh_from(self.stage.saturating_sub(1));
    }
    fn refresh_from(&mut self, from: usize) {
        self.animation_start = self.clock_time;
        if let Some(f) = &self.fitted {
            self.stage = self.stage.min(f.trees.len());
            self.transition_from = from.min(f.trees.len());
            self.before = f.predictions(self.transition_from);
            self.after = f.predictions(self.stage);
            self.feature = self.feature.min(f.data.names.len() - 1);
            self.selected_row = self.selected_row.min(f.data.y.len() - 1);
        }
    }
    fn poll(&mut self, ctx: &egui::Context) {
        self.python.poll();
        if self.python.running {
            ctx.request_repaint_after(std::time::Duration::from_millis(30));
        }
        let now = ctx.input(|i| i.time);
        self.clock_time = now;
        if self.animation_start < 0.0 {
            self.animation_start = now;
        }
        if self.motion && now - self.animation_start < self.seconds_per_tree {
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }
        if let Some(rx) = &self.receiver {
            match rx.try_recv() {
                Ok(result) => {
                    self.receiver = None;
                    match result {
                        Ok(f) => {
                            self.status = format!(
                                "Ready · {} training / {} validation rows · {} trees fitted",
                                f.train.len(),
                                f.valid.len(),
                                f.trees.len()
                            );
                            self.selected_row = (0..f.data.y.len())
                                .max_by(|&a, &b| {
                                    (f.data.y[a] - f.baseline)
                                        .abs()
                                        .total_cmp(&(f.data.y[b] - f.baseline).abs())
                                })
                                .unwrap_or(0);
                            self.fitted = Some(f);
                            self.fit_revision += 1;
                            self.analysis = None;
                            self.stage = 0;
                            self.guess = None;
                            self.guess_feedback.clear();
                            self.refresh_stage();
                        }
                        Err(e) => {
                            self.error = e;
                            self.status = "Ready".into();
                        }
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.receiver = None;
                    self.error = "Training worker stopped unexpectedly.".into();
                }
                Err(mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(100))
                }
            }
        }
        if self.playing {
            if let Some(f) = &self.fitted {
                let steps = playback::due_steps(
                    now - self.last_tick,
                    self.seconds_per_tree,
                    f.trees.len().saturating_sub(self.stage),
                );
                if steps > 0 {
                    self.last_tick += steps as f64 * self.seconds_per_tree;
                    let from = self.stage;
                    self.stage += steps;
                    self.refresh_from(from);
                }
                if self
                    .fitted
                    .as_ref()
                    .is_some_and(|f| self.stage >= f.trees.len())
                {
                    self.playing = false;
                }
            } else {
                self.playing = false;
            }
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }
        if let Some(rx) = &self.analysis_rx {
            match rx.try_recv() {
                Ok((revision, result)) => {
                    self.analysis_rx = None;
                    if revision == self.fit_revision {
                        self.analysis = Some(result);
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => self.analysis_rx = None,
                Err(mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(80))
                }
            }
        }
        if let Some(due) = self.refit_due {
            if self.receiver.is_none() && now >= due {
                self.start_fit();
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(60));
            }
        }
    }
    fn sidebar(&mut self, ui: &mut egui::Ui) {
        let busy = self.receiver.is_some();
        ui.add_space(12.0);
        ui.label(
            egui::RichText::new("CONTROL LAYERS")
                .monospace()
                .size(10.0)
                .color(GREY),
        );
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 5.0;
            let width = (ui.available_width() - 10.0) / 3.0;
            for (i, name) in ["DATA", "ENGINE", "TUNE"].iter().enumerate() {
                let selected = self.settings_layer == i;
                if ui
                    .add_sized(
                        [width, 31.0],
                        egui::Button::new(
                            egui::RichText::new(*name)
                                .monospace()
                                .size(11.0)
                                .color(if selected { instrument::BASE } else { GREY }),
                        )
                        .fill(if selected {
                            BLUE
                        } else {
                            instrument::SURFACE
                        }),
                    )
                    .clicked()
                {
                    self.settings_layer = i;
                }
            }
        });
        ui.add_space(12.0);
        ui.label(egui::RichText::new(&self.table.name).strong());
        ui.small(format!(
            "{} rows  /  predict {}",
            self.table.rows.len(),
            self.table.headers[self.target]
        ));
        ui.separator();
        if self.settings_layer == 0 {
            ui.heading("Data source");
            let names = [
                "A smooth nonlinear signal",
                "Two interacting predictors",
                "Income and age",
                "Diamond prices",
                "Florida communities",
                "Your own CSV",
                "Noise and overfitting",
            ];
            let old = self.scenario;
            ui.add_enabled_ui(!busy, |ui| {
                egui::ComboBox::from_id_salt("guided-scenario")
                    .width(ui.available_width() - 8.0)
                    .selected_text(names[self.scenario])
                    .show_ui(ui, |ui| {
                        for (i, name) in names.iter().enumerate() {
                            ui.selectable_value(&mut self.scenario, i, *name);
                        }
                    });
                if self.scenario != old {
                    match self.scenario {
                        0 | 1 => {
                            let k = self.scenario;
                            self.set_table(Table::demo(k));
                            if k == 0 {
                                self.features = vec![true, false, false];
                            }
                            self.start_fit();
                        }
                        2..=4 => {
                            let i = self.scenario - 2;
                            match examples::table(i) {
                                Ok(t) => {
                                    self.set_table(t);
                                    self.example_lesson = examples::examples()[i].lesson.into();
                                    self.cfg.mtry =
                                        ((self.table.headers.len() - 1) as f64).sqrt().floor()
                                            as usize;
                                    self.start_fit();
                                }
                                Err(e) => self.error = e,
                            }
                        }
                        6 => {
                            self.set_table(Table::demo(2));
                            self.features = vec![true, false, false];
                            self.start_fit();
                        }
                        _ => match choose_path(false) {
                            Ok(Some(p)) => {
                                self.load_file(p);
                                if self.error.is_empty() {
                                    self.start_fit();
                                }
                            }
                            Ok(None) => self.scenario = old,
                            Err(e) => {
                                self.error = e;
                                self.scenario = old;
                            }
                        },
                    }
                }
            });
            ui.small(format!("{} observations", self.table.rows.len()));
            ui.label(format!("Predicting: {}", self.table.headers[self.target]));
            if ui
                .add_enabled(!busy, egui::Button::new("Open your own CSV…"))
                .clicked()
            {
                match choose_path(false) {
                    Ok(Some(p)) => {
                        self.scenario = 5;
                        self.load_file(p);
                        if self.error.is_empty() {
                            self.start_fit();
                        }
                    }
                    Ok(None) => {}
                    Err(e) => self.error = e,
                }
            }
        }
        if self.settings_layer == 1 {
            ui.add_space(8.0);
            ui.heading("Model engine");
            let mut model_changed = false;
            ui.add_enabled_ui(!busy, |ui| {
                for (method, title, description) in [
                    (
                        Method::Tree,
                        "One decision tree",
                        "Split the data into smaller groups",
                    ),
                    (
                        Method::Boosting,
                        "Boosting",
                        "Learn from the errors still left over",
                    ),
                    (
                        Method::Stochastic,
                        "Stochastic boosting",
                        "Learn each correction from a sample",
                    ),
                    (
                        Method::Forest,
                        "Random forest",
                        "Average many different trees",
                    ),
                ] {
                    let selected = self.cfg.method == method;
                    let text = egui::RichText::new(format!("{title}\n{description}"))
                        .size(13.0)
                        .color(if selected { BLUE } else { instrument::TEXT });
                    if ui
                        .add_sized(
                            [ui.available_width(), 58.0],
                            egui::Button::new(text).fill(if selected {
                                Color32::from_rgb(39, 57, 85)
                            } else {
                                instrument::SURFACE
                            }),
                        )
                        .clicked()
                    {
                        self.cfg.method = method;
                        model_changed = true;
                    }
                }
            });
            if model_changed {
                self.start_fit();
            }
            ui.add_space(10.0);
            if ui.button("Open tuning controls  >").clicked() {
                self.settings_layer = 2;
            }
        }
        if self.settings_layer == 2 {
            ui.add_space(8.0);
            ui.heading("Engine tuning");
            let before = self.cfg.clone();
            ui.add_enabled_ui(!busy,|ui|{
            ui.horizontal(|ui|{
                if self.cfg.method.boosting(){instrument::dial(ui,&mut self.cfg.rate,0.01..=1.0,"STEP SIZE",true,false,BLUE);}
                let mut depth=self.cfg.depth as f64;instrument::dial(ui,&mut depth,1.0..=8.0,"DEPTH",false,true,GREEN);self.cfg.depth=depth as usize;
            });
            if self.cfg.method.boosting(){ui.horizontal(|ui|{ui.label("Learning rate");ui.add(egui::DragValue::new(&mut self.cfg.rate).range(0.01..=1.0).speed(0.001));});ui.small("Smaller steps usually need more trees.");}
            ui.horizontal(|ui|{ui.label("Depth");ui.add(egui::DragValue::new(&mut self.cfg.depth).range(1..=8));});
            ui.small("Deeper trees can fit more complex patterns.");
            if self.cfg.method==Method::Stochastic{ui.label("Rows used by each new tree");ui.add(egui::Slider::new(&mut self.cfg.fraction,0.1..=1.0).text("fraction"));}
            if self.cfg.method==Method::Forest{
                let count=self.features.iter().filter(|&&v|v).count().max(1);self.cfg.mtry=self.cfg.mtry.clamp(1,count);
                ui.label("Candidate predictors at each split");ui.add(egui::Slider::new(&mut self.cfg.mtry,1..=count).text("predictors"));
            }
            egui::CollapsingHeader::new("More model settings").show(ui,|ui|{
                if self.cfg.method!=Method::Tree{ui.add(egui::Slider::new(&mut self.cfg.trees,1..=300).text("total trees"));}
                ui.add(egui::Slider::new(&mut self.cfg.min_leaf,1..=25).text("minimum leaf rows"));
                ui.add(egui::Slider::new(&mut self.cfg.validation,0.1..=0.5).text("validation fraction"));
                ui.horizontal(|ui|{ui.label("Seed");ui.add(egui::DragValue::new(&mut self.cfg.seed));});
                if self.cfg.method.boosting(){ui.add(egui::Slider::new(&mut self.cfg.patience,0..=30).text("early-stop patience (0 off)"));ui.small("Stop after this many rounds without sufficient validation improvement. Inspect retains the full fitted path.");ui.add(egui::Slider::new(&mut self.cfg.min_relative_improvement,0.0..=0.05).text("minimum relative improvement"));}
            });
        });
            if before != self.cfg {
                self.refit_due = Some(ui.input(|i| i.time) + 0.35);
                self.playing = false;
            }
        }
        if self.settings_layer == 0 {
            let mut columns_changed = false;
            ui.add_enabled_ui(!busy, |ui| {
                egui::CollapsingHeader::new("Choose response / predictors").show(ui, |ui| {
                    let target = self.target;
                    egui::ComboBox::from_label("Predict")
                        .selected_text(&self.table.headers[self.target])
                        .show_ui(ui, |ui| {
                            for (i, h) in self.table.headers.iter().enumerate() {
                                if self.table.numeric[i] {
                                    ui.selectable_value(&mut self.target, i, h);
                                }
                            }
                        });
                    if self.target != target {
                        self.features[self.target] = false;
                        columns_changed = true;
                    }
                    for i in 0..self.features.len() {
                        if i != self.target && self.table.numeric[i] {
                            columns_changed |= ui
                                .checkbox(&mut self.features[i], &self.table.headers[i])
                                .changed();
                        }
                    }
                });
            });
            if columns_changed {
                self.invalidate();
                self.refit_due = Some(ui.input(|i| i.time) + 0.35);
            }
        }
        if busy {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Updating the model…");
            });
        } else if self.refit_due.is_some() {
            ui.colored_label(BLUE, "Applying your changes…");
        } else if self.fitted.is_none() && ui.button("Prepare this example").clicked() {
            self.start_fit();
        }
        ui.add_space(10.0);
        ui.separator();
        ui.small("Data stays on this computer. Numeric regression models; prediction patterns do not establish causation.");
    }
    #[allow(dead_code)]
    fn advanced_sidebar(&mut self, ui: &mut egui::Ui) {
        let busy = self.receiver.is_some();
        ui.heading("Data & model");
        ui.label("Numeric-response regression · local only");
        ui.separator();
        ui.add_enabled_ui(!busy, |ui| {
            if ui.button("Open CSV…").clicked() {
                match choose_path(false) {
                    Ok(Some(p)) => self.load_file(p),
                    Ok(None) => {}
                    Err(e) => self.error = e,
                }
            }
            ui.label("Or drop a CSV onto this window.");
            ui.horizontal(|ui| {
                ui.text_edit_singleline(&mut self.file_path);
                if ui.button("Load path").clicked() {
                    self.load_file(PathBuf::from(self.file_path.trim()));
                }
            });
            ui.horizontal(|ui| {
                if ui.button("Nonlinear demo").clicked() {
                    self.set_table(Table::demo(0));
                    self.features = vec![true, false, false];
                }
                if ui.button("Interaction demo").clicked() {
                    self.set_table(Table::demo(1));
                }
            });
            let bank = examples::examples();
            egui::ComboBox::from_label("Existing-data examples")
                .selected_text(bank[self.example_index].name)
                .show_ui(ui, |ui| {
                    for (i, e) in bank.iter().enumerate() {
                        ui.selectable_value(&mut self.example_index, i, e.name);
                    }
                });
            if ui.button("Load selected example").clicked() {
                match examples::table(self.example_index) {
                    Ok(t) => {
                        self.set_table(t);
                        self.example_lesson = bank[self.example_index].lesson.into();
                        self.cfg.mtry =
                            ((self.table.headers.len() - 1) as f64).sqrt().floor() as usize;
                    }
                    Err(e) => self.error = e,
                }
            }
            ui.strong(&self.table.name);
            ui.label(format!(
                "{} rows · {} columns",
                self.table.rows.len(),
                self.table.headers.len()
            ));
            let old_target = self.target;
            egui::ComboBox::from_label("Response")
                .selected_text(&self.table.headers[self.target])
                .show_ui(ui, |ui| {
                    for (i, name) in self.table.headers.iter().enumerate() {
                        if self.table.numeric[i] {
                            ui.selectable_value(&mut self.target, i, name);
                        }
                    }
                });
            if self.target != old_target {
                self.features[self.target] = false;
                self.invalidate();
            }
            ui.label("Predictors");
            let mut changed = false;
            for i in 0..self.table.headers.len() {
                if i == self.target {
                    continue;
                }
                if self.table.numeric[i] {
                    changed |= ui
                        .checkbox(&mut self.features[i], &self.table.headers[i])
                        .changed();
                } else {
                    ui.add_enabled(
                        false,
                        egui::Label::new(format!("{} · nonnumeric/missing", self.table.headers[i])),
                    );
                }
            }
            if changed {
                self.invalidate();
            }
            ui.separator();
            egui::ComboBox::from_label("Algorithm")
                .selected_text(self.cfg.method.label())
                .show_ui(ui, |ui| {
                    for method in Method::ALL {
                        ui.selectable_value(&mut self.cfg.method, method, method.label());
                    }
                });
            ui.add(egui::Slider::new(&mut self.cfg.depth, 1..=8).text("Maximum depth"));
            ui.add(egui::Slider::new(&mut self.cfg.min_leaf, 1..=25).text("Minimum leaf rows"));
            if self.cfg.method != Method::Tree {
                ui.add(egui::Slider::new(&mut self.cfg.trees, 1..=300).text("Trees"));
            }
            if self.cfg.method.boosting() {
                ui.add(
                    egui::Slider::new(&mut self.cfg.rate, 0.01..=1.0)
                        .logarithmic(true)
                        .text("Learning rate λ"),
                );
            }
            if self.cfg.method == Method::Stochastic {
                ui.add(
                    egui::Slider::new(&mut self.cfg.fraction, 0.1..=1.0).text("Sample fraction π"),
                );
            }
            if self.cfg.method == Method::Forest {
                let p = self.features.iter().filter(|&&v| v).count().max(1);
                self.cfg.mtry = self.cfg.mtry.clamp(1, p);
                ui.add(egui::Slider::new(&mut self.cfg.mtry, 1..=p).text("Features per split"));
            }
            ui.add(
                egui::Slider::new(&mut self.cfg.validation, 0.1..=0.5).text("Validation fraction"),
            );
            ui.horizontal(|ui| {
                ui.label("Random seed");
                ui.add(egui::DragValue::new(&mut self.cfg.seed));
            });
            if ui
                .add_sized([ui.available_width(), 36.0], egui::Button::new("Fit model"))
                .clicked()
            {
                self.start_fit();
            }
        });
        if busy {
            ui.spinner();
            if ui.button("Cancel training").clicked() {
                self.cancel.store(true, Ordering::Relaxed);
            }
        }
        if self.fitted.as_ref().is_some_and(|f| f.cfg != self.cfg) {
            ui.colored_label(
                ORANGE,
                "Settings changed. Fit again to apply them. Charts still show the completed fit.",
            );
        }
        ui.separator();
        ui.small("CSV requirements: header; 4–5,000 rows; finite numeric selected columns. No automatic encoding, imputation or dropped rows. Random validation split assumes exchangeable rows; not appropriate for time series or grouped leakage.");
    }
    fn advance_guided(&mut self) {
        if let Some(f) = &self.fitted {
            if self.stage >= f.trees.len() {
                return;
            }
            let next = self.stage + 1;
            let change = f.predict(&f.data.x[self.selected_row], next)
                - f.predict(&f.data.x[self.selected_row], self.stage);
            if let Some((row, round, direction)) = self.guess {
                if row == self.selected_row && round == next {
                    let actual = if change > 1e-10 {
                        1
                    } else if change < -1e-10 {
                        -1
                    } else {
                        0
                    };
                    self.guess_feedback = format!(
                        "{} The prediction {} by {:.4}.",
                        if actual == direction {
                            "Your prediction was right."
                        } else {
                            "Compare your prediction with the update:"
                        },
                        if actual == 1 {
                            "rose"
                        } else if actual == -1 {
                            "fell"
                        } else {
                            "changed"
                        },
                        change.abs()
                    );
                }
            }
            self.guess = None;
            self.stage = next;
            self.refresh_stage();
        }
    }
    fn explore(&mut self, ui: &mut egui::Ui) {
        if self.fitted.is_none() {
            ui.add_space(40.0);
            ui.heading("Preparing your example");
            if self.receiver.is_some() {
                ui.spinner();
                ui.label("The first view will appear when the model is ready.");
            } else {
                ui.label("Choose data and a model on the left.");
            }
            return;
        }
        let method = self.fitted.as_ref().unwrap().cfg.method;
        let title = match method {
            Method::Tree => "How a decision tree predicts",
            Method::Boosting => "How boosting learns from its errors",
            Method::Stochastic => "How a sample teaches the next correction",
            Method::Forest => "How different trees become one prediction",
        };
        ui.add_space(7.0);
        ui.label(egui::RichText::new(title).size(27.0).strong());
        ui.label(match method{
            Method::Tree=>"A tree separates observations into groups. Each leaf predicts its group's training average.",
            Method::Boosting=>"Each new tree predicts the errors left by the current model. The learning rate controls how much of that correction we add.",
            Method::Stochastic=>"Only sampled training rows teach this tree. Its correction still updates every observation's prediction.",
            Method::Forest=>"Each tree learns from a different bootstrap sample. The forest averages their predictions rather than correcting residuals.",
        });
        ui.add_space(7.0);
        let max = self.fitted.as_ref().unwrap().trees.len();
        let busy = self.receiver.is_some() || self.refit_due.is_some();
        ui.horizontal(|ui| {
            let label = if method == Method::Tree {
                if self.stage == 0 {
                    "Grow the tree"
                } else {
                    "Replay the tree"
                }
            } else if self.stage >= max {
                "All trees added"
            } else {
                "Add next tree"
            };
            if ui
                .add_enabled(
                    !busy && (self.stage < max || method == Method::Tree),
                    egui::Button::new(egui::RichText::new(label).color(instrument::BASE).strong())
                        .fill(BLUE)
                        .min_size(Vec2::new(160.0, 38.0)),
                )
                .clicked()
            {
                self.playing = false;
                if self.stage >= max {
                    self.animation_start = ui.input(|i| i.time);
                } else {
                    self.advance_guided();
                }
            }
            if method != Method::Tree
                && ui
                    .add_enabled(
                        !busy,
                        egui::Button::new(if self.playing {
                            "Pause"
                        } else {
                            "Play automatically"
                        }),
                    )
                    .clicked()
            {
                self.playing = !self.playing;
                self.last_tick = ui.input(|i| i.time);
                if self.playing {
                    if self.stage >= max {
                        self.stage = 0;
                        self.refresh_stage();
                    }
                    self.advance_guided();
                }
            }
            if ui.button("Start over").clicked() {
                self.stage = 0;
                self.playing = false;
                self.guess = None;
                self.guess_feedback.clear();
                self.refresh_stage();
            }
            ui.label(format!("Tree {} of {}", self.stage, max));
            egui::CollapsingHeader::new("Playback").show(ui, |ui| {
                ui.checkbox(&mut self.motion, "Animation");
                ui.add(
                    egui::Slider::new(&mut self.seconds_per_tree, 0.01..=4.0)
                        .logarithmic(true)
                        .text("seconds / tree"),
                );
            });
        });
        let mut jump = None;
        ui.horizontal_wrapped(|ui| {
            ui.label("Speed");
            for (label, seconds) in playback::PRESETS {
                if ui
                    .selectable_label((self.seconds_per_tree - seconds).abs() < 1e-8, label)
                    .clicked()
                {
                    self.seconds_per_tree = seconds;
                    self.last_tick = ui.input(|i| i.time);
                }
            }
            if ui.button("+10 trees").clicked() {
                jump = Some((self.stage + 10).min(max));
            }
            if ui.button("Finish now").clicked() {
                jump = Some(max);
            }
            if ui.button("Best validation").clicked() {
                jump = Some(self.fitted.as_ref().unwrap().best_stage());
            }
            let mut selected = self.stage;
            if ui
                .add(egui::Slider::new(&mut selected, 0..=max).text("tree"))
                .changed()
            {
                jump = Some(selected);
            }
        });
        if let Some(target) = instrument::step_keys(ui, self.fitted.as_ref().unwrap(), self.stage) {
            jump = Some(target);
        }
        if let Some(target) = jump {
            let from = self.stage;
            self.stage = target;
            self.playing = false;
            self.guess = None;
            self.refresh_from(from);
            self.animation_start = ui.input(|i| i.time) - self.seconds_per_tree;
        }
        let phase = if self.motion {
            ((ui.input(|i| i.time) - self.animation_start) / self.seconds_per_tree).clamp(0.0, 1.0)
        } else {
            1.0
        };
        let labels = match method {
            Method::Tree => [
                "Choose a split",
                "Form the leaves",
                "Predict each leaf's mean",
            ],
            Method::Forest => [
                "Bootstrap the rows",
                "Grow a different tree",
                "Update the average",
            ],
            Method::Stochastic => [
                "Sample training rows",
                "Fit the residuals",
                "Add a scaled correction",
            ],
            Method::Boosting => [
                "Measure the errors",
                "Fit the residuals",
                "Add a scaled correction",
            ],
        };
        ui.horizontal_wrapped(|ui| {
            for (k, label) in labels.iter().enumerate() {
                let active = self.stage > 0
                    && (phase >= 1.0 && k == 2
                        || phase < 1.0 && (phase * 3.0).floor() as usize == k);
                ui.label(
                    egui::RichText::new(format!("{}  {}", k + 1, label))
                        .size(12.0)
                        .color(if active { BLUE } else { GREY })
                        .strong(),
                );
                if k < 2 {
                    ui.label(egui::RichText::new("/").color(GREY));
                }
            }
        });
        let fit = self.fitted.as_ref().unwrap();
        let chosen = studio::chart(
            ui,
            fit,
            self.stage,
            self.feature,
            self.selected_row,
            phase,
            self.motion,
            &mut self.slice_cache,
            self.fit_revision,
            self.transition_from,
        );
        if let Some(row) = chosen {
            self.selected_row = row;
            self.guess = None;
            self.guess_feedback.clear();
        }
        ui.horizontal_wrapped(|ui| {
            ui.colored_label(
                Color32::from_rgb(26, 139, 110),
                "Green line: model prediction",
            );
            ui.colored_label(ORANGE, "Gold tether: residual error");
            ui.label("Dots are observations. Click one to follow it.");
            if fit.data.names.len() > 1 {
                egui::ComboBox::from_id_salt("explore-axis")
                    .selected_text(&fit.data.names[self.feature])
                    .show_ui(ui, |ui| {
                        for (i, name) in fit.data.names.iter().enumerate() {
                            ui.selectable_value(&mut self.feature, i, name);
                        }
                    });
            }
        });
        if fit.data.names.len() > 1 {
            ui.small("The line is a slice through the model with other predictors at training medians. The highlighted prediction uses that observation's actual values.");
        }
        let row = self.selected_row;
        let old = self.before[row];
        let new = self.after[row];
        instrument::glass().show(ui,|ui|{
            ui.label(egui::RichText::new(format!("Following observation {}",row+1)).size(12.0).color(GREY));
            ui.columns(4,|cols|{for(col,label,value,color)in[
                (0,"Actual response",fit.data.y[row],ORANGE),(1,"Prediction before",old,GREY),(2,"Change this step",new-old,BLUE),(3,"Prediction after",new,GREEN),
            ]{cols[col].small(label);cols[col].label(egui::RichText::new(format!("{value:.3}")).size(23.0).color(color).strong());}});
            if self.stage==0{ui.small("The starting prediction is the mean of the training responses. Add a tree to see it change.");}
            else if self.transition_from+1!=self.stage{ui.monospace(format!("Tree {} -> {}: cumulative prediction change {:+.4}. Use Add next tree for a single correction.",self.transition_from,self.stage,new-old));}
            else if method.boosting(){let h=fit.trees[self.stage-1].predict(&fit.data.x[row]);ui.monospace(format!("{old:.3} + {:.3} × ({h:.3}) = {new:.3}    |    residual: {:.3} -> {:.3}",fit.cfg.rate,fit.data.y[row]-old,fit.data.y[row]-new));}
            else if method==Method::Forest{ui.small("The change comes from including one more tree in the average.");}
        });
        if self.stage < max {
            ui.horizontal_wrapped(|ui| {
                ui.label("Predict the next move:");
                for (direction, label) in [(1, "Up"), (-1, "Down"), (0, "No change")] {
                    let selected = self
                        .guess
                        .is_some_and(|(r, s, d)| r == row && s == self.stage + 1 && d == direction);
                    if ui.selectable_label(selected, label).clicked() {
                        self.guess = Some((row, self.stage + 1, direction));
                        self.guess_feedback.clear();
                    }
                }
                ui.small("Optional — then add the next tree.");
            });
        }
        if !self.guess_feedback.is_empty() {
            ui.colored_label(BLUE, &self.guess_feedback);
        }
        ui.columns(2,|cols|{
            studio::error_path(&mut cols[0],fit,self.stage);
            cols[1].add_space(8.0);cols[1].strong("What to notice");
            let hint=if self.stage==0{"The model starts with one horizontal prediction. Which observations need a positive correction? Which need a negative one?"}
                else if method==Method::Forest{"The average may move up or down when a new tree joins. More trees stabilize the average; they do not work by fitting the forest's residuals."}
                else if method==Method::Tree{"Within each leaf, predictions are constant. Change depth to allow more splits, then compare error on held-out observations."}
                else{"The warm vertical tether is this observation's residual. Small steps change predictions gradually. A shrinking training error does not guarantee a shrinking validation error."};
            cols[1].label(hint);
            if self.stage>0{let(splits,leaves)=studio::tree_counts(&fit.trees[self.stage-1]);cols[1].small(format!("Selected tree: {splits} splits, {leaves} leaves."));}
            cols[1].horizontal_wrapped(|ui|{
                if ui.button("See this model in Python").clicked(){self.tab=5;self.python.source_mode=0;}
                if ui.button("Inspect the tree").clicked(){self.tab=0;}
                if ui.button("Pin displayed model").clicked(){self.pinned=Some((fit.clone(),self.stage));self.status=format!("Pinned {} at tree {} for comparison",fit.cfg.method.label(),self.stage);}
                if ui.button("Try an experiment").clicked(){self.tab=8;}
            });
        });
    }
    fn stage_controls(&mut self, ui: &mut egui::Ui) {
        let Some(f) = &self.fitted else {
            return;
        };
        let n = f.trees.len();
        let mut changed = false;
        ui.horizontal(|ui| {
            changed |= ui
                .add(egui::Slider::new(&mut self.stage, 0..=n).text("Trees included"))
                .changed();
            if ui.button("◀ Previous").clicked() && self.stage > 0 {
                self.stage -= 1;
                changed = true;
            }
            if ui.button("Next ▶").clicked() && self.stage < n {
                self.stage += 1;
                changed = true;
            }
            if ui
                .button(if self.playing { "Pause" } else { "Play" })
                .clicked()
            {
                self.playing = !self.playing;
                self.last_tick = ui.input(|i| i.time);
            }
        });
        if changed {
            self.playing = false;
            self.refresh_stage();
        }
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.motion, "Animate updates");
            ui.add(
                egui::Slider::new(&mut self.seconds_per_tree, 0.01..=4.0)
                    .logarithmic(true)
                    .text("Seconds / tree"),
            );
            if ui.button("Replay this step").clicked() {
                self.animation_start = ui.input(|i| i.time);
            }
        });
    }
    fn process(&mut self, ui: &mut egui::Ui) {
        if self.fitted.is_none() {
            ui.label("Choose your data and click Fit model.");
            return;
        }
        self.stage_controls(ui);
        let f = self.fitted.as_ref().unwrap();
        let n = f.trees.len();
        ui.horizontal(|ui| {
            egui::ComboBox::from_label("Horizontal axis")
                .selected_text(&f.data.names[self.feature])
                .show_ui(ui, |ui| {
                    for (j, name) in f.data.names.iter().enumerate() {
                        ui.selectable_value(&mut self.feature, j, name);
                    }
                });
            ui.label(format!(
                "{} · stage {} / {}",
                f.cfg.method.label(),
                self.stage,
                n
            ));
        });
        let caption=match f.cfg.method {
            Method::Tree=>"One CART tree: choose splits to reduce squared error; each leaf predicts its training-response mean.",
            Method::Boosting=>"Fit the next tree to current training residuals. New prediction = old prediction + λ × tree correction.",
            Method::Stochastic=>"Draw fresh training rows WITHOUT replacement, fit residuals on that subset, then update predictions for every row.",
            Method::Forest=>"Draw bootstrap rows WITH replacement; randomly choose candidate predictors at each split. Average the trees' response predictions.",
        };
        ui.label(caption);
        ui.label(format!(
            "At this stage: training MSE {:.5}     validation MSE {:.5}",
            f.train_mse[self.stage], f.valid_mse[self.stage]
        ));
        if self.stage == 0 {
            ui.colored_label(ORANGE,format!("Stage 0 is the training mean ({:.5}). For forests and a single tree this is a comparison baseline, not a fitted tree.",f.baseline));
        }
        let j = self.feature;
        let xs: Vec<_> = f.data.x.iter().map(|x| x[j]).collect();
        let mut center = vec![0.0; f.data.names.len()];
        for (k, value) in center.iter_mut().enumerate() {
            let mut v: Vec<_> = f.train.iter().map(|&i| f.data.x[i][k]).collect();
            v.sort_by(f64::total_cmp);
            *value = v[v.len() / 2];
        }
        let lo = xs.iter().copied().fold(f64::INFINITY, f64::min);
        let hi = xs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let grid: Vec<Vec<f64>> = (0..240)
            .map(|i| {
                let mut x = center.clone();
                x[j] = lo + (hi - lo) * i as f64 / 239.0;
                x
            })
            .collect();
        let old_stage = self.stage.saturating_sub(1);
        let old_curve: Vec<_> = grid
            .iter()
            .map(|x| (x[j], f.predict(x, old_stage)))
            .collect();
        let new_curve: Vec<_> = grid
            .iter()
            .map(|x| (x[j], f.predict(x, self.stage)))
            .collect();
        let train_points: Vec<_> = f.train.iter().map(|&i| (xs[i], f.data.y[i])).collect();
        let valid_points: Vec<_> = f.valid.iter().map(|&i| (xs[i], f.data.y[i])).collect();
        let active = self.stage.checked_sub(1).map(|b| &f.trees[b]);
        let raw_curve: Vec<_> = grid
            .iter()
            .map(|x| (x[j], active.map_or(0.0, |t| t.predict(x))))
            .collect();
        let sample = self.stage.checked_sub(1).map(|b| &f.samples[b]);
        let points: Vec<_> = f
            .train
            .iter()
            .map(|&i| {
                (
                    xs[i],
                    if f.cfg.method.boosting() {
                        f.data.y[i] - self.before[i]
                    } else {
                        f.data.y[i]
                    },
                )
            })
            .collect();
        let sampled_points: Vec<_> = sample.map_or(vec![], |s| {
            s.iter()
                .map(|&i| {
                    (
                        xs[i],
                        if f.cfg.method.boosting() {
                            f.data.y[i] - self.before[i]
                        } else {
                            f.data.y[i]
                        },
                    )
                })
                .collect()
        });
        ui.columns(2, |cols| {
            plot(
                &mut cols[0],
                "Before and after this tree",
                &f.data.names[j],
                &f.data.target,
                &[
                    Series::dots("Training responses", train_points, GREY),
                    Series::dots("Validation responses", valid_points, ORANGE),
                    Series::line("Before", old_curve, GREY),
                    Series::line("After", new_curve, BLUE),
                ],
                None,
            );
            plot(
                &mut cols[1],
                if f.cfg.method.boosting() {
                    "Residuals and the selected tree"
                } else {
                    "Responses and the selected tree"
                },
                &f.data.names[j],
                if f.cfg.method.boosting() {
                    "Residual / raw tree output"
                } else {
                    "Response / tree output"
                },
                &[
                    Series::dots("All training rows", points, GREY),
                    Series::dots("Rows sampled for this tree", sampled_points, GREEN),
                    Series::line("Selected tree", raw_curve, ORANGE),
                ],
                None,
            );
        });
        if f.data.names.len() > 1 {
            ui.small("Lines above are conditional slices: all other predictors held at their TRAINING medians. Dots are actual rows. A slice is not a partial-dependence average and need not pass through the dots.");
        }
        let correction: Vec<_> = grid
            .iter()
            .map(|x| (x[j], f.predict(x, self.stage) - f.predict(x, old_stage)))
            .collect();
        let training: Vec<_> = f
            .train_mse
            .iter()
            .enumerate()
            .map(|(i, &v)| (i as f64, v))
            .collect();
        let validation: Vec<_> = f
            .valid_mse
            .iter()
            .enumerate()
            .map(|(i, &v)| (i as f64, v))
            .collect();
        ui.columns(2, |cols| {
            plot(
                &mut cols[0],
                "Change in ensemble prediction",
                &f.data.names[j],
                "After − before",
                &[Series::line(
                    "Contribution at this stage",
                    correction,
                    GREEN,
                )],
                None,
            );
            plot(
                &mut cols[1],
                "Training and validation error",
                "Trees included",
                "Mean squared error",
                &[
                    Series::line("Training", training, BLUE),
                    Series::line("Validation", validation, ORANGE),
                ],
                Some(self.stage as f64),
            );
        });
        ui.separator();
        ui.strong("Follow one observation");
        ui.add(
            egui::Slider::new(&mut self.selected_row, 0..=f.data.y.len() - 1)
                .text("Data row (zero based)"),
        );
        let i = self.selected_row;
        let h = active.map_or(0.0, |t| t.predict(&f.data.x[i]));
        let copies = sample.map_or(0, |s| s.iter().filter(|&&r| r == i).count());
        let set = if f.train.contains(&i) {
            "TRAINING"
        } else {
            "VALIDATION — never fitted"
        };
        ui.label(format!(
            "{set}   ·   sampled {copies} time(s) for this tree   ·   actual y = {:.5}",
            f.data.y[i]
        ));
        if self.stage > 0 && f.cfg.method.boosting() {
            ui.monospace(format!(
                "old prediction = {:.5};  residual = {:.5} − {:.5} = {:.5}",
                self.before[i],
                f.data.y[i],
                self.before[i],
                f.data.y[i] - self.before[i]
            ));
            ui.monospace(format!(
                "new prediction = {:.5} + {:.3} × {:.5} = {:.5}",
                self.before[i], f.cfg.rate, h, self.after[i]
            ));
        } else if self.stage > 0 && f.cfg.method == Method::Forest {
            if self.stage == 1 {
                ui.monospace(format!(
                    "First tree prediction = {h:.5}. Forest average = {h:.5}."
                ));
            } else {
                ui.monospace(format!(
                    "new average = ({} × {:.5} + {:.5}) / {} = {:.5}",
                    self.stage - 1,
                    self.before[i],
                    h,
                    self.stage,
                    self.after[i]
                ));
            }
        } else {
            ui.monospace(format!(
                "prediction = {:.5}; residual = {:.5}",
                self.after[i],
                f.data.y[i] - self.after[i]
            ));
        }
        ui.separator();
        ui.strong("Selected tree structure");
        if let Some(tree) = active {
            tree_diagram(
                ui,
                tree,
                &f.data.names,
                Some((&f.data.x[self.selected_row], 1.0)),
            );
            egui::CollapsingHeader::new("Exact terminal-node rules").show(ui, |ui| {
                let mut rules = vec![];
                tree.rules(&f.data.names, String::new(), &mut rules);
                for rule in rules {
                    ui.monospace(rule);
                }
            });
        } else {
            ui.label("Advance to tree 1 to inspect its splits.");
        }
    }
    fn flow(&mut self, ui: &mut egui::Ui) {
        if self.fitted.is_none() {
            ui.label("Choose your data and click Fit model.");
            return;
        }
        self.stage_controls(ui);
        let f = self.fitted.as_ref().unwrap();
        ui.heading(match f.cfg.method {
            Method::Tree => "From splits to a leaf prediction",
            Method::Boosting => "Small corrections, accumulated",
            Method::Stochastic => "A sampled correction, applied to every row",
            Method::Forest => "Many tree predictions, one average",
        });
        ui.label(format!(
            "{} · depth {} · tree {} of {}",
            f.cfg.method.label(),
            f.cfg.depth,
            self.stage,
            f.trees.len()
        ));
        if self.stage == 0 {
            ui.colored_label(
                BLUE,
                "Click Next to release the first prediction streams. Play advances automatically.",
            );
        }
        let phase = if self.motion {
            ((ui.input(|i| i.time) - self.animation_start) / self.seconds_per_tree).clamp(0.0, 1.0)
        } else {
            1.0
        };
        let sample = self.stage.checked_sub(1).map(|b| &f.samples[b]);
        let row_changed = ui
            .add(
                egui::Slider::new(&mut self.selected_row, 0..=f.data.y.len() - 1)
                    .text("Follow data row (zero based)"),
            )
            .changed();
        if row_changed {
            self.animation_start = ui.input(|i| i.time);
        }
        let selected = self.selected_row;
        let mut rows: Vec<_> = (0..f.data.y.len())
            .step_by((f.data.y.len() / 44).max(1))
            .take(44)
            .collect();
        if !rows.contains(&selected) {
            rows.push(selected);
        }
        let (response, p) =
            ui.allocate_painter(Vec2::new(ui.available_width(), 315.0), egui::Sense::hover());
        let r = response.rect.shrink2(Vec2::new(10.0, 35.0));
        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        for &i in &rows {
            for v in [self.before[i], self.after[i], f.data.y[i]] {
                lo = lo.min(v);
                hi = hi.max(v);
            }
        }
        let pad = if hi > lo { (hi - lo) * 0.12 } else { 1.0 };
        lo -= pad;
        hi += pad;
        let y = |v: f64| r.bottom() - ((v - lo) / (hi - lo)) as f32 * r.height();
        let start_x = r.left() + 85.0;
        let end_x = r.right() - 145.0;
        let truth_x = r.right() - 20.0;
        for (x, label) in [(start_x, "Before"), (end_x, "After"), (truth_x, "Actual y")] {
            p.line_segment(
                [Pos2::new(x, r.top()), Pos2::new(x, r.bottom())],
                Stroke::new(1.0, Color32::LIGHT_GRAY),
            );
            p.text(
                Pos2::new(x, r.top() - 12.0),
                egui::Align2::CENTER_BOTTOM,
                label,
                egui::FontId::proportional(14.0),
                instrument::MUTED,
            );
        }
        for k in 0..=4 {
            let value = lo + (hi - lo) * k as f64 / 4.0;
            p.text(
                Pos2::new(r.left(), y(value)),
                egui::Align2::LEFT_CENTER,
                number(value),
                egui::FontId::proportional(11.0),
                GREY,
            );
        }
        // Draw the selected stream last to preserve its visibility.
        rows.sort_by_key(|&i| i == selected);
        for i in rows {
            let in_sample = sample.is_some_and(|s| s.contains(&i));
            let validation = f.valid.contains(&i);
            let color = if i == selected {
                BLUE
            } else if validation {
                ORANGE
            } else if in_sample {
                GREEN
            } else {
                GREY
            };
            let a = Pos2::new(start_x, y(self.before[i]));
            let b = Pos2::new(end_x, y(self.after[i]));
            let point = |t: f64| {
                Pos2::new(
                    start_x + (end_x - start_x) * t as f32,
                    y(crate::animation::interpolate(
                        self.before[i],
                        self.after[i],
                        t,
                    )),
                )
            };
            let width = if i == selected { 2.6 } else { 1.0 };
            for k in 0..32 {
                p.line_segment(
                    [point(k as f64 / 32.0), point((k + 1) as f64 / 32.0)],
                    Stroke::new(
                        width,
                        color.gamma_multiply(if i == selected { 0.9 } else { 0.22 }),
                    ),
                );
            }
            p.circle_filled(
                a,
                if in_sample { 3.8 } else { 2.0 },
                color.gamma_multiply(0.5),
            );
            let marker = point(phase);
            let radius = if i == selected {
                6.0
            } else if in_sample {
                3.5
            } else {
                2.1
            };
            if self.motion && phase < 1.0 {
                p.circle_filled(marker, radius + 4.0, color.gamma_multiply(0.10));
            }
            p.circle_filled(
                marker,
                radius,
                color.gamma_multiply(if i == selected { 1.0 } else { 0.65 }),
            );
            let target = Pos2::new(truth_x, y(f.data.y[i]));
            p.line_segment(
                [target - Vec2::new(4.0, 0.0), target + Vec2::new(4.0, 0.0)],
                Stroke::new(1.5, color.gamma_multiply(0.45)),
            );
            if i == selected {
                p.line_segment([b, target], Stroke::new(2.0, ORANGE));
                p.circle_stroke(target, 6.0, Stroke::new(2.0, BLUE));
                p.text(
                    a - Vec2::new(8.0, 0.0),
                    egui::Align2::RIGHT_CENTER,
                    format!("{:.3}", self.before[i]),
                    egui::FontId::proportional(13.0),
                    BLUE,
                );
                p.text(
                    b + Vec2::new(8.0, 12.0),
                    egui::Align2::LEFT_TOP,
                    format!("{:.3}", self.after[i]),
                    egui::FontId::proportional(13.0),
                    BLUE,
                );
            }
        }
        ui.horizontal_wrapped(|ui| {
            ui.colored_label(BLUE, "● Followed observation");
            ui.colored_label(GREEN, "● Sampled training row");
            ui.colored_label(GREY, "● Unsampled training row");
            ui.colored_label(ORANGE, "● Validation row / remaining error");
        });
        ui.small("Flow is a visual metaphor: vertical positions are model predictions, stream displacement is the actual update, and the right-hand gap is residual error. Up to 45 rows are drawn; all rows are used in reported scores. Easing changes playback only.");
        let copies = sample.map_or(0, |s| s.iter().filter(|&&i| i == selected).count());
        ui.label(format!("Followed row {}: sampled {} time(s) · residual {:.4} → {:.4} · change in prediction {:+.4}",selected,copies,f.data.y[selected]-self.before[selected],f.data.y[selected]-self.after[selected],self.after[selected]-self.before[selected]));
        if let Some(b) = self.stage.checked_sub(1) {
            if f.cfg.method.boosting() {
                ui.strong(format!(
                    "λ = {:.3}: {:.4} + {:.3} × ({:.4}) = {:.4}",
                    f.cfg.rate,
                    self.before[selected],
                    f.cfg.rate,
                    f.trees[b].predict(&f.data.x[selected]),
                    self.after[selected]
                ));
                ui.label(if f.cfg.method==Method::Stochastic {"The lit rows train this correction. Predictions for unsampled training rows and validation rows are also updated; validation labels never train the tree."}else{"Each correction fits current training residuals. Smaller λ reduces each move; more rounds may then be needed. Watch validation error rather than assuming smaller is always better."});
            } else if f.cfg.method == Method::Forest {
                let votes: Vec<_> = f.trees[..self.stage]
                    .iter()
                    .enumerate()
                    .map(|(k, t)| (k as f64 + 1.0, t.predict(&f.data.x[selected])))
                    .collect();
                let averages: Vec<_> = (1..=self.stage)
                    .map(|k| (k as f64, f.predict(&f.data.x[selected], k)))
                    .collect();
                plot(
                    ui,
                    "Individual trees gather around their running average",
                    "Tree number",
                    "Prediction for followed row",
                    &[
                        Series::dots("Individual tree prediction", votes, ORANGE),
                        Series::line("Running forest average", averages, BLUE),
                    ],
                    Some(self.stage as f64),
                );
                ui.label("A forest averages independently trained tree predictions. Bootstrap rows and random split candidates encourage diversity. Tree disagreement is not a calibrated uncertainty interval.");
            }
            ui.separator();
            ui.strong("Follow the observation through this tree");
            tree_diagram(
                ui,
                &f.trees[b],
                &f.data.names,
                Some((&f.data.x[selected], phase)),
            );
            ui.label("Greater depth permits more conditional branches and interactions; it can also fit training noise. This marker follows the actual split comparisons for the selected row.");
        }
        ui.separator();
        let tr: Vec<_> = f
            .train_mse
            .iter()
            .enumerate()
            .map(|(k, &v)| (k as f64, v))
            .collect();
        let va: Vec<_> = f
            .valid_mse
            .iter()
            .enumerate()
            .map(|(k, &v)| (k as f64, v))
            .collect();
        plot(
            ui,
            "The fit–generalization balance",
            "Trees included",
            "Mean squared error",
            &[
                Series::line("Training", tr, BLUE),
                Series::line("Validation", va, ORANGE),
            ],
            Some(self.stage as f64),
        );
        ui.label(format!(
            "Stage {}: training MSE {:.5} · validation MSE {:.5}",
            self.stage, f.train_mse[self.stage], f.valid_mse[self.stage]
        ));
    }
    fn science_view(&mut self, ui: &mut egui::Ui) {
        let Some(fit) = &self.fitted else {
            ui.label("Choose and prepare a model first.");
            return;
        };
        ui.heading("Model science");
        ui.label("These diagnostics describe prediction and model behavior. Validation data are used for exploration and selection; they are not an untouched test set.");
        let score = science::metrics(&fit.data.y, &self.after, &fit.valid);
        ui.label(format!(
            "Displayed tree {} · MSE {:.5} · RMSE {:.5} · MAE {:.5} · R² {}",
            self.stage,
            score.mse,
            score.rmse,
            score.mae,
            score
                .r2
                .map_or("undefined: constant target".into(), |r| format!("{r:.4}"))
        ));
        let best = fit.best_stage();
        ui.label(format!("Lowest validation MSE on this fitted path: {:.5} at tree {}. Fitted {} of {} requested trees.",fit.valid_mse[best],best,fit.trees.len(),if fit.cfg.method==Method::Tree{1}else{fit.cfg.trees}));
        if fit.cfg.method.boosting() && fit.cfg.patience > 0 {
            ui.small(format!("Early stopping: patience {}, relative improvement threshold {:.4}. The retained path includes patience rounds; Best validation selects the lowest observed score, including the mean baseline if appropriate.",fit.cfg.patience,fit.cfg.min_relative_improvement));
        }
        if fit.cfg.method == Method::Forest {
            ui.separator();
            ui.strong("Out-of-bag prediction");
            match fit.oob_mse[self.stage] {
                Some(loss) => {
                    ui.label(format!(
                        "OOB MSE {:.5} · training-row coverage {:.1}%",
                        loss,
                        100.0 * fit.oob_coverage[self.stage]
                    ));
                }
                None => {
                    ui.label("No OOB estimate at this stage: no training rows have omitted-tree predictions yet.");
                }
            }
            let path: Vec<_> = fit
                .oob_mse
                .iter()
                .enumerate()
                .filter_map(|(k, v)| v.map(|loss| (k as f64, loss)))
                .collect();
            plot(
                ui,
                "Error from trees that did not train on each row",
                "Trees",
                "OOB MSE",
                &[Series::line("Out-of-bag", path, GREEN)],
                Some(self.stage as f64),
            );
            ui.small("Each training row is predicted only by trees whose bootstrap sample omitted it. Coverage changes with tree count, so early path scores can use different subsets of rows. Validation rows are not part of this OOB score.");
        }
        ui.separator();
        ui.strong("Held-out permutation importance and PDP / ICE");
        egui::ComboBox::from_id_salt("science-feature")
            .selected_text(&fit.data.names[self.feature])
            .show_ui(ui, |ui| {
                for (j, name) in fit.data.names.iter().enumerate() {
                    ui.selectable_value(&mut self.feature, j, name);
                }
            });
        if ui
            .add_enabled(
                self.analysis_rx.is_none(),
                egui::Button::new("Analyze full fitted model"),
            )
            .clicked()
        {
            let f = fit.clone();
            let feature = self.feature;
            let revision = self.fit_revision;
            let (tx, rx) = mpsc::channel();
            self.analysis_rx = Some(rx);
            std::thread::spawn(move || {
                let a = science::analyze(&f, feature, f.trees.len());
                let _ = tx.send((revision, a));
            });
        }
        if self.analysis_rx.is_some() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Computing on bounded reference samples…");
            });
        }
        if let Some(a) = &self.analysis {
            ui.label(format!("Analysis at tree {} · {} held-out rows for permutation · {} training reference rows for partial dependence",a.stage,a.validation_rows,a.reference_rows));
            egui::Grid::new("permutation").striped(true).show(ui, |ui| {
                ui.strong("Predictor");
                ui.strong("Increase in validation MSE");
                ui.strong("SD across 3 permutations");
                ui.end_row();
                for (name, mean, sd) in &a.importance {
                    ui.label(name);
                    ui.monospace(format!("{mean:+.5}"));
                    ui.monospace(format!("{sd:.5}"));
                    ui.end_row();
                }
            });
            ui.small("Shuffle one predictor among held-out rows, keeping the model fixed. Negative values are retained. The displayed SD is repeat-to-repeat variation, not a confidence interval. Correlated predictors complicate interpretation.");
            let mut series = vec![];
            for (k, ice) in a.ice.iter().enumerate() {
                series.push(Series::line(
                    if k == 0 {
                        "Individual conditional expectation"
                    } else {
                        ""
                    },
                    a.grid.iter().copied().zip(ice.iter().copied()).collect(),
                    GREY.gamma_multiply(0.6),
                ));
            }
            series.push(Series::line(
                "Partial dependence average",
                a.grid.iter().copied().zip(a.pdp.iter().copied()).collect(),
                BLUE,
            ));
            plot(
                ui,
                "Average and individual fitted responses",
                &fit.data.names[a.feature],
                "Predicted response",
                &series,
                None,
            );
            ui.small("PDP averages over the full sampled training reference set; up to 12 reference ICE curves are drawn. These are model predictions under substituted feature values, not causal effects. Correlated features can produce unsupported combinations. The Explore curve is a median slice, a different object.");
        }
    }
    fn experiments(&mut self, ui: &mut egui::Ui) {
        ui.heading("Experiments");
        ui.label("Change one decision, keep the data and seed fixed, and compare what the model actually does.");
        let titles = [
            "The learning-rate race",
            "Find an interaction",
            "Forest diversity",
            "Find the stopping point",
        ];
        let tasks=[
            "Finish and pin the larger-step model. Reduce learning rate to 0.03 at the same tree count; finish and compare. Then try more trees. A slower learner can look worse when it has too few rounds.",
            "Finish and pin depth-one boosting. Change depth to 3, then finish and compare. Stumps remain additive; deeper trees can represent interactions. Improvement is an empirical question.",
            "Finish and pin a forest using one candidate predictor per split. Increase that number, refit and compare both validation and OOB errors. More diverse trees need not individually predict better.",
            "Finish the noisy-data fit and pin it. Jump to Best validation and compare the earlier and final stages. Then enable early stopping and inspect how many rounds it retains.",
        ];
        let mut start = None;
        egui::CollapsingHeader::new("Choose an experiment")
            .default_open(self.challenge.is_none())
            .show(ui, |ui| {
                ui.columns(2, |cols| {
                    for i in 0..4 {
                        egui::Frame::group(cols[i % 2].style())
                            .inner_margin(12.0)
                            .show(&mut cols[i % 2], |ui| {
                                ui.strong(titles[i]);
                                ui.label(tasks[i]);
                                if ui
                                    .add_enabled(
                                        self.receiver.is_none(),
                                        egui::Button::new("Start this experiment"),
                                    )
                                    .clicked()
                                {
                                    start = Some(i);
                                }
                            });
                    }
                });
            });
        if let Some(i) = start {
            self.pinned = None;
            self.analysis = None;
            self.challenge = Some(i);
            match i {
                0 => {
                    self.set_table(Table::demo(0));
                    self.features = vec![true, false, false];
                    self.scenario = 0;
                    self.cfg = Config {
                        trees: 80,
                        depth: 1,
                        rate: 0.3,
                        ..Default::default()
                    };
                }
                1 => {
                    self.set_table(Table::demo(1));
                    self.scenario = 1;
                    self.cfg = Config {
                        trees: 100,
                        depth: 1,
                        rate: 0.1,
                        ..Default::default()
                    };
                }
                2 => {
                    self.set_table(examples::table(1).unwrap());
                    self.scenario = 3;
                    self.cfg = Config {
                        method: Method::Forest,
                        trees: 80,
                        depth: 5,
                        mtry: 1,
                        ..Default::default()
                    };
                }
                _ => {
                    self.set_table(Table::demo(2));
                    self.scenario = 6;
                    self.features = vec![true, false, false];
                    self.cfg = Config {
                        trees: 180,
                        depth: 4,
                        rate: 0.15,
                        ..Default::default()
                    };
                }
            }
            self.start_fit();
            self.tab = 6;
        }
        if let Some(i) = self.challenge {
            ui.strong(titles[i]);
            ui.label(tasks[i]);
        }
        let Some(fit) = &self.fitted else {
            ui.label("Prepare a model to begin comparing.");
            return;
        };
        ui.separator();
        ui.heading("Your comparison");
        ui.horizontal(|ui| {
            if ui.button("Pin displayed model").clicked() {
                self.pinned = Some((fit.clone(), self.stage));
            }
            if ui.button("Return to Explore").clicked() {
                self.tab = 6;
            }
            if ui.button("Clear pin").clicked() {
                self.pinned = None;
            }
        });
        let Some((pin, pin_stage)) = &self.pinned else {
            ui.label("Finish or choose a stage in Explore, then pin it. Change a setting and return here to compare against that fixed model.");
            return;
        };
        if pin.data != fit.data {
            ui.colored_label(ORANGE,"The pinned model uses different data or selected columns. Pin this dataset before making a matched comparison.");
            return;
        }
        let same_split = pin.train == fit.train && pin.valid == fit.valid;
        if !same_split {
            ui.colored_label(ORANGE,"The models use different train/validation partitions. The scores below are not a matched validation comparison.");
        }
        let a = science::metrics(&pin.data.y, &pin.stage_predictions[*pin_stage], &pin.valid);
        let b = science::metrics(&fit.data.y, &fit.stage_predictions[self.stage], &fit.valid);
        ui.columns(2, |cols| {
            cols[0].strong("Pinned model");
            cols[0].label(format!(
                "{} · tree {} · depth {} · learning rate {:.3}",
                pin.cfg.method.label(),
                pin_stage,
                pin.cfg.depth,
                pin.cfg.rate
            ));
            cols[0].label(format!("Validation RMSE {:.4} · MAE {:.4}", a.rmse, a.mae));
            cols[1].strong("Current displayed model");
            cols[1].label(format!(
                "{} · tree {} · depth {} · learning rate {:.3}",
                fit.cfg.method.label(),
                self.stage,
                fit.cfg.depth,
                fit.cfg.rate
            ));
            cols[1].label(format!("Validation RMSE {:.4} · MAE {:.4}", b.rmse, b.mae));
        });
        if same_split {
            ui.colored_label(if b.mse<a.mse{GREEN}else{ORANGE},format!("Current minus pinned validation MSE: {:+.5}. {} on this fixed validation split.",b.mse-a.mse,if b.mse<a.mse{"The current displayed model scored lower"}else if b.mse>a.mse{"The pinned model scored lower"}else{"The two displayed models tied"}));
        }
        let j = self.feature.min(fit.data.names.len() - 1);
        let mut center = vec![0.0; fit.data.names.len()];
        for (k, v) in center.iter_mut().enumerate() {
            let mut x: Vec<_> = fit.train.iter().map(|&i| fit.data.x[i][k]).collect();
            x.sort_by(f64::total_cmp);
            *v = x[x.len() / 2];
        }
        let lo = fit
            .data
            .x
            .iter()
            .map(|x| x[j])
            .fold(f64::INFINITY, f64::min);
        let hi = fit
            .data
            .x
            .iter()
            .map(|x| x[j])
            .fold(f64::NEG_INFINITY, f64::max);
        let rows: Vec<_> = (0..240)
            .map(|k| {
                let mut x = center.clone();
                x[j] = lo + (hi - lo) * k as f64 / 239.0;
                x
            })
            .collect();
        plot(
            ui,
            "Same input slice, two models",
            &fit.data.names[j],
            "Predicted response",
            &[
                Series::line(
                    "Pinned",
                    rows.iter()
                        .map(|x| (x[j], pin.predict(x, *pin_stage)))
                        .collect(),
                    ORANGE,
                ),
                Series::line(
                    "Current",
                    rows.iter()
                        .map(|x| (x[j], fit.predict(x, self.stage)))
                        .collect(),
                    BLUE,
                ),
            ],
            None,
        );
        ui.small("Both lines use the same substituted inputs and current training medians. Repeated comparison tunes against this validation set; it does not create an independent test result.");
    }
    fn diagnostics(&mut self, ui: &mut egui::Ui) {
        let Some(f) = &self.fitted else {
            ui.label("Fit a model first.");
            return;
        };
        ui.heading("Selected-stage diagnostics");
        let training: Vec<_> = f
            .train
            .iter()
            .map(|&i| (f.data.y[i], self.after[i]))
            .collect();
        let validation: Vec<_> = f
            .valid
            .iter()
            .map(|&i| (f.data.y[i], self.after[i]))
            .collect();
        let lo = f
            .data
            .y
            .iter()
            .chain(&self.after)
            .copied()
            .fold(f64::INFINITY, f64::min);
        let hi = f
            .data
            .y
            .iter()
            .chain(&self.after)
            .copied()
            .fold(f64::NEG_INFINITY, f64::max);
        let res: Vec<_> = f
            .valid
            .iter()
            .map(|&i| (self.after[i], f.data.y[i] - self.after[i]))
            .collect();
        ui.columns(2, |cols| {
            plot(
                &mut cols[0],
                "Observed versus predicted",
                "Observed response",
                "Predicted response",
                &[
                    Series::dots("Training", training, BLUE),
                    Series::dots("Validation", validation, ORANGE),
                    Series::line("Perfect prediction", vec![(lo, lo), (hi, hi)], GREY),
                ],
                None,
            );
            plot(
                &mut cols[1],
                "Validation residuals",
                "Predicted response",
                "Observed − predicted",
                &[Series::dots("Validation rows", res, ORANGE)],
                None,
            );
        });
        let best = (1..f.valid_mse.len())
            .min_by(|&a, &b| f.valid_mse[a].total_cmp(&f.valid_mse[b]))
            .unwrap_or(0);
        ui.label(format!(
            "Lowest validation MSE along the fitted path: {:.5} at {} tree(s).",
            f.valid_mse[best], best
        ));
        ui.label("This validation set is used for exploration and tuning. Repeatedly selecting against it makes it unsuitable as an untouched final test set.");
        ui.separator();
        ui.strong("Full fitted model: impurity importance");
        for (name, &v) in f.data.names.iter().zip(&f.importance) {
            ui.horizontal(|ui| {
                ui.label(name);
                ui.add(
                    egui::ProgressBar::new(v as f32)
                        .text(format!("{:.1}%", 100.0 * v))
                        .desired_width(300.0),
                );
            });
        }
        ui.label("Normalized training SSE reductions across all fitted trees. This is predictive usage, not a causal effect; correlated or high-cardinality predictors can complicate importance.");
        ui.separator();
        if ui.button("Export predictions at selected stage…").clicked() {
            match choose_path(true) {
                Ok(Some(path)) => {
                    let mut csv = String::from(
                        "data_row,partition,actual,predicted,residual,trees_included\n",
                    );
                    for i in 0..f.data.y.len() {
                        csv.push_str(&format!(
                            "{},{},{},{},{},{}\n",
                            i + 1,
                            if f.train.contains(&i) {
                                "training"
                            } else {
                                "validation"
                            },
                            f.data.y[i],
                            self.after[i],
                            f.data.y[i] - self.after[i],
                            self.stage
                        ));
                    }
                    match std::fs::write(&path, csv) {
                        Ok(()) => {
                            self.status = format!(
                                "Exported stage {} predictions to {}",
                                self.stage,
                                path.display()
                            );
                            self.error.clear();
                        }
                        Err(e) => self.error = format!("Export failed: {e}"),
                    }
                }
                Ok(None) => {}
                Err(e) => self.error = e,
            }
        }
    }
    fn data_view(&self, ui: &mut egui::Ui) {
        ui.heading(&self.table.name);
        ui.label("Read-only preview · first 100 rows · all original columns retained in memory");
        egui::ScrollArea::horizontal().show(ui, |ui| {
            egui::Grid::new("data-preview")
                .striped(true)
                .show(ui, |ui| {
                    ui.strong("Row");
                    for h in &self.table.headers {
                        ui.strong(h);
                    }
                    ui.end_row();
                    for (i, row) in self.table.rows.iter().take(100).enumerate() {
                        ui.label((i + 1).to_string());
                        for value in row {
                            ui.label(value);
                        }
                        ui.end_row();
                    }
                });
        });
    }
    fn python_view(&mut self, ui: &mut egui::Ui) {
        ui.heading("Python workspace");
        ui.label("Generated code follows the CURRENT controls. The practice editor keeps your changes. Running code is an immutable snapshot of the program actually executing.");
        ui.horizontal(|ui| {
            ui.selectable_value(
                &mut self.python.library,
                false,
                "Transparent Python · no packages",
            );
            ui.selectable_value(
                &mut self.python.library,
                true,
                "Class-library Python · scikit-learn",
            );
        });
        let generated = if self.python.library {
            python::sklearn_code(&self.cfg)
        } else {
            python::generated(&self.cfg)
        };
        ui.strong(match self.cfg.method {
            Method::Tree=>"Equation: leaf prediction = mean of training responses in that leaf; splits minimize within-leaf SSE.",
            Method::Boosting|Method::Stochastic=>"Equations: residual = y - F_old(x);  F_new(x) = F_old(x) + learning_rate * tree(x).",
            Method::Forest=>"Equation: forest(x) = sum of individual tree predictions / number of trees. No residual fitting.",
        });
        ui.label("To experiment with an equation, copy the generated code into Practice and edit the residual or update expression. Those edits affect the Python run; they do not silently change the native Rust model.");
        ui.horizontal(|ui| {
            ui.label("Python 3 interpreter");
            ui.text_edit_singleline(&mut self.python.interpreter);
            ui.add_enabled_ui(!self.python.running, |ui| {
                ui.checkbox(&mut self.python.step, "Pause after each round");
            });
        });
        ui.small("Run executes local Python with your account's file access. Transparent Python needs only the standard library. The class-library version requires scikit-learn in the chosen interpreter. No packages are silently installed.");
        let mut run = false;
        ui.horizontal(|ui| {
            if ui
                .add_enabled(!self.python.running, egui::Button::new("Run selected code"))
                .clicked()
            {
                run = true;
            }
            if ui
                .add_enabled(
                    self.python.running && self.python.waiting,
                    egui::Button::new("Next Python round"),
                )
                .clicked()
            {
                if let Some(s) = self.python.session.as_mut() {
                    match s.next() {
                        Ok(()) => {
                            self.python.waiting = false;
                            self.python.message = "Python advancing…".into();
                        }
                        Err(e) => self.python.message = e,
                    }
                }
            }
            if ui
                .add_enabled(self.python.running, egui::Button::new("Stop Python"))
                .clicked()
            {
                if let Some(s) = self.python.session.as_mut() {
                    s.stop();
                }
                self.python.running = false;
                self.python.waiting = false;
                self.python.message = "Python stopped".into();
            }
            if ui
                .button(if self.python.draft.is_empty() {
                    "Copy generated to practice editor"
                } else {
                    "Replace practice editor with generated code"
                })
                .clicked()
            {
                self.python.draft = generated.clone();
                self.python.source_mode = 1;
            }
        });
        ui.horizontal(|ui| {
            if ui.button("Open a .py file into practice…").clicked() {
                match choose_path(false) {
                    Ok(Some(path)) => match std::fs::read_to_string(path) {
                        Ok(code) if code.len() <= 1_000_000 => {
                            self.python.draft = code;
                            self.python.source_mode = 1;
                        }
                        Ok(_) => self.python.message = "Python source exceeds 1 MB.".into(),
                        Err(e) => self.python.message = e.to_string(),
                    },
                    Ok(None) => {}
                    Err(e) => self.python.message = e,
                }
            }
            if ui.button("Export selected code + data + helper…").clicked() {
                let code = match self.python.source_mode {
                    1 => self.python.draft.as_str(),
                    2 => self
                        .python
                        .session
                        .as_ref()
                        .map_or(generated.as_str(), |s| s.code.as_str()),
                    _ => generated.as_str(),
                };
                let features: Vec<_> = self
                    .features
                    .iter()
                    .enumerate()
                    .filter_map(|(j, &v)| (v && j != self.target).then_some(j))
                    .collect();
                let data = if self.python.source_mode == 2 {
                    self.python
                        .session
                        .as_ref()
                        .map(|s| s.data.clone())
                        .ok_or_else(|| "No executed program to export.".to_owned())
                } else {
                    self.table.dataset(self.target, &features)
                };
                match (choose_folder(), data) {
                    (Ok(Some(parent)), Ok(data)) => {
                        match python::export_bundle(&parent, code, &data) {
                            Ok(dir) => {
                                self.python.message =
                                    format!("Exported Python bundle: {}", dir.display())
                            }
                            Err(e) => self.python.message = e,
                        }
                    }
                    (Err(e), _) | (_, Err(e)) => self.python.message = e,
                    _ => {}
                }
            }
        });
        ui.horizontal(|ui| {
            ui.selectable_value(
                &mut self.python.source_mode,
                0,
                "Generated · follows settings",
            );
            ui.selectable_value(&mut self.python.source_mode, 1, "Practice · editable");
            ui.add_enabled_ui(self.python.session.is_some(), |ui| {
                ui.selectable_value(
                    &mut self.python.source_mode,
                    2,
                    "Running / last executed code",
                );
            });
            ui.checkbox(&mut self.follow_code, "Follow executed line");
        });
        if run {
            let code = match self.python.source_mode {
                1 => self.python.draft.clone(),
                2 => self
                    .python
                    .session
                    .as_ref()
                    .map_or(generated.clone(), |s| s.code.clone()),
                _ => generated.clone(),
            };
            if code.trim().is_empty() {
                self.python.message = "Copy or write some Python code first.".into();
            } else {
                let features: Vec<_> = self
                    .features
                    .iter()
                    .enumerate()
                    .filter_map(|(j, &v)| (v && j != self.target).then_some(j))
                    .collect();
                match self.table.dataset(self.target, &features) {
                    Ok(d) => self.python.launch(
                        code,
                        d,
                        format!("{} · selected Python source", self.table.name),
                    ),
                    Err(e) => self.python.message = e,
                }
            }
        }
        ui.colored_label(
            if self.python.running { BLUE } else { GREY },
            &self.python.message,
        );
        if let Some(s) = &self.python.session {
            ui.label(format!(
                "Run snapshot: {} · {} rows · Python round {}",
                s.label,
                s.data.y.len(),
                self.python.stage
            ));
        }
        ui.columns(2, |cols| {
            cols[0].strong("Code");
            if self.python.source_mode == 1 {
                egui::ScrollArea::both()
                    .id_salt("python-editor")
                    .max_height(480.0)
                    .show(&mut cols[0], |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut self.python.draft)
                                .code_editor()
                                .desired_rows(24)
                                .desired_width(f32::INFINITY),
                        );
                    });
            } else {
                let source = if self.python.source_mode == 2 {
                    self.python
                        .session
                        .as_ref()
                        .map_or(generated.as_str(), |s| s.code.as_str())
                } else {
                    generated.as_str()
                };
                egui::ScrollArea::both()
                    .id_salt("python-code-view")
                    .max_height(480.0)
                    .show(&mut cols[0], |ui| {
                        for (i, line) in source.lines().enumerate() {
                            let active = self.python.source_mode == 2 && self.python.line == i + 1;
                            let text = egui::RichText::new(format!("{:>3}  {}", i + 1, line))
                                .monospace()
                                .background_color(if active {
                                    Color32::from_rgb(214, 236, 254)
                                } else {
                                    Color32::TRANSPARENT
                                });
                            let response = ui.label(text);
                            if active && self.python.running && self.follow_code {
                                response.scroll_to_me(Some(egui::Align::Center));
                            }
                        }
                    });
            }
            cols[1].strong("Actual Python results");
            cols[1].monospace(format!(
                "Executed source line: {}\nLocals: {}",
                self.python.line, self.python.locals
            ));
            if !self.python.train.is_empty() {
                plot(
                    &mut cols[1],
                    "Python training and validation error",
                    "Python round",
                    "Mean squared error",
                    &[
                        Series::line("Training", self.python.train.clone(), BLUE),
                        Series::line("Validation", self.python.valid.clone(), ORANGE),
                    ],
                    Some(self.python.stage as f64),
                );
                if let (Some(a), Some(b)) = (self.python.train.last(), self.python.valid.last()) {
                    cols[1].label(format!(
                        "Training MSE {:.6}; validation MSE {:.6}",
                        a.1, b.1
                    ));
                }
            }
            if let Some(s) = &self.python.session {
                if self.python.predictions.len() == s.data.y.len() {
                    let points: Vec<_> = s
                        .data
                        .y
                        .iter()
                        .zip(&self.python.predictions)
                        .map(|(&a, &b)| (a, b))
                        .collect();
                    plot(
                        &mut cols[1],
                        "Python observed vs predicted",
                        "Observed response",
                        "Python prediction",
                        &[Series::dots("All rows in run snapshot", points, BLUE)],
                        None,
                    );
                }
            }
        });
        ui.separator();
        ui.strong("Python output / errors");
        egui::ScrollArea::vertical()
            .id_salt("python-output")
            .max_height(160.0)
            .stick_to_bottom(true)
            .show(ui, |ui| {
                for line in &self.python.output {
                    ui.monospace(line);
                }
            });
        egui::CollapsingHeader::new("Inspect the actual Python tree-fitting helper").show(
            ui,
            |ui| {
                egui::ScrollArea::both()
                    .id_salt("python-helper")
                    .max_height(400.0)
                    .show(ui, |ui| {
                        ui.monospace(include_str!("../python/lab_runtime.py"));
                    });
            },
        );
        ui.small("Python plots show that run's actual results. Rust, transparent Python and scikit-learn use different RNGs and implementation conventions; predictions need not match. Package-code availability does not establish a successful package run: inspect completion/errors above.");
    }
    fn quiz(&mut self, ui: &mut egui::Ui) {
        let bank = quiz::questions();
        let n = bank.len();
        ui.heading("Practice questions");
        ui.label("Original practice based on the course topics. These are not actual or predicted quiz questions. Answers remain hidden until Check answer.");
        ui.horizontal_wrapped(|ui| {
            egui::ComboBox::from_label("Topic")
                .selected_text(&self.quiz_topic)
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut self.quiz_topic,
                        "All topics".to_owned(),
                        "All topics",
                    );
                    let topics: std::collections::BTreeSet<_> =
                        bank.iter().map(|q| q.topic.as_str()).collect();
                    for topic in topics {
                        ui.selectable_value(&mut self.quiz_topic, topic.to_owned(), topic);
                    }
                });
            ui.checkbox(&mut self.quiz_review_only, "Review missed");
            if ui.button("Shuffle question order").clicked() {
                use rand::seq::SliceRandom;
                use rand::SeedableRng;
                self.quiz_shuffle += 1;
                let mut rng = rand::rngs::StdRng::seed_from_u64(4710 + self.quiz_shuffle);
                self.quiz_order.shuffle(&mut rng);
            }
        });
        let visible: Vec<_> = self
            .quiz_order
            .iter()
            .copied()
            .filter(|&i| {
                (self.quiz_topic == "All topics" || bank[i].topic == self.quiz_topic)
                    && (!self.quiz_review_only
                        || (self.quiz_checked[i]
                            && (0..4)
                                .filter(|&j| self.quiz_answers[i][j])
                                .collect::<Vec<_>>()
                                != bank[i].correct))
            })
            .collect();
        if visible.is_empty() {
            ui.label("No questions match this selection. Turn off Review missed or choose another topic.");
            return;
        }
        if !visible.contains(&self.quiz_index) {
            self.quiz_index = visible[0];
        }
        let position = visible.iter().position(|&i| i == self.quiz_index).unwrap();
        ui.label(format!(
            "{} of {} in this selection · {} total questions",
            position + 1,
            visible.len(),
            n
        ));
        egui::CollapsingHeader::new("Question map").show(ui, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("question-map")
                .max_height(180.0)
                .show(ui, |ui| {
                    egui::Grid::new("quiz-question-grid").show(ui, |ui| {
                        for (k, &i) in visible.iter().enumerate() {
                            let correct = (0..4)
                                .filter(|&j| self.quiz_answers[i][j])
                                .collect::<Vec<_>>()
                                == bank[i].correct;
                            let text = egui::RichText::new((i + 1).to_string()).monospace().color(
                                if self.quiz_checked[i] {
                                    if correct {
                                        GREEN
                                    } else {
                                        ORANGE
                                    }
                                } else {
                                    instrument::TEXT
                                },
                            );
                            if ui.selectable_label(self.quiz_index == i, text).clicked() {
                                self.quiz_index = i;
                            }
                            if k % 10 == 9 {
                                ui.end_row();
                            }
                        }
                    });
                });
        });
        ui.separator();
        let i = self.quiz_index;
        let q = &bank[i];
        ui.small(&q.topic);
        ui.heading(format!("{}. {}", q.id, q.title));
        ui.strong(if q.correct.len() > 1 {
            "Select all that apply · exact set required"
        } else {
            "Select one answer"
        });
        ui.add_space(10.0);
        ui.label(&q.stem);
        ui.add_space(10.0);
        if !q.code.is_empty() {
            instrument::glass().show(ui, |ui| {
                ui.monospace(&q.code);
            });
            if ui.button("Copy snippet to code editor").clicked() {
                self.python.draft = q.code.clone();
                self.python.source_mode = 1;
                self.tab = 5;
            }
            ui.small("Some snippets assume the variables described in the question.");
        }
        ui.add_enabled_ui(!self.quiz_checked[i], |ui| {
            for j in 0..4 {
                let text = format!("{}. {}", (b'A' + j as u8) as char, q.options[j]);
                if q.correct.len() > 1 {
                    ui.checkbox(&mut self.quiz_answers[i][j], text);
                } else if ui.radio(self.quiz_answers[i][j], text).clicked() {
                    self.quiz_answers[i] = [false; 4];
                    self.quiz_answers[i][j] = true;
                }
            }
        });
        ui.add_space(12.0);
        if ui
            .add_enabled(
                self.quiz_answers[i].iter().any(|&v| v) && !self.quiz_checked[i],
                egui::Button::new("Check answer"),
            )
            .clicked()
        {
            self.quiz_checked[i] = true;
        }
        if self.quiz_checked[i] {
            let selected: Vec<_> = (0..4).filter(|&j| self.quiz_answers[i][j]).collect();
            let correct = selected == q.correct;
            ui.colored_label(
                if correct { GREEN } else { RED },
                if correct {
                    "Correct"
                } else {
                    "Review this answer"
                },
            );
            let letters: Vec<_> = q
                .correct
                .iter()
                .map(|&j| ((b'A' + j as u8) as char).to_string())
                .collect();
            ui.strong(format!("Correct answer: {}", letters.join(", ")));
            ui.label(&q.explanation);
            if ui.button("Try this question again").clicked() {
                self.quiz_checked[i] = false;
                self.quiz_answers[i] = [false; 4];
            }
        }
        ui.add_space(20.0);
        ui.horizontal(|ui| {
            if ui.button("Previous question").clicked() {
                self.quiz_index = visible[position.saturating_sub(1)];
            }
            if ui.button("Next question").clicked() {
                self.quiz_index = visible[(position + 1).min(visible.len() - 1)];
            }
            if ui.button("Reset all answers").clicked() {
                self.quiz_answers.fill([false; 4]);
                self.quiz_checked.fill(false);
                self.quiz_index = 0;
            }
        });
        let checked = self.quiz_checked.iter().filter(|&&b| b).count();
        let score = (0..n)
            .filter(|&k| {
                self.quiz_checked[k]
                    && (0..4)
                        .filter(|&j| self.quiz_answers[k][j])
                        .collect::<Vec<_>>()
                        == bank[k].correct
            })
            .count();
        ui.label(format!("{score} correct / {checked} checked · {n} questions · session only, no stored learner record"));
    }
}

impl Lab {
    /// Legacy smoke-test workflow driven by `--ui-smoke-test`.
    #[cfg(feature = "acceptance")]
    fn smoke_hooks(&mut self, ctx: &egui::Context) {
        if std::env::args().any(|a| a == "--ui-smoke-test") {
            if self.fitted.is_some() {
                self.smoke_frames += 1;
            }
            for event in ctx.input(|i| i.events.clone()) {
                if let egui::Event::Screenshot { image, .. } = event {
                    if let Some(path) = crate::acceptance::argument("--screenshot") {
                        self.smoke_screenshot = crate::acceptance::save_png(&path, &image).is_ok();
                    }
                }
            }
            match self.smoke_frames {
                5 => self.tab = 7,
                10 => self.tab = 8,
                15 => self.tab = 5,
                20 => {
                    self.tab = 4;
                    self.quiz_index = 90;
                }
                25 => {
                    self.tab = 0;
                    self.stage = 3.min(self.fitted.as_ref().unwrap().trees.len());
                    self.refresh_stage();
                }
                30 => {
                    self.tab = 6;
                    self.settings_layer = 2;
                }
                35 => ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(
                    egui::UserData::default(),
                )),
                _ => {}
            }
            if self.smoke_frames > 40 && self.smoke_screenshot {
                if let Some(path) = crate::acceptance::argument("--report") {
                    let report = serde_json::json!({"status":"passed","os":std::env::consts::OS,"native_window_frames":self.smoke_frames,"views_rendered":["explore","science","experiments","python","quiz-code-question","tree-details","encoder-controls"],"screenshot":true,"question_count":quiz::questions().len()});
                    if crate::acceptance::write(&path, &report).is_ok() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                }
            }
            ctx.request_repaint_after(std::time::Duration::from_millis(25));
        }
    }
    #[cfg(not(feature = "acceptance"))]
    fn smoke_hooks(&mut self, _ctx: &egui::Context) {}
}
impl eframe::App for Lab {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll(&ctx);
        let dropped = ctx.input(|i| i.raw.dropped_files.first().map(|f| f.path().to_path_buf()));
        if self.receiver.is_none() {
            if let Some(p) = dropped {
                self.scenario = 5;
                self.load_file(p);
                if self.error.is_empty() {
                    self.start_fit();
                }
            }
        }
        egui::Panel::top("header").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.add_space(9.0);
                ui.label(
                    egui::RichText::new("TREE / LAB")
                        .monospace()
                        .size(21.0)
                        .strong(),
                );
                ui.label(
                    egui::RichText::new("ENSEMBLE INSTRUMENT  /  04")
                        .monospace()
                        .size(10.0)
                        .color(GREY),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(self.cfg.method.label())
                            .monospace()
                            .size(11.0)
                            .color(BLUE),
                    );
                });
            });
        });
        egui::Panel::bottom("status").show(ui, |ui| {
            ui.label(&self.status);
            if !self.error.is_empty() {
                ui.colored_label(RED, &self.error);
            }
        });
        egui::Panel::left("mode-dock")
            .default_size(96.0)
            .resizable(false)
            .show(ui, |ui| {
                ui.add_space(12.0);
                for (index, label, tab) in [
                    ("01", "EXPLORE", 6),
                    ("02", "LABS", 8),
                    ("03", "CODE", 5),
                    ("04", "QUIZ", 4),
                    ("05", "SCIENCE", 7),
                ] {
                    if instrument::mode_key(ui, index, label, self.tab == tab).clicked() {
                        self.tab = tab;
                    }
                    ui.add_space(4.0);
                }
                ui.add_space(12.0);
                ui.separator();
                ui.label(
                    egui::RichText::new("DETAILS")
                        .monospace()
                        .size(9.0)
                        .color(GREY),
                );
                for (tab, label) in [(0, "Tree"), (1, "Flow"), (2, "Scores"), (3, "Data")] {
                    if ui.selectable_label(self.tab == tab, label).clicked() {
                        self.tab = tab;
                    }
                }
            });
        egui::Panel::right("controls")
            .default_size(280.0)
            .resizable(true)
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| self.sidebar(ui));
            });
        egui::CentralPanel::default().show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                if !self.example_lesson.is_empty() && self.tab != 6 {
                    egui::CollapsingHeader::new("About this example").show(ui, |ui| {
                        ui.label(&self.example_lesson);
                    });
                }
                match self.tab {
                    0 => self.process(ui),
                    1 => self.flow(ui),
                    2 => self.diagnostics(ui),
                    3 => self.data_view(ui),
                    4 => self.quiz(ui),
                    5 => self.python_view(ui),
                    7 => self.science_view(ui),
                    8 => self.experiments(ui),
                    _ => self.explore(ui),
                }
            });
        });
        self.smoke_hooks(&ctx);
    }
}

struct Series {
    name: &'static str,
    points: Vec<(f64, f64)>,
    color: Color32,
    line: bool,
}
impl Series {
    fn line(name: &'static str, points: Vec<(f64, f64)>, color: Color32) -> Self {
        Self {
            name,
            points,
            color,
            line: true,
        }
    }
    fn dots(name: &'static str, points: Vec<(f64, f64)>, color: Color32) -> Self {
        Self {
            name,
            points,
            color,
            line: false,
        }
    }
}
fn plot(
    ui: &mut egui::Ui,
    title: &str,
    x_label: &str,
    y_label: &str,
    series: &[Series],
    stage: Option<f64>,
) {
    ui.strong(title);
    let (response, p) = ui.allocate_painter(
        Vec2::new(ui.available_width().max(220.0), 245.0),
        egui::Sense::hover(),
    );
    let rect = response.rect;
    let graph = Rect::from_min_max(
        rect.min + Vec2::new(65.0, 12.0),
        rect.max - Vec2::new(12.0, 42.0),
    );
    let mut xmin = f64::INFINITY;
    let mut xmax = f64::NEG_INFINITY;
    let mut ymin = f64::INFINITY;
    let mut ymax = f64::NEG_INFINITY;
    for s in series {
        for &(x, y) in &s.points {
            if x.is_finite() && y.is_finite() {
                xmin = xmin.min(x);
                xmax = xmax.max(x);
                ymin = ymin.min(y);
                ymax = ymax.max(y);
            }
        }
    }
    if !xmin.is_finite() {
        xmin = 0.;
        xmax = 1.;
        ymin = 0.;
        ymax = 1.;
    }
    let dx = if xmax > xmin {
        (xmax - xmin) * 0.04
    } else {
        1.0
    };
    let dy = if ymax > ymin {
        (ymax - ymin) * 0.08
    } else {
        1.0
    };
    xmin -= dx;
    xmax += dx;
    ymin -= dy;
    ymax += dy;
    let map = |x: f64, y: f64| {
        Pos2::new(
            graph.left() + ((x - xmin) / (xmax - xmin)) as f32 * graph.width(),
            graph.bottom() - ((y - ymin) / (ymax - ymin)) as f32 * graph.height(),
        )
    };
    p.rect_stroke(
        graph,
        0.0,
        Stroke::new(1.0, Color32::from_gray(185)),
        egui::StrokeKind::Inside,
    );
    for k in 0..=4 {
        let x = xmin + (xmax - xmin) * k as f64 / 4.;
        let y = ymin + (ymax - ymin) * k as f64 / 4.;
        let a = map(x, ymin);
        let b = map(xmin, y);
        p.text(
            a + Vec2::new(0.0, 7.0),
            egui::Align2::CENTER_TOP,
            number(x),
            egui::FontId::proportional(11.0),
            instrument::MUTED,
        );
        p.text(
            b - Vec2::new(7.0, 0.0),
            egui::Align2::RIGHT_CENTER,
            number(y),
            egui::FontId::proportional(11.0),
            instrument::MUTED,
        );
    }
    p.text(
        Pos2::new(graph.center().x, rect.bottom() - 4.0),
        egui::Align2::CENTER_BOTTOM,
        x_label,
        egui::FontId::proportional(12.0),
        instrument::MUTED,
    );
    p.text(
        rect.min,
        egui::Align2::LEFT_TOP,
        y_label,
        egui::FontId::proportional(10.0),
        instrument::MUTED,
    );
    let clipped = p.with_clip_rect(graph.shrink(1.0));
    if ymin <= 0.0 && ymax >= 0.0 {
        clipped.line_segment(
            [map(xmin, 0.0), map(xmax, 0.0)],
            Stroke::new(1.0, Color32::LIGHT_GRAY),
        );
    }
    for s in series {
        if s.line {
            for pair in s.points.windows(2) {
                clipped.line_segment(
                    [map(pair[0].0, pair[0].1), map(pair[1].0, pair[1].1)],
                    Stroke::new(2.0, s.color),
                );
            }
        } else {
            for &(x, y) in &s.points {
                clipped.circle_filled(map(x, y), 2.5, s.color.gamma_multiply(0.75));
            }
        }
    }
    if let Some(x) = stage {
        clipped.line_segment([map(x, ymin), map(x, ymax)], Stroke::new(1.5, GREEN));
    }
    ui.horizontal_wrapped(|ui| {
        for s in series {
            if !s.name.is_empty() {
                ui.colored_label(s.color, s.name);
            }
        }
    });
}
fn number(v: f64) -> String {
    if v.abs() >= 10000. || (v != 0. && v.abs() < 0.001) {
        format!("{v:.1e}")
    } else {
        format!("{v:.2}")
    }
}

fn tree_diagram(ui: &mut egui::Ui, tree: &Node, names: &[String], route: Option<(&[f64], f64)>) {
    let (response, p) = ui.allocate_painter(
        Vec2::new(ui.available_width().max(300.0), 300.0),
        egui::Sense::hover(),
    );
    fn draw(
        p: &egui::Painter,
        node: &Node,
        names: &[String],
        r: Rect,
        level: usize,
        x: f32,
        width: f32,
        parent: Option<Pos2>,
    ) {
        let pos = Pos2::new(x, r.top() + 25.0 + level as f32 * 75.0);
        if let Some(parent) = parent {
            p.line_segment(
                [parent + Vec2::new(0.0, 20.0), pos - Vec2::new(0.0, 18.0)],
                Stroke::new(1.0, GREY),
            );
        }
        let label = if let Some((j, t, _, _)) = &node.split {
            let name: String = names[*j].chars().take(16).collect();
            format!("{name} ≤ {t:.2}")
        } else {
            format!("Leaf {:.3}", node.value)
        };
        let boxr = Rect::from_center_size(pos, Vec2::new(width.min(150.0).max(70.0), 38.0));
        p.rect_filled(
            boxr,
            4.0,
            if node.split.is_some() {
                Color32::from_rgb(39, 55, 78)
            } else {
                Color32::from_rgb(29, 61, 57)
            },
        );
        p.text(
            pos,
            egui::Align2::CENTER_CENTER,
            label,
            egui::FontId::proportional(12.0),
            instrument::TEXT,
        );
        if let Some((_, _, l, rr)) = &node.split {
            if level < 3 {
                draw(
                    p,
                    l,
                    names,
                    r,
                    level + 1,
                    x - width / 4.,
                    width / 2.,
                    Some(pos),
                );
                draw(
                    p,
                    rr,
                    names,
                    r,
                    level + 1,
                    x + width / 4.,
                    width / 2.,
                    Some(pos),
                );
            } else {
                p.text(
                    pos + Vec2::new(0.0, 24.0),
                    egui::Align2::CENTER_TOP,
                    "… deeper rules below",
                    egui::FontId::proportional(11.0),
                    GREY,
                );
            }
        }
    }
    draw(
        &p,
        tree,
        names,
        response.rect,
        0,
        response.rect.center().x,
        response.rect.width(),
        None,
    );
    if let Some((x, phase)) = route {
        let mut node = tree;
        let mut pos = Pos2::new(response.rect.center().x, response.rect.top() + 25.0);
        let mut width = response.rect.width();
        let mut path = vec![pos];
        for level in 0..3 {
            if let Some((j, t, l, r)) = &node.split {
                let left = x[*j] <= *t;
                pos = Pos2::new(
                    pos.x + if left { -width / 4.0 } else { width / 4.0 },
                    response.rect.top() + 25.0 + (level + 1) as f32 * 75.0,
                );
                width /= 2.0;
                path.push(pos);
                node = if left { l } else { r };
            } else {
                break;
            }
        }
        for pair in path.windows(2) {
            p.line_segment(
                [pair[0], pair[1]],
                Stroke::new(3.0, GREEN.gamma_multiply(0.4)),
            );
        }
        let t = phase.clamp(0.0, 1.0) * (path.len() - 1) as f64;
        let index = (t.floor() as usize).min(path.len() - 1);
        let end = (index + 1).min(path.len() - 1);
        let marker = path[index] + (path[end] - path[index]) * (t - index as f64) as f32;
        p.circle_filled(marker, 7.0, GREEN);
        p.circle_stroke(marker, 10.0, Stroke::new(2.0, GREEN.gamma_multiply(0.4)));
    }
    ui.small("Left branch: condition true (≤). Right branch: false (>). Diagram shows at most four levels; terminal-node rules retain the complete tree.");
}

pub fn choose_path(save: bool) -> Result<Option<PathBuf>, String> {
    #[cfg(target_os="macos")]
    let output=std::process::Command::new("osascript").args(["-e",if save {"POSIX path of (choose file name with prompt \"Export predictions\" default name \"predictions.csv\")"}else{"POSIX path of (choose file with prompt \"Choose a numeric CSV dataset\")"}]).output();
    #[cfg(target_os = "windows")]
    let output = {
        let script = if save {
            "Add-Type -AssemblyName System.Windows.Forms; $d=New-Object System.Windows.Forms.SaveFileDialog; $d.Filter='CSV files (*.csv)|*.csv'; $d.FileName='predictions.csv'; if($d.ShowDialog() -eq 'OK'){[Console]::OutputEncoding=[Text.Encoding]::UTF8;Write-Output $d.FileName}"
        } else {
            "Add-Type -AssemblyName System.Windows.Forms; $d=New-Object System.Windows.Forms.OpenFileDialog; $d.Filter='CSV files (*.csv)|*.csv|All files (*.*)|*.*'; if($d.ShowDialog() -eq 'OK'){[Console]::OutputEncoding=[Text.Encoding]::UTF8;Write-Output $d.FileName}"
        };
        std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-STA", "-Command", script])
            .output()
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let output = if save {
        std::process::Command::new("zenity")
            .args(["--file-selection", "--save", "--confirm-overwrite"])
            .output()
    } else {
        std::process::Command::new("zenity")
            .arg("--file-selection")
            .output()
    };
    let output = output.map_err(|e| {
        format!("File dialog unavailable ({e}). Load by path or drag and drop instead.")
    })?;
    if !output.status.success() {
        return Ok(None);
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok((!path.is_empty()).then(|| PathBuf::from(path)))
}

fn choose_folder() -> Result<Option<PathBuf>, String> {
    #[cfg(target_os="macos")]
    let output=std::process::Command::new("osascript").args(["-e","POSIX path of (choose folder with prompt \"Choose where to export the Python bundle\")"]).output();
    #[cfg(target_os="windows")]
    let output=std::process::Command::new("powershell.exe").args(["-NoProfile","-STA","-Command","Add-Type -AssemblyName System.Windows.Forms; $d=New-Object System.Windows.Forms.FolderBrowserDialog; if($d.ShowDialog() -eq 'OK'){[Console]::OutputEncoding=[Text.Encoding]::UTF8;Write-Output $d.SelectedPath}"]).output();
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let output = std::process::Command::new("zenity")
        .args(["--file-selection", "--directory"])
        .output();
    let out = output.map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Ok(None);
    }
    let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
    Ok((!p.is_empty()).then(|| PathBuf::from(p)))
}
