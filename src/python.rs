use crate::{
    data::Data,
    model::{Config, Method},
};
use serde_json::Value;
use std::{
    io::{BufRead, BufReader, Write},
    path::PathBuf,
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc,
    time::{SystemTime, UNIX_EPOCH},
};
static RUN_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
fn unique_id() -> String {
    format!(
        "{}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        RUN_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    )
}
/// The interpreter this build may use without being told: an explicit
/// `MODEL_LAB_PYTHON`/`TREE_LAB_PYTHON` override, the runtime packaged beside
/// the executable, or, in debug builds only, the project's own `.venv`.
/// A release build with no bundled runtime returns an empty string, and
/// launching reports that a runtime must be chosen. It never scans caches,
/// Homebrew or `PATH`, so a shipped binary cannot silently depend on the host.
pub fn default_interpreter() -> String {
    for key in ["MODEL_LAB_PYTHON", "TREE_LAB_PYTHON"] {
        if let Some(p) = std::env::var_os(key) {
            return p.to_string_lossy().into_owned();
        }
    }
    if let Some(bundled) = bundled_interpreter() {
        return bundled;
    }
    #[cfg(debug_assertions)]
    {
        let local = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(if cfg!(windows) {
            ".venv/Scripts/python.exe"
        } else {
            ".venv/bin/python"
        });
        if local.is_file() {
            return local.to_string_lossy().into_owned();
        }
    }
    String::new()
}
/// The Python runtime packaged with this executable, if present.
pub fn bundled_interpreter() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    let parent = exe.parent()?;
    let candidates = if cfg!(target_os = "windows") {
        vec![parent.join("Python/python.exe")]
    } else {
        vec![parent.join("../Resources/Python/bin/python3")]
    };
    candidates
        .into_iter()
        .find(|p| p.is_file())
        .map(|p| p.to_string_lossy().into_owned())
}
pub const NO_INTERPRETER: &str = "No Python runtime is packaged with this build and none was chosen. Set MODEL_LAB_PYTHON or select an interpreter in the application.";

pub fn generated(cfg: &Config) -> String {
    let count = if cfg.method == Method::Tree {
        1
    } else {
        cfg.trees
    };
    let mut s=format!("# {}: editable, standard-library Python\n# These are transparent teaching tools, not scikit-learn wrappers.\nimport random\nfrom lab_runtime import load_data, split_rows, mean, fit_tree, predict, report\n\nlearning_rate = {}\nn_estimators = {}\nmax_depth = {}\nmin_leaf = {}\nsample_fraction = {}\nmax_features = {}\nvalidation_fraction = {}\nseed = {}\n\nX, y, feature_names = load_data(\"selected_data.csv\")\ntrain_rows, validation_rows = split_rows(len(y), validation_fraction, seed)\nrng = random.Random(seed + 1)\ninitial_mean = mean([y[i] for i in train_rows])\npredictions = [initial_mean for _ in y]\ntrees = []\nreport(0, y, predictions, train_rows, validation_rows, [])\n\nfor round_no in range(1, n_estimators + 1):\n",cfg.method.label(),cfg.rate,count,cfg.depth,cfg.min_leaf,cfg.fraction,cfg.mtry,cfg.validation,cfg.seed);
    match cfg.method {
        Method::Boosting|Method::Stochastic=>{
            s.push_str("    # Squared-error negative gradient: residual = actual - prediction.\n    residuals = [actual - fitted for actual, fitted in zip(y, predictions)]\n");
            if cfg.method==Method::Stochastic {
                s.push_str("    # A fresh subset WITHOUT replacement for this round.\n    sample_size = max(1, int(sample_fraction * len(train_rows)))\n    sampled_rows = rng.sample(train_rows, sample_size)\n");
            }else{s.push_str("    sampled_rows = list(train_rows)\n");}
            s.push_str("    tree = fit_tree(X, residuals, sampled_rows, max_depth, min_leaf, rng)\n    trees.append(tree)\n    # F_new(x) = F_old(x) + learning_rate * h(x), for EVERY row.\n    predictions = [old + learning_rate * predict(tree, x)\n                   for old, x in zip(predictions, X)]\n");
        },
        Method::Forest=>s.push_str("    # Bootstrap WITH replacement; original y, never ensemble residuals.\n    sampled_rows = rng.choices(train_rows, k=len(train_rows))\n    # Candidate-feature subset is redrawn at EVERY split inside fit_tree.\n    tree = fit_tree(X, y, sampled_rows, max_depth, min_leaf, rng, max_features)\n    trees.append(tree)\n    # Forest prediction = average of the trees fitted so far.\n    predictions = [mean([predict(t, x) for t in trees]) for x in X]\n"),
        Method::Tree=>s.push_str("    sampled_rows = list(train_rows)\n    tree = fit_tree(X, y, sampled_rows, max_depth, min_leaf, rng)\n    trees.append(tree)\n    predictions = [predict(tree, x) for x in X]\n"),
    }
    s.push_str("    report(round_no, y, predictions, train_rows, validation_rows, sampled_rows)\n\nprint(\"Finished: inspect the prediction and error plots in the app.\")\n");
    with_stopping(s, cfg)
}

fn with_stopping(mut code: String, cfg: &Config) -> String {
    if !cfg.method.boosting() || cfg.patience == 0 {
        return code;
    }
    code=code.replace("report(0, y, predictions, train_rows, validation_rows, [])",&format!("best_validation = report(0, y, predictions, train_rows, validation_rows, [])['validation_mse']\npatience = {}\nmin_relative_improvement = {}\nstale_rounds = 0",cfg.patience,cfg.min_relative_improvement));
    for sample in ["sampled_rows", "[]"] {
        let old =
            format!("    report(round_no, y, predictions, train_rows, validation_rows, {sample})");
        let new=format!("    result = report(round_no, y, predictions, train_rows, validation_rows, {sample})\n    loss = result['validation_mse']\n    if loss < best_validation - min_relative_improvement * max(abs(best_validation), 1e-12):\n        best_validation, stale_rounds = loss, 0\n    else:\n        stale_rounds += 1\n    if stale_rounds >= patience:\n        print(f'Early stopping after {{round_no}} rounds; retained path includes patience rounds.')\n        break");
        code = code.replace(&old, &new);
    }
    code
}

pub fn sklearn_code(cfg: &Config) -> String {
    let (class,params)=match cfg.method {
        Method::Tree=>("DecisionTreeRegressor",format!("max_depth={}, min_samples_leaf={}, random_state={}",cfg.depth,cfg.min_leaf,cfg.seed)),
        Method::Forest=>("RandomForestRegressor",format!("n_estimators=1, max_depth={}, min_samples_leaf={},\n    max_features={}, bootstrap=True, warm_start=True, n_jobs=1, random_state={}",cfg.depth,cfg.min_leaf,cfg.mtry,cfg.seed)),
        _=>("GradientBoostingRegressor",format!("n_estimators=1, learning_rate={}, max_depth={}, min_samples_leaf={},\n    subsample={}, loss='squared_error', warm_start=True, random_state={}",cfg.rate,cfg.depth,cfg.min_leaf,if cfg.method==Method::Stochastic{cfg.fraction}else{1.0},cfg.seed)),
    };
    let count = if cfg.method == Method::Tree {
        1
    } else {
        cfg.trees
    };
    let mut s=format!("# Class-library version: requires scikit-learn in the selected interpreter.\n# Package RNGs/split conventions differ from the native teaching engine.\nfrom sklearn.tree import DecisionTreeRegressor\nfrom sklearn.ensemble import GradientBoostingRegressor, RandomForestRegressor\nfrom sklearn.model_selection import train_test_split\nfrom lab_runtime import load_data, mean, report\n\nX, y, feature_names = load_data('selected_data.csv')\ntrain_rows, validation_rows = train_test_split(\n    list(range(len(y))), test_size={}, random_state={})\nX_train = [X[i] for i in train_rows]\ny_train = [y[i] for i in train_rows]\nmodel = {class}(\n    {params})\npredictions = [mean(y_train) for _ in y]\nreport(0, y, predictions, train_rows, validation_rows, [])\n\nfor round_no in range(1, {} + 1):\n",cfg.validation,cfg.seed,count);
    if cfg.method != Method::Tree {
        s.push_str("    # warm_start retains fitted trees and adds the requested next tree.\n    model.set_params(n_estimators=round_no)\n");
    }
    s.push_str("    model.fit(X_train, y_train)\n    predictions = [float(value) for value in model.predict(X)]\n    # This view does not expose package-internal sampled row IDs.\n    report(round_no, y, predictions, train_rows, validation_rows, [])\n\nprint('scikit-learn fit completed.')\n");
    with_stopping(s, cfg)
}

pub fn export_bundle(parent: &std::path::Path, code: &str, data: &Data) -> Result<PathBuf, String> {
    let stamp = unique_id();
    let dir = parent.join(format!("Tree-Lab-Python-{stamp}"));
    std::fs::create_dir(&dir).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("student.py"), code).map_err(|e| e.to_string())?;
    std::fs::write(
        dir.join("lab_runtime.py"),
        include_str!("../python/lab_runtime.py"),
    )
    .map_err(|e| e.to_string())?;
    std::fs::write(dir.join("runner.py"), include_str!("../python/runner.py"))
        .map_err(|e| e.to_string())?;
    let mut csv = data
        .names
        .iter()
        .chain(std::iter::once(&data.target))
        .map(|s| format!("\"{}\"", s.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(",");
    csv.push('\n');
    for (x, y) in data.x.iter().zip(&data.y) {
        csv.push_str(
            &x.iter()
                .map(|v| v.to_string())
                .collect::<Vec<_>>()
                .join(","),
        );
        csv.push_str(&format!(",{y}\n"));
    }
    std::fs::write(dir.join("selected_data.csv"), csv).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("README.txt"),"Run from this folder: python student.py\nOptional line trace: python runner.py student.py\nThe transparent version needs only Python 3 standard library. The class-library version requires scikit-learn.\nOriginal predictor column names are retained in their selected order; the response column is last.\nThe transparent implementation is not a Rust or scikit-learn parity claim.\n").map_err(|e|e.to_string())?;
    Ok(dir)
}

pub enum Event {
    Protocol(Value),
    Output(String),
}
pub struct Session {
    pub receiver: mpsc::Receiver<Event>,
    pub child: Child,
    pub stdin: ChildStdin,
    pub code: String,
    pub data: Data,
    pub dir: PathBuf,
    pub step: bool,
    pub label: String,
}
impl Session {
    pub fn next(&mut self) -> Result<(), String> {
        self.stdin
            .write_all(b"\n")
            .and_then(|_| self.stdin.flush())
            .map_err(|e| e.to_string())
    }
    pub fn stop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        self.stop();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

pub fn start(
    interpreter: &str,
    code: String,
    data: Data,
    step: bool,
    label: String,
) -> Result<Session, String> {
    let stamp = unique_id();
    let dir = std::env::temp_dir().join(format!("tree-lab-python-{stamp}"));
    std::fs::create_dir(&dir).map_err(|e| e.to_string())?;
    let prepare = (|| -> Result<(), String> {
        std::fs::write(
            dir.join("lab_runtime.py"),
            include_str!("../python/lab_runtime.py"),
        )
        .map_err(|e| e.to_string())?;
        std::fs::write(dir.join("runner.py"), include_str!("../python/runner.py"))
            .map_err(|e| e.to_string())?;
        std::fs::write(dir.join("student.py"), &code).map_err(|e| e.to_string())?;
        let mut csv = data
            .names
            .iter()
            .chain(std::iter::once(&data.target))
            .map(|s| format!("\"{}\"", s.replace('"', "\"\"")))
            .collect::<Vec<_>>()
            .join(",");
        csv.push('\n');
        for (row, y) in data.x.iter().zip(&data.y) {
            csv.push_str(
                &row.iter()
                    .map(|v| v.to_string())
                    .collect::<Vec<_>>()
                    .join(","),
            );
            csv.push_str(&format!(",{y}\n"));
        }
        std::fs::write(dir.join("selected_data.csv"), csv).map_err(|e| e.to_string())?;
        Ok(())
    })();
    if let Err(e) = prepare {
        let _ = std::fs::remove_dir_all(&dir);
        return Err(e);
    }
    if interpreter.trim().is_empty() {
        let _ = std::fs::remove_dir_all(&dir);
        return Err(NO_INTERPRETER.into());
    }
    // No shell interpolation. Do not pass API tokens or the full inherited environment.
    let mut command = Command::new(interpreter.trim());
    command
        .args(["-I", "-B", "-u", "runner.py", "student.py"])
        .current_dir(&dir)
        .env_clear();
    for key in ["PATH", "SystemRoot", "WINDIR", "TEMP", "TMP", "TMPDIR"] {
        if let Some(v) = std::env::var_os(key) {
            command.env(key, v);
        }
    }
    command
        .env("LAB_MODE", if step { "step" } else { "play" })
        .env("LAB_DELAY", "0");
    let mut child = match command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&dir);
            return Err(format!(
                "Could not start Python ({e}). Set the path to a Python 3 interpreter."
            ));
        }
    };
    let stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let (tx, rx) = mpsc::sync_channel(256);
    let out = tx.clone();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            let event = if let Some(s) = line.strip_prefix("@LAB@") {
                serde_json::from_str(s)
                    .map(Event::Protocol)
                    .unwrap_or_else(|_| Event::Output(line))
            } else {
                Event::Output(line)
            };
            if out.send(event).is_err() {
                break;
            }
        }
    });
    std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            if tx.send(Event::Output(line)).is_err() {
                break;
            }
        }
    });
    Ok(Session {
        receiver: rx,
        child,
        stdin,
        code,
        data,
        dir,
        step,
        label,
    })
}

pub struct Workspace {
    pub interpreter: String,
    pub draft: String,
    pub session: Option<Session>,
    pub running: bool,
    pub line: usize,
    pub locals: String,
    pub output: Vec<String>,
    pub stage: usize,
    pub train: Vec<(f64, f64)>,
    pub valid: Vec<(f64, f64)>,
    pub predictions: Vec<f64>,
    pub source_mode: usize,
    pub step: bool,
    pub message: String,
    pub waiting: bool,
    pub library: bool,
    pub metric_label: String,
}
impl Default for Workspace {
    fn default() -> Self {
        Self {
            interpreter: default_interpreter(),
            draft: String::new(),
            session: None,
            running: false,
            line: 0,
            locals: String::new(),
            output: vec![],
            stage: 0,
            train: vec![],
            valid: vec![],
            predictions: vec![],
            source_mode: 0,
            step: false,
            message: String::new(),
            waiting: false,
            library: false,
            metric_label: "MSE".into(),
        }
    }
}
impl Workspace {
    pub fn launch(&mut self, code: String, data: Data, label: String) {
        if self.running {
            return;
        }
        self.session = None;
        self.output.clear();
        self.train.clear();
        self.valid.clear();
        self.predictions.clear();
        self.line = 0;
        self.stage = 0;
        self.waiting = false;
        match start(&self.interpreter, code, data, self.step, label) {
            Ok(s) => {
                self.session = Some(s);
                self.running = true;
                self.source_mode = 2;
                self.message = "Python running locally".into();
            }
            Err(e) => self.message = e,
        }
    }
    pub fn poll(&mut self) {
        let Some(session) = self.session.as_mut() else {
            return;
        };
        for event in session.receiver.try_iter().take(4000) {
            match event {
                Event::Output(line) => {
                    self.output.push(line.chars().take(2000).collect());
                    if self.output.len() > 400 {
                        self.output.remove(0);
                    }
                }
                Event::Protocol(v) => match v["kind"].as_str() {
                    Some("line") => {
                        self.line = v["line"].as_u64().unwrap_or(0) as usize;
                        self.locals = v["locals"].to_string();
                    }
                    Some("stage") => {
                        self.stage = v["stage"].as_u64().unwrap_or(0) as usize;
                        if let Some(label) = v["metric_name"].as_str() {
                            self.metric_label = label.into();
                        } else {
                            self.metric_label = "MSE".into();
                        }
                        if let (Some(a), Some(b)) = (
                            v["train_loss"].as_f64().or_else(|| v["train_mse"].as_f64()),
                            v["validation_loss"]
                                .as_f64()
                                .or_else(|| v["validation_mse"].as_f64()),
                        ) {
                            self.train.push((self.stage as f64, a));
                            self.valid.push((self.stage as f64, b));
                        }
                        if let Some(p) = v["predictions"].as_array() {
                            let values: Vec<_> = p.iter().filter_map(Value::as_f64).collect();
                            if values.len() == session.data.y.len() {
                                self.predictions = values;
                            }
                        }
                        self.waiting = session.step;
                        self.message = if session.step {
                            format!(
                                "Paused after Python round {} — click Next Python round",
                                self.stage
                            )
                        } else {
                            format!("Python round {}", self.stage)
                        };
                    }
                    Some("done") => {
                        self.running = false;
                        self.waiting = false;
                        self.message = "Python completed".into();
                    }
                    _ => {}
                },
            }
        }
        if self.running {
            if let Ok(Some(status)) = session.child.try_wait() {
                self.running = false;
                self.waiting = false;
                self.message = format!("Python exited: {status}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn code_changes_with_controls_and_sampling_method() {
        let mut c = Config::default();
        let a = generated(&c);
        c.rate = 0.03;
        c.depth = 4;
        let b = generated(&c);
        assert_ne!(a, b);
        assert!(b.contains("learning_rate = 0.03"));
        assert!(b.contains("max_depth = 4"));
        c.method = Method::Forest;
        assert!(generated(&c).contains("rng.choices"));
        assert!(!generated(&c).contains("residuals ="));
        c.method = Method::Stochastic;
        assert!(generated(&c).contains("rng.sample"));
    }
    #[test]
    fn all_generated_programs_execute_and_report_actual_results() {
        if !cfg!(target_os = "macos") {
            return;
        }
        let table = crate::data::Table::parse("fixture", include_str!("../sample.csv")).unwrap();
        for method in Method::ALL {
            let cfg = Config {
                method,
                trees: 2,
                min_leaf: 1,
                ..Default::default()
            };
            let mut session = start(
                "/opt/homebrew/bin/python3",
                generated(&cfg),
                table.dataset(2, &[0, 1]).unwrap(),
                false,
                "test".into(),
            )
            .unwrap();
            let mut stages = 0;
            let mut traced = false;
            let mut done = false;
            for _ in 0..1000 {
                match session
                    .receiver
                    .recv_timeout(std::time::Duration::from_secs(5))
                    .unwrap()
                {
                    Event::Protocol(v) => match v["kind"].as_str() {
                        Some("stage") => {
                            stages += 1;
                            assert!(v["validation_mse"].as_f64().unwrap().is_finite());
                            assert_eq!(v["predictions"].as_array().unwrap().len(), 12);
                        }
                        Some("line") => traced = true,
                        Some("done") => {
                            done = true;
                            break;
                        }
                        _ => {}
                    },
                    Event::Output(s) => assert!(!s.contains("Error"), "{s}"),
                }
            }
            assert!(traced && done);
            assert_eq!(stages, if method == Method::Tree { 2 } else { 3 });
            session.stop();
        }
    }
    #[test]
    fn round_step_mode_really_waits_and_resumes() {
        if !cfg!(target_os = "macos") {
            return;
        }
        let t = crate::data::Table::parse("fixture", include_str!("../sample.csv")).unwrap();
        let cfg = Config {
            method: Method::Tree,
            ..Default::default()
        };
        let mut s = start(
            "/opt/homebrew/bin/python3",
            generated(&cfg),
            t.dataset(2, &[0]).unwrap(),
            true,
            "step test".into(),
        )
        .unwrap();
        let mut stages = vec![];
        for expected in 0..=1 {
            loop {
                match s
                    .receiver
                    .recv_timeout(std::time::Duration::from_secs(3))
                    .unwrap()
                {
                    Event::Protocol(v) if v["kind"] == "stage" => {
                        assert_eq!(v["stage"].as_u64().unwrap(), expected);
                        stages.push(expected);
                        break;
                    }
                    _ => {}
                }
            }
            assert!(s
                .receiver
                .recv_timeout(std::time::Duration::from_millis(40))
                .is_err());
            s.next().unwrap();
        }
        let mut done = false;
        while let Ok(event) = s.receiver.recv_timeout(std::time::Duration::from_secs(3)) {
            if let Event::Protocol(v) = event {
                if v["kind"] == "done" {
                    done = true;
                    break;
                }
            }
        }
        assert!(done);
        assert_eq!(stages, vec![0, 1]);
        s.stop();
    }
    #[test]
    fn class_library_programs_execute_in_existing_scientific_environment() {
        let python = default_interpreter();
        if !cfg!(target_os = "macos") || !python.contains("archive-v0") {
            return;
        }
        let t = crate::data::Table::parse("fixture", include_str!("../sample.csv")).unwrap();
        for method in Method::ALL {
            let cfg = Config {
                method,
                trees: 2,
                min_leaf: 1,
                ..Default::default()
            };
            let mut s = start(
                &python,
                sklearn_code(&cfg),
                t.dataset(2, &[0, 1]).unwrap(),
                false,
                "package test".into(),
            )
            .unwrap();
            let mut done = false;
            let mut output = String::new();
            let mut count = 0;
            while let Ok(event) = s.receiver.recv_timeout(std::time::Duration::from_secs(15)) {
                match event {
                    Event::Protocol(v) if v["kind"] == "done" => {
                        done = true;
                        break;
                    }
                    Event::Protocol(v) if v["kind"] == "stage" => {
                        count += 1;
                        assert!(v["validation_mse"].as_f64().unwrap().is_finite());
                    }
                    Event::Output(line) => output.push_str(&line),
                    _ => {}
                }
            }
            assert!(done, "{output}");
            assert_eq!(count, if method == Method::Tree { 2 } else { 3 });
            s.stop();
        }
    }
    #[test]
    fn generated_early_stopping_really_stops_in_both_python_backends() {
        if !cfg!(target_os = "macos") {
            return;
        }
        let interpreter = default_interpreter();
        let t = crate::data::Table::parse("fixture", include_str!("../sample.csv")).unwrap();
        let mut d = t.dataset(2, &[0]).unwrap();
        d.y.fill(7.0);
        let cfg = Config {
            trees: 20,
            patience: 2,
            min_leaf: 1,
            ..Default::default()
        };
        for code in [generated(&cfg), sklearn_code(&cfg)] {
            let mut s = start(
                &interpreter,
                code,
                d.clone(),
                false,
                "early stopping test".into(),
            )
            .unwrap();
            let mut stages = vec![];
            let mut done = false;
            let mut errors = String::new();
            while let Ok(event) = s.receiver.recv_timeout(std::time::Duration::from_secs(15)) {
                match event {
                    Event::Protocol(v) if v["kind"] == "stage" => {
                        stages.push(v["stage"].as_u64().unwrap())
                    }
                    Event::Protocol(v) if v["kind"] == "done" => {
                        done = true;
                        break;
                    }
                    Event::Output(text) => errors.push_str(&text),
                    _ => {}
                }
            }
            assert!(done, "{errors}");
            assert_eq!(stages, vec![0, 1, 2]);
            s.stop();
        }
    }
}
