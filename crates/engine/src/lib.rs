//! Model Lab engine: data, solvers, ensembles, experiment records and study
//! documents, with no user-interface dependency.

pub mod data;
pub mod decision;
pub mod experiment;
pub mod linear;
pub mod model;
pub mod provenance;
pub mod science;
pub mod study;

#[cfg(test)]
mod boundary {
    #[test]
    fn engine_has_no_ui_dependency() {
        let manifest = include_str!("../Cargo.toml");
        for banned in ["eframe", "egui", "wgpu", "png"] {
            assert!(!manifest.contains(banned), "{banned} in engine manifest");
        }
    }
}
