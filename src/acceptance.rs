use serde_json::json;
use std::{
    path::{Path, PathBuf},
    sync::{atomic::AtomicBool, Arc},
    time::Duration,
};
pub fn argument(name: &str) -> Option<PathBuf> {
    let args: Vec<_> = std::env::args().collect();
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .map(PathBuf::from)
}
pub fn write(path: &Path, value: &serde_json::Value) -> Result<(), String> {
    std::fs::write(
        path,
        serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}
pub fn self_test() -> Result<(), String> {
    let data = crate::data::Table::parse("acceptance", include_str!("../sample.csv"))?
        .dataset(2, &[0, 1])?;
    let mut methods = vec![];
    for method in crate::model::Method::ALL {
        let fit = crate::model::fit(
            data.clone(),
            crate::model::Config {
                method,
                trees: 12,
                ..Default::default()
            },
            Arc::new(AtomicBool::new(false)),
        )?;
        if fit.predictions(12).iter().any(|v| !v.is_finite()) {
            return Err("Nonfinite prediction".into());
        }
        methods.push(method.label());
    }
    let python = crate::python::default_interpreter();
    let mut python_methods = vec![];
    let details=std::process::Command::new(&python).args(["-I","-c","import json,sys,sklearn; print(json.dumps({'prefix':sys.prefix,'python':sys.version.split()[0],'sklearn':sklearn.__version__}))"]).output().map_err(|e|e.to_string())?;
    let runtime: serde_json::Value =
        serde_json::from_slice(&details.stdout).map_err(|e| e.to_string())?;
    // Measured, not assumed: is the interpreter the one inside this package?
    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
    let expected = if cfg!(target_os = "windows") {
        executable.parent().unwrap().join("Python")
    } else {
        executable.parent().unwrap().join("../Resources/Python")
    };
    let prefix = Path::new(runtime["prefix"].as_str().unwrap_or(""));
    let runtime_relocated = match (prefix.canonicalize(), expected.canonicalize()) {
        (Ok(a), Ok(b)) => a.starts_with(&b),
        _ => false,
    };
    if cfg!(target_os = "windows") && !runtime_relocated {
        return Err("Packaged Python is not using the relocated runtime".into());
    }
    for method in crate::model::Method::ALL {
        for library in [false, true] {
            let cfg = crate::model::Config {
                method,
                trees: 2,
                ..Default::default()
            };
            let code = if library {
                crate::python::sklearn_code(&cfg)
            } else {
                crate::python::generated(&cfg)
            };
            let mut session = crate::python::start(
                &python,
                code,
                data.clone(),
                false,
                "Windows acceptance".into(),
            )?;
            let mut done = false;
            let mut errors = String::new();
            while let Ok(event) = session.receiver.recv_timeout(Duration::from_secs(20)) {
                match event {
                    crate::python::Event::Protocol(v) if v["kind"] == "done" => {
                        done = true;
                        break;
                    }
                    crate::python::Event::Output(line) => errors.push_str(&line),
                    _ => {}
                }
            }
            session.stop();
            if !done {
                return Err(format!(
                    "Python {} did not complete: {errors}",
                    method.label()
                ));
            }
            python_methods.push(format!(
                "{} / {}",
                method.label(),
                if library { "sklearn" } else { "transparent" }
            ));
        }
    }
    for method in crate::linear::LinearMethod::ALL {
        let cfg = crate::experiment::RunConfig {
            model: crate::experiment::ModelId::Linear(method),
            ..Default::default()
        };
        let data = crate::experiment::scenario(
            if method == crate::linear::LinearMethod::Logistic {
                3
            } else {
                2
            },
            4710,
            0.8,
        );
        let run = crate::experiment::Run::fit(
            "Packaged linear check".into(),
            data,
            cfg,
            Arc::new(AtomicBool::new(false)),
        )?;
        let mut session = crate::python::start(
            &python,
            crate::codegen::python(&run),
            run.data.clone(),
            false,
            "Packaged linear check".into(),
        )?;
        let mut done = false;
        let mut predictions = None;
        let mut errors = String::new();
        while let Ok(event) = session.receiver.recv_timeout(Duration::from_secs(20)) {
            match event {
                crate::python::Event::Protocol(v) if v["kind"] == "done" => {
                    done = true;
                    break;
                }
                crate::python::Event::Protocol(v) if v["kind"] == "stage" => {
                    predictions = Some(
                        serde_json::from_value::<Vec<f64>>(v["predictions"].clone())
                            .map_err(|e| e.to_string())?,
                    );
                }
                crate::python::Event::Output(line) => errors.push_str(&line),
                _ => {}
            }
        }
        session.stop();
        let predicted =
            predictions.ok_or_else(|| format!("No {} result: {errors}", method.label()))?;
        if !done
            || predicted.len() != run.data.y.len()
            || predicted
                .iter()
                .zip(run.predictions(run.last_stage()))
                .any(|(a, b)| !a.is_finite() || (a - b).abs() > 2e-5)
        {
            return Err(format!(
                "Packaged {} Python reproduction failed: {errors}",
                method.label()
            ));
        }
        methods.push(method.label());
        python_methods.push(format!("{} / sklearn", method.label()));
    }
    if crate::quiz::questions().len() != 100 {
        return Err("Expected100 questions".into());
    }
    let result = json!({"status":"passed","os":std::env::consts::OS,"arch":std::env::consts::ARCH,"native_models":methods,"python_models":python_methods,"python_version":runtime["python"],"sklearn_version":runtime["sklearn"],"runtime_relocated":runtime_relocated,"runtime_prefix":runtime["prefix"],"questions":100,"code_questions":crate::quiz::questions().iter().filter(|q|!q.code.is_empty()).count()});
    if let Some(path) = argument("--report") {
        write(&path, &result)?;
    }
    println!("{result}");
    Ok(())
}
pub fn save_png(path: &Path, image: &eframe::egui::ColorImage) -> Result<(), String> {
    let mut encoder = png::Encoder::new(
        std::fs::File::create(path).map_err(|e| e.to_string())?,
        image.size[0] as u32,
        image.size[1] as u32,
    );
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
    let bytes: Vec<u8> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
    writer.write_image_data(&bytes).map_err(|e| e.to_string())
}

pub fn linear_fixtures() -> Result<(), String> {
    use crate::{
        experiment::{scenario, ModelId, Run, RunConfig},
        linear::{LinearConfig, LinearMethod},
    };
    let mut fixtures = Vec::new();
    for method in LinearMethod::ALL {
        for seed in [7, 4710, 83] {
            let scenario_index = if method == LinearMethod::Logistic {
                3
            } else if seed == 83 {
                1
            } else {
                2
            };
            let data = scenario(scenario_index, seed, 0.8);
            let cfg = RunConfig {
                model: ModelId::Linear(method),
                seed,
                linear: LinearConfig {
                    method,
                    alpha: 0.15,
                    mix: 0.4,
                    tolerance: 1e-10,
                    max_iterations: 20000,
                    ..Default::default()
                },
                ..Default::default()
            };
            let run = Run::fit(
                format!("{} reference seed{seed}", method.label()),
                data,
                cfg,
                Arc::new(AtomicBool::new(false)),
            )?;
            fixtures.push(run.to_json());
        }
    }
    let result =
        json!({"schema_version":1,"engine_version":env!("CARGO_PKG_VERSION"),"fixtures":fixtures});
    if let Some(path) = argument("--report") {
        write(&path, &result)?;
    } else {
        println!("{result}");
    }
    Ok(())
}
/// Screenshot delivery is a separate bounded acceptance condition. A lost or
/// unavailable capture must not leave a native test running indefinitely.
pub struct CaptureGate {
    started: std::time::Instant,
    last: Option<std::time::Instant>,
    pub attempts: usize,
}
impl Default for CaptureGate {
    fn default() -> Self {
        Self {
            started: std::time::Instant::now(),
            last: None,
            attempts: 0,
        }
    }
}
impl CaptureGate {
    pub fn tick(&mut self, ctx: &eframe::egui::Context) {
        if self
            .last
            .is_none_or(|t| t.elapsed() >= std::time::Duration::from_millis(500))
        {
            ctx.send_viewport_cmd(eframe::egui::ViewportCommand::Screenshot(
                eframe::egui::UserData::default(),
            ));
            self.last = Some(std::time::Instant::now());
            self.attempts += 1;
        }
    }
    pub fn expired(&self) -> bool {
        self.started.elapsed() > std::time::Duration::from_secs(8)
    }
}
