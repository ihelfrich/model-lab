//! Versioned model adapters and a bounded, cancellable local scientific worker.
//! No shell is involved. Wire/source hashes bind a response to the actual job.
use crate::{
    data::{Data, Table},
    experiment::{self, ModelId, Run, RunConfig},
};
use serde_json::{json, Value};
use std::{
    io::{BufRead, BufReader},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    time::{Duration, Instant},
};

pub const SCHEMA: u64 = 1;
pub const WORKER: &str = include_str!("../python/research_worker.py");
pub use model_lab_engine::provenance::{identity, sha256};
pub const STUDY_WORKER: &str = include_str!("../python/study_worker.py");
pub fn study_manifest() -> Value {
    json!({"study_worker.py":sha256(STUDY_WORKER.as_bytes()),"research_worker.py":sha256(WORKER.as_bytes())})
}
pub fn study_bundle_hash() -> String {
    sha256(&serde_json::to_vec(&study_manifest()).unwrap())
}

#[cfg_attr(not(feature = "acceptance"), allow(dead_code))]
pub trait ModelAdapter {
    fn id(&self) -> &'static str;
    fn manifest(&self) -> Value;
    fn validate(&self, request: &Value) -> Result<(), String>;
}
pub struct NativeAdapter;
pub struct ScientificAdapter;
pub struct StudyAdapter;
impl ModelAdapter for StudyAdapter {
    fn id(&self) -> &'static str {
        "scientific-study"
    }
    fn manifest(&self) -> Value {
        json!({"schema_version":crate::study::PROTOCOL,"backend":self.id(),"engine_version":env!("CARGO_PKG_VERSION"),"worker_bundle_sha256":study_bundle_hash(),"sources":study_manifest(),"operations":["study_preflight","study_preview","simulate","study_inspect","study_export"],"scenarios":["heteroskedasticity","confounding","clustering"],"models":["ols","ridge","forest"],"actual_trace":false,"known_truth_scope":"Declared simulation only"})
    }
    fn validate(&self, request: &Value) -> Result<(), String> {
        crate::study::validate_request(request)
    }
}
impl ModelAdapter for NativeAdapter {
    fn id(&self) -> &'static str {
        "native-rust"
    }
    fn manifest(&self) -> Value {
        json!({"schema_version":SCHEMA,"backend":self.id(),"engine_version":env!("CARGO_PKG_VERSION"),"models":experiment::MODELS.iter().map(|m|json!({"id":m.id.key(),"fit":true,"actual_trace":true,"coefficients":m.coefficients,"inference":false,"penalty_path":m.penalty_path})).collect::<Vec<_>>()})
    }
    fn validate(&self, request: &Value) -> Result<(), String> {
        validate_header(request)?;
        ModelId::parse(request["model"].as_str().ok_or("Missing model")?)?;
        if request["operation"] != "fit" {
            return Err("The native adapter exposes fitting and its actual teaching trace. Select the scientific adapter for inference/resampling.".into());
        }
        Ok(())
    }
}
impl NativeAdapter {
    pub fn fit(
        &self,
        name: String,
        data: Data,
        config: RunConfig,
        cancel: Arc<AtomicBool>,
    ) -> Result<Run, String> {
        let header = json!({"schema_version":SCHEMA,"request_id":identity(),"model":config.model.key(),"operation":"fit"});
        self.validate(&header)?;
        Run::fit(name, data, config, cancel)
    }
}
impl ModelAdapter for ScientificAdapter {
    fn id(&self) -> &'static str {
        "scientific-python"
    }
    fn manifest(&self) -> Value {
        json!({"schema_version":SCHEMA,"backend":self.id(),"engine_version":env!("CARGO_PKG_VERSION"),"worker_sha256":sha256(WORKER.as_bytes()),"operations":["fit","inference","resample","evaluate_test","export","capabilities"],"actual_trace":false,"models":experiment::MODELS.iter().map(|m|json!({"id":m.id.key(),"coefficients":m.coefficients,"inference":matches!(m.id,ModelId::Linear(crate::linear::LinearMethod::Ols|crate::linear::LinearMethod::Logistic))})).collect::<Vec<_>>()})
    }
    fn validate(&self, r: &Value) -> Result<(), String> {
        validate_header(r)?;
        if r["operation"] == "capabilities" {
            return Ok(());
        }
        ModelId::parse(r["model"].as_str().ok_or("Missing model")?)?;
        if ![
            "fit",
            "inference",
            "resample",
            "evaluate_test",
            "export",
            "capabilities",
        ]
        .contains(&r["operation"].as_str().unwrap_or(""))
        {
            return Err("Unsupported scientific operation.".into());
        }
        let rows = r["dataset"]["rows"]
            .as_array()
            .ok_or("Missing dataset rows")?;
        if rows.len() < 4 || rows.len() > 50000 {
            return Err(
                "Scientific datasets require 4–50,000 rows within the request byte limit.".into(),
            );
        }
        let columns = r["dataset"]["columns"]
            .as_array()
            .ok_or("Missing dataset columns")?;
        let mut unique = std::collections::HashSet::new();
        if !(2..=67).contains(&columns.len())
            || columns.iter().any(|c| {
                c.as_str()
                    .is_none_or(|s| s.is_empty() || s.len() > 256 || !unique.insert(s))
            })
            || rows
                .iter()
                .any(|row| row.as_array().is_none_or(|r| r.len() != columns.len()))
        {
            return Err("Invalid rectangular dataset or column names.".into());
        }
        let target = r["roles"]["target"]
            .as_str()
            .ok_or("Missing response role")?;
        let features = r["roles"]["features"]
            .as_array()
            .ok_or("Missing predictor roles")?;
        let mut names = std::collections::HashSet::new();
        if !unique.contains(target)
            || !(1..=64).contains(&features.len())
            || features.iter().any(|c| {
                c.as_str()
                    .is_none_or(|s| s == target || !unique.contains(s) || !names.insert(s))
            })
        {
            return Err("Invalid response or predictor roles.".into());
        }
        Ok(())
    }
}
fn validate_header(r: &Value) -> Result<(), String> {
    if r["schema_version"].as_u64() != Some(SCHEMA)
        || r["request_id"].as_str().is_none_or(str::is_empty)
    {
        return Err("Invalid model-adapter protocol header.".into());
    }
    Ok(())
}
#[cfg_attr(not(feature = "acceptance"), allow(dead_code))]
pub fn catalogue() -> Value {
    json!({"schema_version":SCHEMA,"supported_protocols":[1,2],"adapters":[NativeAdapter.manifest(),ScientificAdapter.manifest(),StudyAdapter.manifest()]})
}

pub fn dataset(data: &Data, name: &str) -> Value {
    let columns = data
        .names
        .iter()
        .chain(std::iter::once(&data.target))
        .cloned()
        .collect::<Vec<_>>();
    let rows = data
        .x
        .iter()
        .zip(&data.y)
        .map(|(x, y)| {
            x.iter()
                .copied()
                .chain(std::iter::once(*y))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    json!({"name":name,"columns":columns,"rows":rows})
}
pub fn table_dataset(table: &Table) -> Value {
    json!({"name":table.name,"columns":table.headers,"rows":table.rows})
}
pub fn request_for_run(run: &Run, operation: &str) -> Value {
    let c = &run.config;
    json!({"schema_version":SCHEMA,"request_id":identity(),"operation":operation,"model":c.model.key(),"seed":c.seed,
        "dataset":dataset(&run.data,&run.name),"roles":{"target":run.data.target,"features":run.data.names},
        "sample_rows":run.partition.train,"config":{"alpha":c.linear.alpha,"mix":c.linear.mix,"standardize":c.linear.standardize,"degree":1,
        "depth":c.tree.depth,"min_leaf":c.tree.min_leaf,"trees":c.tree.trees,"rate":c.tree.rate,"fraction":c.tree.fraction,"mtry":c.tree.mtry},
        "inference":{"covariance":if c.model==ModelId::Linear(crate::linear::LinearMethod::Logistic){"classical"}else{"HC3"},"alpha":0.05,"lags":1},
        "resampling":{"kind":if c.model==ModelId::Linear(crate::linear::LinearMethod::Logistic){"stratified"}else{"random"},"folds":3,"repeats":1,"inner_folds":3,"test_fraction":0.2,"gap":0,"alpha_grid":[0.001,0.01,0.1,1.0]},"max_fits":500})
}

pub enum Event {
    Progress(Value),
    Completed(Value),
    Failed(String),
}
pub struct Session {
    pub receiver: mpsc::Receiver<Event>,
    pub cancel: Arc<AtomicBool>,
    pub finished: Arc<AtomicBool>,
}
impl Drop for Session {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}
pub fn launch(request: Value, interpreter: String) -> Result<Session, String> {
    if interpreter.trim().is_empty() {
        return Err(crate::python::NO_INTERPRETER.into());
    }
    let study = request["schema_version"] == 2;
    if study {
        StudyAdapter.validate(&request)?;
    } else {
        ScientificAdapter.validate(&request)?;
    }
    let bytes = serde_json::to_vec_pretty(&request).map_err(|e| e.to_string())?;
    if bytes.len() > 32 * 1024 * 1024 {
        return Err("Research request exceeds 32 MiB.".into());
    }
    let directory = std::env::temp_dir().join(format!("model-lab-research-{}", identity()));
    std::fs::create_dir(&directory).map_err(|e| e.to_string())?;
    let prepare = (|| {
        std::fs::write(directory.join("research_worker.py"), WORKER).map_err(|e| e.to_string())?;
        if study {
            std::fs::write(directory.join("study_worker.py"), STUDY_WORKER)
                .map_err(|e| e.to_string())?;
        }
        std::fs::write(directory.join("request.json"), &bytes).map_err(|e| e.to_string())
    })();
    if let Err(e) = prepare {
        let _ = std::fs::remove_dir_all(&directory);
        return Err(e);
    }
    let (tx, rx) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let stop = cancel.clone();
    let finished = Arc::new(AtomicBool::new(false));
    let finished_signal = finished.clone();
    std::thread::spawn(move || {
        let mut command = Command::new(interpreter.trim());
        command
            .args([
                "-I",
                "-B",
                "-u",
                if study {
                    "study_worker.py"
                } else {
                    "research_worker.py"
                },
                "--request",
                "request.json",
                "--response",
                "response.json",
            ])
            .current_dir(&directory)
            .env_clear();
        for key in ["PATH", "SystemRoot", "WINDIR", "TEMP", "TMP", "TMPDIR"] {
            if let Some(v) = std::env::var_os(key) {
                command.env(key, v);
            }
        }
        command
            .env("MPLBACKEND", "Agg")
            .env("OMP_NUM_THREADS", "1")
            .env("OPENBLAS_NUM_THREADS", "1");
        command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::null());
        let result = (|| -> Result<Value, String> {
            if stop.load(Ordering::Relaxed) {
                return Err("Scientific job canceled before startup.".into());
            }
            let mut child = command
                .spawn()
                .map_err(|e| format!("Could not start scientific Python: {e}"))?;
            let stdout = child.stdout.take().unwrap();
            let stderr = child.stderr.take().unwrap();
            let updates = tx.clone();
            let progress_request = request.clone();
            let out = std::thread::spawn(move || {
                for line in BufReader::new(stdout)
                    .lines()
                    .map_while(Result::ok)
                    .take(20000)
                {
                    if line.len() > 65536 {
                        continue;
                    }
                    if let Ok(mut v) = serde_json::from_str::<Value>(&line) {
                        if study {
                            if v["schema_version"] != 2
                                || v["request_id"] != progress_request["request_id"]
                            {
                                continue;
                            }
                            if v["kind"] == "checkpoint" {
                                if let Some(path) = progress_request["checkpoint_path"].as_str() {
                                    if let Ok(document) =
                                        crate::study::read_checkpoint(std::path::Path::new(path))
                                    {
                                        v["checkpoint_document"] = document;
                                    }
                                }
                            }
                        }
                        let _ = updates.send(Event::Progress(v));
                    }
                }
            });
            let err = std::thread::spawn(move || {
                let mut text = String::new();
                for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                    if text.len() < 16000 {
                        text.push_str(&line.chars().take(1000).collect::<String>());
                        text.push('\n');
                    }
                }
                text
            });
            let start = Instant::now();
            let status = loop {
                if stop.load(Ordering::Relaxed) || start.elapsed() > Duration::from_secs(330) {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = out.join();
                    let _ = err.join();
                    return Err(if stop.load(Ordering::Relaxed) {
                        "Scientific job canceled."
                    } else {
                        "Scientific job exceeded its execution limit."
                    }
                    .into());
                }
                if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
                    break status;
                }
                std::thread::sleep(Duration::from_millis(20));
            };
            let _ = out.join();
            let diagnostic = err.join().unwrap_or_default();
            let response = directory.join("response.json");
            if !response.is_file() {
                return Err(format!(
                    "Scientific worker exited {status} without a response. {diagnostic}"
                ));
            }
            if std::fs::metadata(&response)
                .map_err(|e| e.to_string())?
                .len()
                > 128 * 1024 * 1024
            {
                return Err("Scientific result exceeds 128 MiB.".into());
            }
            let value: Value =
                serde_json::from_slice(&std::fs::read(response).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?;
            validate_response(&request, &bytes, &value)?;
            if !status.success() {
                return Err(format!("Worker status {status}: {diagnostic}"));
            }
            Ok(value)
        })();
        let _ = std::fs::remove_dir_all(directory);
        finished_signal.store(true, Ordering::Release);
        let _ = tx.send(match result {
            Ok(v) => Event::Completed(v),
            Err(e) => Event::Failed(e),
        });
    });
    Ok(Session {
        receiver: rx,
        cancel,
        finished,
    })
}
pub fn validate_response(request: &Value, wire: &[u8], response: &Value) -> Result<(), String> {
    if request["schema_version"] == 2 {
        if response["schema_version"] != 2
            || response["request_id"] != request["request_id"]
            || response["operation"] != request["operation"]
            || response["request_sha256"].as_str() != Some(&sha256(wire))
            || response["worker_sha256"].as_str() != Some(&sha256(STUDY_WORKER.as_bytes()))
            || response["worker_bundle_sha256"].as_str() != Some(&study_bundle_hash())
        {
            return Err("Study response identity, source bundle or wire hash mismatch.".into());
        }
        if response["status"] != "completed" || response.get("result").is_none() {
            return Err(response["error"]
                .as_str()
                .unwrap_or("Study operation did not complete")
                .into());
        }
        return Ok(());
    }
    if response["schema_version"].as_u64() != Some(SCHEMA)
        || response["request_id"] != request["request_id"]
        || response["request_sha256"].as_str() != Some(&sha256(wire))
        || response["worker_sha256"].as_str() != Some(&sha256(WORKER.as_bytes()))
    {
        return Err("Scientific response identity, protocol or source hash did not match the submitted job.".into());
    }
    if response["status"] == "failed" {
        return Err(response["error"]
            .as_str()
            .unwrap_or("Scientific operation failed.")
            .into());
    }
    if response["status"] != "completed"
        || response["operation"] != request["operation"]
        || response.get("result").is_none()
    {
        return Err("Incomplete or incompatible scientific result.".into());
    }
    validate_payload(request, response)?;
    Ok(())
}

pub fn validate_payload(request: &Value, response: &Value) -> Result<(), String> {
    let operation = request["operation"].as_str().unwrap_or("");
    if operation == "export" || operation == "capabilities" {
        return Ok(());
    }
    let n = request["dataset"]["rows"]
        .as_array()
        .ok_or("Missing request rows")?
        .len();
    let result = &response["result"];
    let rows = |v: &Value| -> Result<Vec<usize>, String> {
        let a = v.as_array().ok_or("Missing sample identity array")?;
        let mut seen = std::collections::HashSet::new();
        a.iter()
            .map(|v| {
                let i = v.as_u64().ok_or("Non-integer sample identity")? as usize;
                if i >= n || !seen.insert(i) {
                    return Err("Duplicate/out-of-range sample identity".into());
                }
                Ok(i)
            })
            .collect()
    };
    let finite = |v: &Value| v.as_f64().is_some_and(f64::is_finite);
    if operation == "fit" || operation == "inference" {
        let train = rows(&result["training_rows"])?;
        let prediction = rows(&result["prediction_rows"])?;
        let values = result["predictions"]
            .as_array()
            .ok_or("Missing predictions")?;
        if train.len() < 3
            || values.len() != prediction.len()
            || values.iter().any(|v| !finite(v))
            || !finite(&result["training_loss"])
        {
            return Err("Malformed scientific prediction result.".into());
        }
        if operation == "inference" {
            let table = result["coefficient_table"]
                .as_array()
                .ok_or("Missing coefficient inference table")?;
            if table.len() < 2
                || table.len() > 513
                || table.iter().any(|row| {
                    row["term"].as_str().is_none()
                        || [
                            "estimate",
                            "std_error",
                            "statistic",
                            "p_value",
                            "lower",
                            "upper",
                        ]
                        .iter()
                        .any(|key| !finite(&row[key]))
                })
            {
                return Err("Malformed coefficient inference table.".into());
            }
            if result["n"].as_u64() != Some(train.len() as u64)
                || result["parameter_count"].as_u64() != Some(table.len() as u64)
            {
                return Err("Inference sample/parameter dimensions disagree.".into());
            }
        }
    } else if operation == "resample" || operation == "evaluate_test" {
        let development = rows(&result["development_rows"])?;
        let test = rows(&result["test_rows"])?;
        let set = development
            .iter()
            .copied()
            .collect::<std::collections::HashSet<_>>();
        let folds = result["folds"].as_array().ok_or("Missing fold records")?;
        if development.len() < 3
            || test.is_empty()
            || test.iter().any(|i| set.contains(i))
            || folds.is_empty()
            || folds.len() > 50
            || !finite(&result["cv_loss"])
        {
            return Err("Malformed resampling partitions or score.".into());
        }
        for f in folds {
            let a = rows(&f["training_rows"])?;
            let b = rows(&f["validation_rows"])?;
            let a_set = a.iter().copied().collect::<std::collections::HashSet<_>>();
            if a.len() < 3
                || b.is_empty()
                || a.iter().chain(&b).any(|i| !set.contains(i))
                || b.iter().any(|i| a_set.contains(i))
                || !finite(&f["validation_loss"])
            {
                return Err("Invalid outer-fold boundary or score.".into());
            }
        }
        if operation == "resample"
            && (!result["test_loss"].is_null()
                || !result["test_predictions"].is_null()
                || result["test_revealed"] != false)
        {
            return Err("An unscored test operation unexpectedly disclosed test results.".into());
        }
        if operation == "evaluate_test"
            && (!finite(&result["test_loss"]) || result["test_revealed"] != true)
        {
            return Err("Missing explicit test evaluation result.".into());
        }
    } else {
        return Err("Unsupported saved scientific operation.".into());
    }
    Ok(())
}
pub fn default_interpreter() -> String {
    crate::python::default_interpreter()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn protocol_hash_known_vector_and_stale_reply_rejection() {
        assert_eq!(
            sha256(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let r = json!({"request_id":"a","operation":"fit"});
        assert!(validate_response(
            &r,
            b"request",
            &json!({"schema_version":SCHEMA,"request_id":"b"})
        )
        .is_err());
    }
    #[test]
    fn catalogue_declares_actual_trace_boundary() {
        let c = catalogue();
        assert_eq!(c["adapters"][0]["models"].as_array().unwrap().len(), 9);
        assert_eq!(c["adapters"][1]["actual_trace"], false);
    }
    #[test]
    fn actual_scientific_worker_inference_and_project_integrity() {
        let run = Run::fit(
            "Protocol fixture".into(),
            experiment::scenario(0, 4710, 0.8),
            RunConfig::default(),
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
        let request = request_for_run(&run, "inference");
        let session = launch(request.clone(), default_interpreter()).unwrap();
        let response = loop {
            match session
                .receiver
                .recv_timeout(Duration::from_secs(45))
                .expect("scientific worker timeout")
            {
                Event::Progress(_) => {}
                Event::Failed(e) => panic!("{e}"),
                Event::Completed(value) => break value,
            }
        };
        assert_eq!(
            response["result"]["n"].as_u64(),
            Some(run.partition.train.len() as u64)
        );
        let native = run.coefficients(run.last_stage()).unwrap();
        let table = response["result"]["coefficient_table"].as_array().unwrap();
        for (j, b) in std::iter::once(native.0).chain(native.1).enumerate() {
            assert!((table[j]["estimate"].as_f64().unwrap() - b).abs() < 1e-8);
        }
        let mut project = crate::research_ui::Research::default();
        project.template = request.clone();
        project.history.push(std::sync::Arc::new(json!({"request":request,"result_sha256":sha256(&serde_json::to_vec(&response["result"]).unwrap()),"response":response})));
        let wire = serde_json::to_string_pretty(&project.to_json()).unwrap();
        let mut restored = crate::research_ui::Research::default();
        restored
            .restore(serde_json::from_str(&wire).unwrap())
            .unwrap();
        assert_eq!(restored.history.len(), 1);
        let mut corrupted: Value = serde_json::from_str(&wire).unwrap();
        corrupted["history"][0]["response"]["result"]["coefficient_table"][0]["estimate"] =
            json!(999.);
        assert!(restored.restore(corrupted).is_err());
    }
    #[test]
    fn actual_worker_cancellation_stops_before_acceptance() {
        let request =
            json!({"schema_version":SCHEMA,"request_id":identity(),"operation":"capabilities"});
        let session = launch(request, default_interpreter()).unwrap();
        session.cancel.store(true, Ordering::Relaxed);
        match session
            .receiver
            .recv_timeout(Duration::from_secs(10))
            .unwrap()
        {
            Event::Failed(message) => assert!(message.contains("canceled")),
            _ => panic!("Canceled operation accepted a result"),
        }
    }
}
