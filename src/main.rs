#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
mod acceptance;
mod adapter;
mod animation;
mod codegen;
mod data;
mod decision;
mod examples;
mod experiment;
mod instrument;
mod linear;
mod model;
mod playback;
mod python;
mod quiz;
mod research_acceptance;
mod research_ui;
mod science;
mod studio;
mod study;
mod study_acceptance;
mod study_ui;
mod teaching;
mod ui;
mod visuals;
mod workbench;
fn main() -> eframe::Result {
    if std::env::args().any(|a| a == "--study-self-test" || a == "--research-project-check") {
        let result = if std::env::args().any(|a| a == "--study-self-test") {
            study_acceptance::self_test()
        } else {
            study_acceptance::project_check()
        };
        if let Err(error) = result {
            if let Some(path) = acceptance::argument("--report") {
                let _ =
                    acceptance::write(&path, &serde_json::json!({"status":"failed","error":error}));
            }
            eprintln!("{error}");
            std::process::exit(1);
        }
        return Ok(());
    }
    if std::env::args().any(|a| a == "--research-request" || a == "--research-self-test") {
        let result = if std::env::args().any(|a| a == "--research-request") {
            research_acceptance::command()
        } else {
            research_acceptance::self_test()
        };
        if let Err(error) = result {
            if let Some(path) = acceptance::argument("--report") {
                let _ =
                    acceptance::write(&path, &serde_json::json!({"status":"failed","error":error}));
            }
            eprintln!("{error}");
            std::process::exit(1);
        }
        return Ok(());
    }
    if std::env::args().any(|a| a == "--check-decision-evidence") {
        let checked = acceptance::argument("--check-decision-evidence")
            .ok_or_else(|| "Missing evidence path".to_string())
            .and_then(|path| path.to_str().ok_or_else(|| "Evidence path is not UTF-8".to_string())
                .and_then(decision::Evidence::read));
        match checked {
            Ok(e) => println!("{}", serde_json::json!({
                "status":"valid", "round":e.round, "products":e.products.len(),
                "historical_groups":e.historical_validation.as_ref().map_or(0,|h|h.results.len()),
                "native_stress_cases":e.native_stress.as_ref().map_or(0,|n|n.cases.len()),
                "scope":"Schema and numeric checks only; not source authentication or predictive validation"
            })),
            Err(error) => { eprintln!("{error}"); std::process::exit(1); }
        }
        return Ok(());
    }
    if std::env::args().any(|a| a == "--adapter-catalog") {
        if let Some(path) = acceptance::argument("--report") {
            acceptance::write(&path, &adapter::catalogue())
                .map_err(|e| eframe::Error::AppCreation(e.into()))?;
        } else {
            println!("{}", adapter::catalogue());
        }
        return Ok(());
    }
    if std::env::args().any(|a| a == "--linear-fixtures") {
        if let Err(e) = acceptance::linear_fixtures() {
            eprintln!("{e}");
            std::process::exit(1);
        }
        return Ok(());
    }
    if std::env::args().any(|a| a == "--self-test") {
        if let Err(e) = acceptance::self_test() {
            eprintln!("{e}");
            std::process::exit(1);
        }
        return Ok(());
    }
    let mut options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1440.0, 1000.0])
            .with_min_inner_size([1000.0, 720.0]),
        ..Default::default()
    };
    if std::env::args().any(|a| a == "--compact-workbench") {
        options.viewport = options.viewport.with_inner_size([1000., 720.]);
    }
    if std::env::args().any(|a| a == "--software-renderer") {
        if let eframe::egui_wgpu::WgpuSetup::CreateNew(setup) = &mut options.wgpu_options.wgpu_setup
        {
            setup.native_adapter_selector = Some(std::sync::Arc::new(|adapters, surface| {
                adapters
                    .iter()
                    .filter(|a| surface.is_none_or(|s| a.is_surface_supported(s)))
                    .min_by_key(|a| {
                        if a.get_info().device_type == eframe::wgpu::DeviceType::Cpu {
                            0
                        } else {
                            1
                        }
                    })
                    .cloned()
                    .ok_or_else(|| "No surface-compatible graphics adapter".to_owned())
            }));
        }
    }
    if std::env::args().any(|a| a == "--legacy-tree-lab")
        || std::env::args().any(|a| a == "--ui-smoke-test")
    {
        eframe::run_native(
            concat!("Model Lab ", env!("CARGO_PKG_VERSION"), " — tree laboratory"),
            options,
            Box::new(|cc| Ok(Box::new(ui::Lab::new(cc)))),
        )
    } else {
        eframe::run_native(
            if std::env::args().any(|a| a == "--decision-evidence") {
                "Model Lab — decision landscape"
            } else {
                "Model Lab — learning workbench"
            },
            options,
            Box::new(|cc| Ok(Box::new(workbench::Workbench::new(cc)))),
        )
    }
}
