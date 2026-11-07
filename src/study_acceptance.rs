//! Real child-process, project, audience and cancellation acceptance for 0.8.
use crate::{acceptance, adapter, experiment, research_ui, study::*, study_ui::Action};
use serde_json::{json, Value};
use std::{
    path::Path,
    process::Command,
    sync::{atomic::AtomicBool, Arc},
    time::{Duration, Instant},
};

fn await_study(project: &mut research_ui::Research) -> Result<(), String> {
    let ctx = eframe::egui::Context::default();
    let start = Instant::now();
    while project.studies.busy() {
        project.poll(&ctx);
        if start.elapsed() > Duration::from_secs(340) {
            project.studies.cancel();
            return Err("Study acceptance exceeded its bounded deadline.".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    if !project.studies.error.is_empty() {
        return Err(project.studies.error.clone());
    }
    Ok(())
}

pub fn seed_project(path: &Path) -> Result<research_ui::Research, String> {
    let run = experiment::Run::fit(
        "Simulation acceptance".into(),
        experiment::scenario(0, 4710, 0.8),
        experiment::RunConfig::default(),
        Arc::new(AtomicBool::new(false)),
    )?;
    let mut project = research_ui::Research::default();
    project.synchronize(&run, None, 1);
    project.project_path = path.to_string_lossy().into_owned();
    project.save()?;
    Ok(project)
}

pub fn self_test() -> Result<(), String> {
    let base =
        std::env::temp_dir().join(format!("model-lab-08-acceptance-{}", adapter::identity()));
    std::fs::create_dir(&base).map_err(|e| e.to_string())?;
    let result = (|| {
        let project_path = base.join("Research project with spaces.json");
        let mut project = seed_project(&project_path)?;
        let mut scenarios = vec![];
        for kind in ["heteroskedasticity", "confounding", "clustering"] {
            project.studies.draft = StudySpec::for_scenario(kind);
            project.studies.draft.replications = 4;
            project.studies.add_study()?;
            project.save()?;
            project
                .studies
                .run_selected(false, project.interpreter.clone())?;
            await_study(&mut project)?;
            let doc = project
                .studies
                .current_document()
                .ok_or("Missing study checkpoint")?;
            if doc.status != "completed"
                || doc.summary.completed_replications != 4
                || doc.spent() != 16
            {
                return Err("Study did not complete with the declared fit accounting.".into());
            }
            scenarios.push(kind);
        }
        project.studies.replicate = 1;
        project.studies.method_id = project.studies.current().unwrap().spec.methods[0]
            .id
            .clone();
        project.studies.inspect(project.interpreter.clone())?;
        await_study(&mut project)?;
        if !project
            .studies
            .inspection
            .as_ref()
            .is_some_and(|r| r["verified"] == true)
        {
            return Err("Replication inspection was not verified.".into());
        }
        for audience in ["student", "research"] {
            project.studies.audience = audience.into();
            project.studies.export_path = base.join(audience).to_string_lossy().into_owned();
            project.studies.export(project.interpreter.clone())?;
            await_study(&mut project)?;
        }
        if base.join("student/study.json").exists()
            || base.join("student/instructor-key.txt").exists()
        {
            return Err("Student packet contains instructor material.".into());
        }
        let interpreter = adapter::default_interpreter();
        let reproduce=Command::new(&interpreter).args(["-I","-B","-c","import subprocess,sys; p=subprocess.run([sys.executable,'-I','-B',sys.argv[1]],timeout=90);sys.exit(p.returncode)"]).arg(base.join("research/reproduce.py")).output().map_err(|e|e.to_string())?;
        if !reproduce.status.success() {
            return Err(format!(
                "Exported study reproduction failed: {}",
                String::from_utf8_lossy(&reproduce.stderr)
            ));
        }
        project.save()?;
        let saved: Value =
            serde_json::from_slice(&std::fs::read(&project_path).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        if saved["schema_version"] != 2 {
            return Err("Study project did not use version 2.".into());
        }
        let mut restored = research_ui::Research::default();
        restored.project_path = project_path.to_string_lossy().into_owned();
        restored.restore(saved.clone())?;
        if restored.studies.entries.len() != 3 || restored.studies.documents.len() != 3 {
            return Err("Study references did not restore.".into());
        }
        let moved = base.join("moved");
        std::fs::create_dir(&moved).map_err(|e| e.to_string())?;
        restored.project_path = moved
            .join("Relocated project.json")
            .to_string_lossy()
            .into_owned();
        restored.save()?;
        let mut relocated = research_ui::Research::default();
        relocated.project_path = restored.project_path.clone();
        relocated.restore(
            serde_json::from_slice(
                &std::fs::read(&relocated.project_path).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?,
        )?;
        if relocated.studies.documents.len() != 3 {
            return Err("Relocated project artifacts were not readable.".into());
        }
        // Exercise actual cancellation through the same child boundary, then
        // resume from the canonical on-disk checkpoint without erasing attempts.
        project.studies.draft = StudySpec::default();
        project.studies.draft.replications = 12;
        project.studies.add_study()?;
        project.save()?;
        project
            .studies
            .run_selected(false, project.interpreter.clone())?;
        let ctx = eframe::egui::Context::default();
        let deadline = Instant::now();
        while project.studies.busy() && project.studies.current_document().is_none() {
            project.poll(&ctx);
            if deadline.elapsed() > Duration::from_secs(45) {
                return Err("No live checkpoint before cancellation.".into());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let cancel = Instant::now();
        project.studies.cancel();
        let checkpoint = project.studies.current().unwrap().path(&project_path)?;
        // Process shutdown is asynchronous. A new worker uses the OS file lock,
        // and cannot overlap or delete an active worker's lock.
        await_study(&mut project)?;
        if cancel.elapsed() > Duration::from_secs(2) || !checkpoint.exists() {
            return Err("Cancellation/checkpoint acceptance failed.".into());
        }
        project
            .studies
            .run_selected(true, project.interpreter.clone())?;
        await_study(&mut project)?;
        if project.studies.current_document().unwrap().status != "completed" {
            return Err("Canceled study did not resume to completion.".into());
        }
        // Pure egui layout timing is labelled separately from end-to-end input
        // latency; it cannot certify real hardware interaction or accessibility.
        for stage in 0..5 {
            project.studies.stage = stage;
            let mut path = project.project_path.clone();
            let _ = ctx.run_ui(Default::default(), |ui| {
                let _: Option<Action> = project.studies.ui(ui, &mut path);
            });
        }
        let mut timings = project.studies.frame_ms.clone();
        timings.sort_by(f64::total_cmp);
        let p95 = timings
            .get((timings.len() * 95 / 100).min(timings.len().saturating_sub(1)))
            .copied();
        let probe=Command::new(&interpreter).args(["-I","-B","-c","import json,sys;print(json.dumps({'prefix':sys.prefix,'python':sys.version.split()[0]}))"]).output().map_err(|e|e.to_string())?;
        let runtime: Value = serde_json::from_slice(&probe.stdout).map_err(|e| e.to_string())?;
        let executable = std::env::current_exe().map_err(|e| e.to_string())?;
        let expected = executable.parent().unwrap().join(if cfg!(windows) {
            "Python"
        } else {
            "../Resources/Python"
        });
        let relocated = expected.canonicalize().is_ok_and(|p| {
            Path::new(runtime["prefix"].as_str().unwrap_or(""))
                .canonicalize()
                .is_ok_and(|actual| actual == p)
        });
        if std::env::args().any(|a| a == "--require-bundled-research") && !relocated {
            return Err("Study acceptance did not use the bundled scientific runtime.".into());
        }
        if project.studies.cancel_exit_ms.is_none_or(|ms| ms > 2000.) {
            return Err("Study child did not acknowledge shutdown within two seconds.".into());
        }
        let report = json!({"status":"passed","engine_version":env!("CARGO_PKG_VERSION"),"protocol_version":2,"os":std::env::consts::OS,"scenarios":scenarios,"actual_child_process":true,"fit_accounting":true,"checkpoint_and_resume":true,"project_v2_restored":true,"project_artifacts_relocated":true,"inspection_regenerated":true,"student_packet_excludes_instructor_data":true,"exported_reproduction":true,"cancel_ack_ms":project.studies.cancel_ack_ms,"cancel_exit_ms":project.studies.cancel_exit_ms,"ui_build_p95_ms":p95,"runtime_relocated":relocated,"python":runtime["python"],"latency_scope":"CPU egui layout only; not end-to-end input or GPU presentation latency.","human_pilot":"unavailable per user; not evaluated","actual_windows_desktop":"unavailable per user; CI only","voiceover":"unsupported"});
        if let Some(path) = acceptance::argument("--report") {
            acceptance::write(&path, &report)?;
        } else {
            println!("{report}");
        }
        Ok(())
    })();
    let _ = std::fs::remove_dir_all(base);
    result
}

pub fn project_check() -> Result<(), String> {
    let input =
        acceptance::argument("--research-project-check").ok_or("Missing project-check request")?;
    if std::fs::metadata(&input).map_err(|e| e.to_string())?.len() > 32 * 1024 * 1024 {
        return Err("Request exceeds 32 MiB.".into());
    }
    let request: Value = serde_json::from_slice(&std::fs::read(input).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let response = crate::research_acceptance::execute(request.clone())?;
    let base =
        std::env::temp_dir().join(format!("model-lab-project-check-{}", adapter::identity()));
    std::fs::create_dir(&base).map_err(|e| e.to_string())?;
    let mut project = research_ui::Research::default();
    project.template = request.clone();
    project.project_path = base
        .join("public-reference.json")
        .to_string_lossy()
        .into_owned();
    project.history.push(Arc::new(json!({"request":request,"result_sha256":adapter::sha256(&serde_json::to_vec(&response["result"]).unwrap()),"response":response})));
    project.save()?;
    let mut restored = research_ui::Research::default();
    restored.project_path = project.project_path.clone();
    restored.restore(
        serde_json::from_slice(&std::fs::read(&project.project_path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?,
    )?;
    let record = &restored.history[0];
    let mut export = record["request"].clone();
    export["operation"] = json!("export");
    export["source_request"] = record["request"].clone();
    export["source_response"] = record["response"].clone();
    export["source_wire"] = json!(serde_json::to_string_pretty(&record["request"]).unwrap());
    export["export_path"] = json!(base.join("reproduction"));
    crate::research_acceptance::execute(export)?;
    let output=Command::new(adapter::default_interpreter()).args(["-I","-B","-c","import subprocess,sys;p=subprocess.run([sys.executable,'-I','-B',sys.argv[1]],timeout=60);sys.exit(p.returncode)"]).arg(base.join("reproduction/reproduce.py")).output().map_err(|e|e.to_string())?;
    if !output.status.success() {
        return Err(format!(
            "Public-reference reproduction failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let report = json!({"status":"passed","project_restored":true,"exported_reproduction":true,"response":record["response"]});
    if let Some(path) = acceptance::argument("--report") {
        acceptance::write(&path, &report)?;
    } else {
        println!("{report}");
    }
    let _ = std::fs::remove_dir_all(base);
    Ok(())
}
