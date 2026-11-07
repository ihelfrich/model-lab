//! Typed simulation contracts and canonical project artifact identities.
use crate::adapter;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Component, Path, PathBuf},
};

pub const WORKER: &str = include_str!("../python/study_worker.py");
pub const PROTOCOL: u64 = 2;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ScenarioSpec {
    pub kind: String,
    pub version: u32,
    pub n: usize,
    pub groups: usize,
    pub group_size: usize,
    pub parameters: BTreeMap<String, f64>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MethodSpec {
    pub id: String,
    pub label: String,
    pub model: String,
    pub operation: String,
    pub covariance: String,
    pub oracle: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct StudySpec {
    pub version: u32,
    pub aim: String,
    pub estimand: String,
    pub scenario: ScenarioSpec,
    pub methods: Vec<MethodSpec>,
    pub replications: usize,
    pub seed: u64,
    pub max_fits: usize,
    pub seconds: u64,
}
impl Default for StudySpec {
    fn default() -> Self {
        Self::for_scenario("heteroskedasticity")
    }
}
impl MethodSpec {
    pub fn interval(id: &str, label: &str, covariance: &str, oracle: bool) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            model: "ols".into(),
            operation: "inference".into(),
            covariance: covariance.into(),
            oracle,
        }
    }
    pub fn predictive(model: &str) -> Self {
        Self {
            id: format!("predict_{model}"),
            label: format!("{model}: prediction"),
            model: model.into(),
            operation: "predict".into(),
            covariance: "none".into(),
            oracle: false,
        }
    }
    pub fn cost(&self) -> usize {
        if self.operation == "inference" {
            2
        } else {
            1
        }
    }
}
impl StudySpec {
    pub fn for_scenario(kind: &str) -> Self {
        let methods = match kind {
            "confounding" => vec![
                MethodSpec::interval("observed", "Observed OLS / HC3", "HC3", false),
                MethodSpec::interval("oracle", "Simulator-only oracle / HC3", "HC3", true),
            ],
            "clustering" => vec![
                MethodSpec::interval("classical", "OLS classical", "classical", false),
                MethodSpec::interval("cluster", "OLS cluster", "cluster", false),
            ],
            _ => vec![
                MethodSpec::interval("classical", "OLS classical", "classical", false),
                MethodSpec::interval("robust", "OLS HC3", "HC3", false),
            ],
        };
        Self{version:1,aim:"Examine estimation, uncertainty and assumptions under a known generating mechanism.".into(),estimand:if kind=="confounding"{"structural_effect"}else{"regression_coefficient"}.into(),
            scenario:ScenarioSpec{kind:kind.into(),version:1,n:200,groups:20,group_size:10,parameters:[("beta",1.),("tau",1.),("sigma",1.),("h",1.),("c",1.),("gamma",1.),("rho",0.5)].into_iter().map(|(k,v)|(k.into(),v)).collect()},
            methods,replications:200,seed:471008,max_fits:1000,seconds:300}
    }
    pub fn truth(&self) -> f64 {
        if self.estimand == "regression_intercept" {
            1.
        } else {
            self.scenario.parameters[if self.scenario.kind == "confounding" {
                "tau"
            } else {
                "beta"
            }]
        }
    }
    pub fn planned_fits(&self) -> usize {
        self.replications
            .saturating_mul(self.methods.iter().map(MethodSpec::cost).sum())
    }
    pub fn wire(&self) -> String {
        serde_json::to_string_pretty(self).expect("finite validated study")
    }
    pub fn validate(&self) -> Result<(), String> {
        let s = &self.scenario;
        if self.version != 1
            || s.version != 1
            || !matches!(
                s.kind.as_str(),
                "heteroskedasticity" | "confounding" | "clustering"
            )
            || self.aim.is_empty()
            || self.aim.len() > 2000
        {
            return Err(
                "Select a supported versioned study and enter an aim (up to 2,000 characters)."
                    .into(),
            );
        }
        if !(50..=2000).contains(&s.n)
            || !(5..=50).contains(&s.groups)
            || !(10..=40).contains(&s.group_size)
            || (s.kind == "clustering" && s.n != s.groups * s.group_size)
        {
            return Err("Use 50–2,000 observations; grouped samples require groups times observations per group.".into());
        }
        let bounds = [
            ("beta", -3., 3.),
            ("tau", -3., 3.),
            ("sigma", 0.1, 3.),
            ("h", 0., 4.),
            ("c", -2., 2.),
            ("gamma", -2., 2.),
            ("rho", 0., 0.9),
        ];
        if s.parameters.len() != bounds.len()
            || bounds.iter().any(|(k, lo, hi)| {
                s.parameters
                    .get(*k)
                    .is_none_or(|v| !v.is_finite() || v < lo || v > hi)
            })
        {
            return Err("Scenario parameters exceed their declared bounds.".into());
        }
        let estimand = if s.kind == "confounding" {
            "structural_effect"
        } else {
            "regression_coefficient"
        };
        if (self.estimand != estimand
            && !(s.kind != "confounding" && self.estimand == "regression_intercept"))
            || !(2..=500).contains(&self.replications)
            || self.seed > u32::MAX as u64
            || !(1..=1000).contains(&self.max_fits)
            || !(1..=300).contains(&self.seconds)
        {
            return Err("Invalid estimand, replications, seed or resource budget.".into());
        }
        let mut ids = BTreeSet::new();
        if self.methods.is_empty() || self.methods.len() > 4 {
            return Err("Choose one to four method variants.".into());
        }
        for m in &self.methods {
            if m.id.is_empty()
                || m.id.len() > 80
                || !ids.insert(&m.id)
                || m.label.is_empty()
                || m.label.len() > 160
                || (m.oracle && s.kind != "confounding")
            {
                return Err("Invalid or duplicate method identity or oracle declaration.".into());
            }
            if !matches!(m.model.as_str(), "ols" | "ridge" | "forest")
                || !matches!(m.operation.as_str(), "predict" | "inference")
                || (m.operation == "inference"
                    && (m.model != "ols"
                        || !matches!(m.covariance.as_str(), "classical" | "HC3" | "cluster")
                        || (m.covariance == "cluster" && s.kind != "clustering")))
                || (m.operation == "predict" && m.covariance != "none")
            {
                return Err(
                    "The selected method does not support this target or covariance.".into(),
                );
            }
        }
        if self.planned_fits() > self.max_fits {
            return Err(format!("This design requires {} backend fits; the allowance is {}. Reduce replications or variants.",self.planned_fits(),self.max_fits));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Metric {
    pub value: Option<f64>,
    pub mc_se: Option<f64>,
    pub n: usize,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MethodResult {
    pub method_id: String,
    pub status: String,
    pub estimate: Option<f64>,
    pub std_error: Option<f64>,
    pub lower: Option<f64>,
    pub upper: Option<f64>,
    pub truth: f64,
    pub prediction_mse: BTreeMap<String, f64>,
    pub error: Option<String>,
    pub warnings: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReplicateResult {
    pub replicate: usize,
    pub data_sha256: String,
    pub methods: Vec<MethodResult>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FitReservation {
    pub replicate: usize,
    pub method_id: String,
    pub purpose: String,
    pub slots: usize,
    pub actual_fits: usize,
    pub status: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MethodSummary {
    pub method_id: String,
    pub label: String,
    pub attempted: usize,
    pub completed: usize,
    pub failed: usize,
    pub failure_rate: Metric,
    pub bias: Metric,
    pub mse: Metric,
    pub empirical_sd: Option<f64>,
    pub empirical_sd_mc_se: Option<f64>,
    pub coverage_valid_intervals: Metric,
    pub valid_and_covering: Metric,
    pub interval_width: Metric,
    pub prediction: BTreeMap<String, Metric>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StudySummary {
    pub completed_replications: usize,
    pub attempted_replications: usize,
    pub requested_replications: usize,
    pub methods: Vec<MethodSummary>,
    pub paired: Vec<Value>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StudyDocument {
    pub schema_version: u64,
    pub study_id: String,
    pub spec: StudySpec,
    pub spec_wire: String,
    pub spec_sha256: String,
    pub provenance: Value,
    pub status: String,
    pub records: Vec<ReplicateResult>,
    pub reservations: Vec<FitReservation>,
    pub summary: StudySummary,
    pub integrity_sha256: String,
    #[serde(default)]
    pub stop_reason: Option<String>,
}
impl StudyDocument {
    /// Recompute bias, coefficient MSE, interval coverage and failure rate from
    /// the replication records and require the worker's summary to agree. The
    /// worker wrote the summary; this is the native, independent arithmetic.
    /// `integrity_sha256` is a worker-side corruption check over its own JSON
    /// serialization and is not recomputed here.
    fn check_recomputed_summary(&self) -> Result<(), String> {
        fn mean(values: &[f64]) -> Option<f64> {
            if values.is_empty() {
                None
            } else {
                Some(values.iter().sum::<f64>() / values.len() as f64)
            }
        }
        fn agree(name: &str, metric: &Metric, values: &[f64]) -> Result<(), String> {
            let expected = mean(values);
            let close = match (metric.value, expected) {
                (None, None) => true,
                (Some(a), Some(b)) => (a - b).abs() <= 1e-9 * (1. + a.abs().max(b.abs())),
                _ => false,
            };
            if metric.n != values.len() || !close {
                return Err(format!(
                    "Study summary {name} does not match the arithmetic of its records (reported {:?} over {}, recomputed {:?} over {}).",
                    metric.value, metric.n, expected, values.len()
                ));
            }
            Ok(())
        }
        for summary in &self.summary.methods {
            let rows = self
                .records
                .iter()
                .flat_map(|r| r.methods.iter())
                .filter(|m| m.method_id == summary.method_id)
                .collect::<Vec<_>>();
            let valid = rows.iter().filter(|m| m.status == "completed").collect::<Vec<_>>();
            let errors = valid
                .iter()
                .filter_map(|m| m.estimate.map(|b| b - m.truth))
                .collect::<Vec<_>>();
            let squared = errors.iter().map(|e| e * e).collect::<Vec<_>>();
            let intervals = valid
                .iter()
                .filter(|m| m.lower.is_some() && m.upper.is_some())
                .collect::<Vec<_>>();
            let cover = intervals
                .iter()
                .map(|m| f64::from(m.lower.unwrap() <= m.truth && m.truth <= m.upper.unwrap()))
                .collect::<Vec<_>>();
            let failures = rows
                .iter()
                .map(|m| f64::from(m.status != "completed"))
                .collect::<Vec<_>>();
            agree("bias", &summary.bias, &errors)?;
            agree("coefficient MSE", &summary.mse, &squared)?;
            agree("coverage among valid intervals", &summary.coverage_valid_intervals, &cover)?;
            agree("failure rate", &summary.failure_rate, &failures)?;
        }
        Ok(())
    }
    pub fn validate(&self, entry: &StudyRef) -> Result<(), String> {
        self.spec.validate()?;
        if self.schema_version != 2
            || self.study_id != entry.id
            || self.spec != entry.spec
            || self.spec_sha256 != adapter::sha256(entry.spec_wire.as_bytes())
            || self.spec_wire != entry.spec_wire
        {
            return Err("Study checkpoint identity/specification mismatch.".into());
        }
        let sources = self.provenance["sources"]
            .as_object()
            .ok_or("Missing source provenance")?;
        if sources.len() != 2
            || sources
                .values()
                .any(|v| v.as_str().is_none_or(|s| s.len() != 64))
        {
            return Err("Invalid worker source provenance.".into());
        }
        let mut reps = BTreeSet::new();
        let methods = self
            .spec
            .methods
            .iter()
            .map(|m| (m.id.as_str(), m))
            .collect::<BTreeMap<_, _>>();
        for r in &self.records {
            if r.replicate >= self.spec.replications
                || !reps.insert(r.replicate)
                || r.data_sha256.len() != 64
            {
                return Err("Duplicate or invalid replication identity.".into());
            }
            let mut ids = BTreeSet::new();
            for m in &r.methods {
                let desc = methods
                    .get(m.method_id.as_str())
                    .ok_or("Unknown method in checkpoint")?;
                if !ids.insert(&m.method_id)
                    || !matches!(m.status.as_str(), "completed" | "failed")
                    || (desc.operation == "predict"
                        && (m.estimate.is_some() || m.lower.is_some() || m.upper.is_some()))
                    || !m.truth.is_finite()
                {
                    return Err("Invalid replication capability or status.".into());
                }
                if m.status == "failed" && (m.estimate.is_some() || m.error.is_none()) {
                    return Err("Failed methods must retain a failure, not an estimate.".into());
                }
                if m.truth != self.spec.truth()
                    || m.prediction_mse.values().any(|v| !v.is_finite() || *v < 0.)
                {
                    return Err("Simulation target or predictive metric is invalid.".into());
                }
                if m.status == "completed" && desc.operation == "inference" {
                    match (m.estimate, m.std_error, m.lower, m.upper) {
                        (Some(b), Some(se), Some(lo), Some(hi))
                            if b.is_finite()
                                && se.is_finite()
                                && se > 0.
                                && lo.is_finite()
                                && hi.is_finite()
                                && lo <= b
                                && b <= hi => {}
                        _ => {
                            return Err(
                                "Completed interval result lacks a finite valid interval.".into()
                            )
                        }
                    }
                }
            }
        }
        for r in &self.reservations {
            let m = methods
                .get(r.method_id.as_str())
                .ok_or("Unknown fit reservation")?;
            if r.replicate >= self.spec.replications
                || r.slots != m.cost()
                || r.actual_fits > r.slots
                || !matches!(r.purpose.as_str(), "study" | "inspection")
                || !matches!(
                    r.status.as_str(),
                    "running" | "uncertain" | "completed" | "failed"
                )
            {
                return Err("Invalid fit reservation.".into());
            }
        }
        if self.spent() > self.spec.max_fits
            || self.summary.requested_replications != self.spec.replications
            || self.summary.completed_replications
                != self
                    .records
                    .iter()
                    .filter(|r| r.methods.len() == self.spec.methods.len())
                    .count()
        {
            return Err(
                "Study summary or resource allowance does not match its replication records."
                    .into(),
            );
        }
        if self.summary.methods.len() != self.spec.methods.len() {
            return Err("Method summary is incomplete.".into());
        }
        let mut summaries = BTreeSet::new();
        for summary in &self.summary.methods {
            let rows = self
                .records
                .iter()
                .flat_map(|r| r.methods.iter())
                .filter(|m| m.method_id == summary.method_id)
                .collect::<Vec<_>>();
            if !methods.contains_key(summary.method_id.as_str())
                || !summaries.insert(&summary.method_id)
                || summary.attempted != rows.len()
                || summary.completed != rows.iter().filter(|m| m.status == "completed").count()
                || summary.failed != summary.attempted - summary.completed
            {
                return Err("Failure denominators do not match the retained results.".into());
            }
        }
        self.check_recomputed_summary()?;
        if !matches!(self.status.as_str(), "running" | "partial" | "completed")
            || (self.status == "completed"
                && self.summary.completed_replications != self.spec.replications)
        {
            return Err("Study completion status is inconsistent with its records.".into());
        }
        Ok(())
    }
    pub fn spent(&self) -> usize {
        self.reservations
            .iter()
            .map(|r| {
                if matches!(r.status.as_str(), "running" | "uncertain") {
                    r.slots
                } else {
                    r.actual_fits
                }
            })
            .sum()
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StudyRef {
    pub id: String,
    pub spec: StudySpec,
    pub spec_wire: String,
    pub relative_path: String,
    #[serde(default)]
    pub answers: Vec<String>,
}
impl StudyRef {
    pub fn new(spec: StudySpec, store: &str) -> Result<Self, String> {
        spec.validate()?;
        let id = adapter::identity();
        let spec_wire = spec.wire();
        Ok(Self {
            relative_path: format!("{store}/{id}.json"),
            id,
            spec,
            spec_wire,
            answers: vec![String::new(); 2],
        })
    }
    pub fn path(&self, project: &Path) -> Result<PathBuf, String> {
        let rel = Path::new(&self.relative_path);
        let parts = rel.components().collect::<Vec<_>>();
        if parts.len() != 2
            || parts.iter().any(|p| !matches!(p, Component::Normal(_)))
            || !parts[0].as_os_str().to_string_lossy().ends_with(".studies")
            || parts[1].as_os_str() != std::ffi::OsStr::new(&format!("{}.json", self.id))
            || self.id.is_empty()
            || self.id.len() > 120
            || !self
                .id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        {
            return Err(
                "Study artifacts must remain inside the project's associated .studies folder."
                    .into(),
            );
        }
        let root = project
            .parent()
            .ok_or("Project has no parent folder")?
            .canonicalize()
            .map_err(|e| e.to_string())?;
        let path = root.join(rel);
        if path.parent().unwrap().exists()
            && !path
                .parent()
                .unwrap()
                .canonicalize()
                .map_err(|e| e.to_string())?
                .starts_with(&root)
        {
            return Err("Study artifact symlink escapes its project folder.".into());
        }
        if path.exists()
            && !path
                .canonicalize()
                .map_err(|e| e.to_string())?
                .starts_with(&root)
        {
            return Err("Study checkpoint symlink escapes its project folder.".into());
        }
        Ok(path)
    }
    pub fn request(&self, project: &Path, operation: &str) -> Result<Value, String> {
        self.spec.validate()?;
        Ok(
            json!({"schema_version":2,"request_id":adapter::identity(),"operation":operation,"study_id":self.id,"study_spec":self.spec,"spec_wire":self.spec_wire,"project_root":project.parent().ok_or("Missing project folder")?.canonicalize().map_err(|e|e.to_string())?,"checkpoint_path":self.path(project)?}),
        )
    }
    pub fn load(&self, project: &Path) -> Result<StudyDocument, String> {
        let path = self.path(project)?;
        if std::fs::metadata(&path).map_err(|e| e.to_string())?.len() > 64 * 1024 * 1024 {
            return Err("Checkpoint exceeds 64 MiB.".into());
        }
        let doc: StudyDocument =
            serde_json::from_value(read_checkpoint(&path)?).map_err(|e| e.to_string())?;
        doc.validate(self)?;
        Ok(doc)
    }
}

pub fn read_checkpoint(path: &Path) -> Result<Value, String> {
    if std::fs::metadata(path).map_err(|e| e.to_string())?.len() > 64 * 1024 * 1024 {
        return Err("Checkpoint exceeds 64 MiB.".into());
    }
    let envelope: Value = serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let wire = envelope["document_wire"]
        .as_str()
        .ok_or("Missing checkpoint document wire")?;
    if envelope["schema_version"] != 2
        || envelope["document_sha256"].as_str() != Some(&adapter::sha256(wire.as_bytes()))
    {
        return Err("Study checkpoint integrity validation failed.".into());
    }
    serde_json::from_str(wire).map_err(|e| e.to_string())
}

pub fn manifest() -> Value {
    json!({"study_worker.py":adapter::sha256(WORKER.as_bytes()),"research_worker.py":adapter::sha256(adapter::WORKER.as_bytes())})
}
pub fn bundle_hash() -> String {
    adapter::sha256(&serde_json::to_vec(&manifest()).unwrap())
}
pub fn validate_request(request: &Value) -> Result<(), String> {
    if request["schema_version"].as_u64() != Some(PROTOCOL)
        || request["request_id"].as_str().is_none_or(str::is_empty)
        || !matches!(
            request["operation"].as_str().unwrap_or(""),
            "study_preflight" | "study_preview" | "simulate" | "study_inspect" | "study_export"
        )
    {
        return Err("Unsupported study protocol/operation or missing request identity.".into());
    }
    let spec: StudySpec =
        serde_json::from_value(request["study_spec"].clone()).map_err(|e| e.to_string())?;
    spec.validate()?;
    let wire = request["spec_wire"]
        .as_str()
        .ok_or("Missing exact study specification wire")?;
    if serde_json::from_str::<Value>(wire).map_err(|e| e.to_string())? != request["study_spec"] {
        return Err("Study wire does not match its specification.".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn typed_study_cost_and_capability() {
        let mut s = StudySpec::default();
        s.validate().unwrap();
        assert_eq!(s.planned_fits(), 800);
        s.replications = 500;
        assert!(s.validate().unwrap_err().contains("2000"));
        s.replications = 4;
        s.methods[0].model = "forest".into();
        assert!(s.validate().is_err());
    }
    #[test]
    fn artifact_traversal_and_wire_rejected() {
        let temp = std::env::temp_dir().join("test-project.json");
        let mut r = StudyRef::new(StudySpec::default(), "test.studies").unwrap();
        r.relative_path = "../escape.json".into();
        assert!(r.path(&temp).is_err());
        let s = StudySpec::default();
        let mut q = json!({"schema_version":2,"request_id":"test","operation":"study_preview","study_spec":s,"spec_wire":s.wire()});
        validate_request(&q).unwrap();
        q["study_spec"]["seed"] = json!(8);
        assert!(validate_request(&q).is_err());
    }
}
