//! End-to-end research checks through the same process boundary used by the UI.
use crate::{
    acceptance,
    adapter::{self, Event},
    experiment::{self, Run, RunConfig},
};
use serde_json::{json, Value};
use std::{
    path::Path,
    process::Command,
    sync::{atomic::AtomicBool, Arc},
    time::{Duration, Instant},
};

pub fn execute(request: Value) -> Result<Value, String> {
    let session = adapter::launch(request, adapter::default_interpreter())?;
    loop {
        match session
            .receiver
            .recv_timeout(Duration::from_secs(350))
            .map_err(|e| format!("Scientific worker did not finish: {e}"))?
        {
            Event::Progress(_) => {}
            Event::Completed(value) => return Ok(value),
            Event::Failed(error) => return Err(error),
        }
    }
}
pub fn command() -> Result<(), String> {
    let path = acceptance::argument("--research-request").ok_or("Missing request path")?;
    if std::fs::metadata(&path).map_err(|e| e.to_string())?.len() > 32 * 1024 * 1024 {
        return Err("Research request exceeds 32 MiB.".into());
    }
    let request = serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let result = execute(request)?;
    if let Some(path) = acceptance::argument("--report") {
        acceptance::write(&path, &result)?;
    } else {
        println!("{}", serde_json::to_string_pretty(&result).unwrap());
    }
    Ok(())
}
pub fn self_test() -> Result<(), String> {
    let run = Run::fit(
        "Packaged research acceptance".into(),
        experiment::scenario(0, 4710, 0.8),
        RunConfig::default(),
        Arc::new(AtomicBool::new(false)),
    )?;
    let catalog = execute(
        json!({"schema_version":adapter::SCHEMA,"request_id":adapter::identity(),"operation":"capabilities"}),
    )?;
    if catalog["result"]["models"].as_array().map(Vec::len) != Some(9) {
        return Err("Incomplete scientific capability catalogue.".into());
    }
    let inference_request = adapter::request_for_run(&run, "inference");
    let inference = execute(inference_request.clone())?;
    let mut resampling = adapter::request_for_run(&run, "resample");
    resampling["model"] = json!("ridge");
    resampling["request_id"] = json!(adapter::identity());
    let sealed = execute(resampling.clone())?;
    if !sealed["result"]["test_loss"].is_null()
        || sealed["result"]["folds"].as_array().map(Vec::len) != Some(3)
    {
        return Err("Reserved test or fold acceptance failed.".into());
    }
    let mut project = crate::research_ui::Research::default();
    project.template = resampling.clone();
    project.history.push(std::sync::Arc::new(json!({"request":inference_request,"result_sha256":adapter::sha256(&serde_json::to_vec(&inference["result"]).unwrap()),"response":inference})));
    let mut evaluate = resampling.clone();
    evaluate["operation"] = json!("evaluate_test");
    project.launch(evaluate)?;
    let ctx = eframe::egui::Context::default();
    let deadline = Instant::now();
    while project.busy() {
        project.poll(&ctx);
        if deadline.elapsed() > Duration::from_secs(350) {
            return Err("Research project evaluation timed out.".into());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    if !project.error.is_empty() {
        return Err(project.error.clone());
    }
    if project.disclosures.len() != 1 || project.disclosures[0]["status"] != "completed" {
        return Err("Explicit test disclosure was not retained in project history.".into());
    }
    let saved = serde_json::to_vec_pretty(&project.to_json()).map_err(|e| e.to_string())?;
    let mut restored = crate::research_ui::Research::default();
    restored.restore(serde_json::from_slice(&saved).map_err(|e| e.to_string())?)?;
    let base = std::env::temp_dir().join(format!(
        "Model Lab research acceptance {}",
        adapter::identity()
    ));
    let bundle = base.join("Research bundle with spaces");
    let analysis = &restored.history[0];
    let mut export = analysis["request"].clone();
    export["request_id"] = json!(adapter::identity());
    export["operation"] = json!("export");
    export["export_path"] = json!(bundle);
    export["source_request"] = analysis["request"].clone();
    export["source_response"] = analysis["response"].clone();
    export["source_wire"] = json!(serde_json::to_string_pretty(&analysis["request"]).unwrap());
    let exported = execute(export)?;
    for name in [
        "research_worker.py",
        "reproduce.py",
        "report.tex",
        "figure.pdf",
        "figure.svg",
        "student-worksheet.tex",
        "instructor-key.txt",
        "manifest.json",
    ] {
        if !bundle.join(name).is_file() {
            return Err(format!("Missing research export file: {name}"));
        }
    }
    let interpreter = adapter::default_interpreter();
    let reproduction=Command::new(&interpreter).args(["-I","-c","import subprocess,sys; p=subprocess.run([sys.argv[1],'-I',sys.argv[2]],capture_output=True,text=True,timeout=60); print(p.stdout); print(p.stderr,file=sys.stderr); sys.exit(p.returncode)",&interpreter,&bundle.join("reproduce.py").to_string_lossy()]).output().map_err(|e|e.to_string())?;
    if !reproduction.status.success() {
        return Err(format!(
            "Exported reproduction failed: {}",
            String::from_utf8_lossy(&reproduction.stderr)
        ));
    }
    let verification: Value = serde_json::from_slice(
        &std::fs::read(bundle.join("reproduction-verification.json")).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    if verification["status"] != "passed" {
        return Err("Exported reproduction did not verify.".into());
    }
    let info=Command::new(&interpreter).args(["-I","-c","import json,sys;print(json.dumps({'prefix':sys.prefix,'python':sys.version.split()[0]}))"]).output().map_err(|e|e.to_string())?;
    let info: Value = serde_json::from_slice(&info.stdout).map_err(|e| e.to_string())?;
    let path = std::path::PathBuf::from(&interpreter);
    let prefix = Path::new(info["prefix"].as_str().ok_or("Missing runtime prefix")?)
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let expected = if cfg!(windows) {
        path.parent().unwrap().to_path_buf()
    } else {
        path.parent().unwrap().parent().unwrap().to_path_buf()
    };
    let relocated =
        expected.canonicalize().is_ok_and(|p| p == prefix) && interpreter.contains("Python");
    if std::env::args().any(|a| a == "--require-bundled-research") && !relocated {
        return Err("Scientific runtime was not loaded from the packaged Python directory.".into());
    }
    let report = json!({"status":"passed","os":std::env::consts::OS,"engine_version":env!("CARGO_PKG_VERSION"),"protocol_version":adapter::SCHEMA,
        "capability_models":catalog["result"]["models"].as_array().map_or(0,Vec::len),
        "worker_sha256_declared":catalog["worker_sha256"],
        "inference_training_rows":inference["result"]["n"],
        "inference_request_sha256":inference["request_sha256"],
        "resampling_folds":sealed["result"]["folds"].as_array().map_or(0,Vec::len),
        "reserved_test_loss_before_evaluation":sealed["result"]["test_loss"],
        "test_disclosures_recorded":project.disclosures.len(),
        "restored_history_entries":restored.history.len(),
        "exported_reproduction":verification,
        "receipt_scope":"Values observed during this run of the harness; a passed status means each gate in research_acceptance::self_test held. Hashes are integrity checks, not authentication.","export_files":exported["result"]["files"],
        "runtime_relocated":relocated,"python":info["python"],"versions":catalog["versions"]});
    let _ = std::fs::remove_dir_all(&base);
    if let Some(path) = acceptance::argument("--report") {
        acceptance::write(&path, &report)?;
    } else {
        println!("{report}");
    }
    Ok(())
}
