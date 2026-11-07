"""Model Lab 0.8: versioned, failure-aware simulation studies.

The 0.7 scientific worker is an immutable numerical dependency. Study checkpoints
are canonical project artifacts, locked across processes and committed before a
fit reservation. They are data, never imported or executed as Python.
"""
from __future__ import annotations
import argparse
from contextlib import contextmanager
import csv
from dataclasses import dataclass, asdict
import hashlib
import importlib.metadata
import json
import math
import os
from pathlib import Path
import shutil
import sys
import tempfile
import time
import warnings

sys.path.insert(0, str(Path(__file__).resolve().parent))
import numpy as np
import pandas as pd
import research_worker as science

SCHEMA = 2
FROZEN_V07 = "3522d9b34131b8f551ecd2b499690a62cda1b4e8058be5c0cd8d3e891ac35406"
if hashlib.sha256(Path(science.__file__).read_bytes()).hexdigest() != FROZEN_V07:
    raise RuntimeError("The frozen 0.7 numerical worker was changed.")


def packed(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"), allow_nan=False).encode()


def digest(value):
    return hashlib.sha256(packed(value)).hexdigest()


def source_manifest():
    return {p.name: hashlib.sha256(p.read_bytes()).hexdigest()
            for p in (Path(__file__), Path(science.__file__))}


def provenance():
    return {"sources": source_manifest(), "python": sys.version.split()[0],
            "packages": {p: importlib.metadata.version(p) for p in
                         ("numpy", "scipy", "pandas", "scikit-learn", "statsmodels", "matplotlib")}}


def integer(value, lo, hi, name):
    if type(value) is not int or not lo <= value <= hi:
        raise ValueError(f"{name} must be an integer between {lo} and {hi}.")
    return value


def number(value, lo, hi, name):
    if type(value) not in (int, float) or not math.isfinite(value) or not lo <= value <= hi:
        raise ValueError(f"{name} must be finite and between {lo} and {hi}.")
    return float(value)


@dataclass(frozen=True)
class ScenarioSpec:
    kind: str
    version: int
    n: int
    groups: int
    group_size: int
    parameters: dict


@dataclass(frozen=True)
class MethodSpec:
    id: str
    label: str
    model: str
    operation: str
    covariance: str
    oracle: bool


@dataclass(frozen=True)
class StudySpec:
    version: int
    aim: str
    estimand: str
    scenario: ScenarioSpec
    methods: tuple[MethodSpec, ...]
    replications: int
    seed: int
    max_fits: int
    seconds: int


def validate_spec(value):
    keys = set(StudySpec.__dataclass_fields__)
    if not isinstance(value, dict) or set(value) != keys:
        raise ValueError("StudySpec has missing or unrecognized fields.")
    if value["version"] != 1 or not isinstance(value["aim"], str) or not 1 <= len(value["aim"]) <= 2000:
        raise ValueError("Use study version 1 with a nonempty aim (up to 2,000 characters).")
    scenario = value["scenario"]
    if not isinstance(scenario, dict) or set(scenario) != set(ScenarioSpec.__dataclass_fields__):
        raise ValueError("ScenarioSpec has missing or unrecognized fields.")
    kind = scenario["kind"]
    if kind not in ("heteroskedasticity", "confounding", "clustering") or scenario["version"] != 1:
        raise ValueError("Select a supported versioned simulation template.")
    integer(scenario["n"], 50, 2000, "Sample size")
    integer(scenario["groups"], 5, 50, "Groups")
    integer(scenario["group_size"], 10, 40, "Observations per group")
    if kind == "clustering" and scenario["n"] != scenario["groups"] * scenario["group_size"]:
        raise ValueError("The clustered sample size must equal groups times observations per group.")
    bounds = {"beta": (-3, 3), "tau": (-3, 3), "sigma": (.1, 3),
              "h": (0, 4), "c": (-2, 2), "gamma": (-2, 2), "rho": (0, .9)}
    if not isinstance(scenario["parameters"], dict) or set(scenario["parameters"]) != set(bounds):
        raise ValueError("Use the seven declared scenario parameters; arbitrary equations are not evaluated.")
    for name, (lo, hi) in bounds.items():
        number(scenario["parameters"][name], lo, hi, name)
    expected = "structural_effect" if kind == "confounding" else "regression_coefficient"
    if value["estimand"] != expected and not (kind!="confounding" and value["estimand"]=="regression_intercept"):
        raise ValueError("The selected estimand does not match this template's declared target.")
    integer(value["replications"], 2, 500, "Replications")
    integer(value["seed"], 0, 2**32 - 1, "Seed")
    integer(value["max_fits"], 1, 1000, "Fit allowance")
    integer(value["seconds"], 1, 300, "Execution seconds")
    methods = value["methods"]
    if not isinstance(methods, list) or not 1 <= len(methods) <= 4:
        raise ValueError("Choose one to four method variants.")
    ids = set()
    for m in methods:
        if not isinstance(m, dict) or set(m) != set(MethodSpec.__dataclass_fields__):
            raise ValueError("MethodSpec has missing or unrecognized fields.")
        if not isinstance(m["id"], str) or not m["id"] or len(m["id"]) > 80 or m["id"] in ids:
            raise ValueError("Methods require distinct bounded identities.")
        ids.add(m["id"])
        if not isinstance(m["label"], str) or not 1 <= len(m["label"]) <= 160 or type(m["oracle"]) is not bool:
            raise ValueError("Invalid method label or oracle declaration.")
        if m["oracle"] and kind != "confounding":
            raise ValueError("The hidden-confounder oracle exists only in the confounding simulator.")
        if m["model"] not in ("ols", "ridge", "forest") or m["operation"] not in ("inference", "predict"):
            raise ValueError("This study supports OLS inference and OLS/ridge/forest prediction.")
        if m["operation"] == "inference":
            if m["model"] != "ols" or m["covariance"] not in ("classical", "HC3", "cluster"):
                raise ValueError("Ordinary interval coverage is available only for supported OLS covariance rules.")
            if m["covariance"] == "cluster" and kind != "clustering":
                raise ValueError("Cluster covariance requires the clustering template.")
        elif m["covariance"] != "none":
            raise ValueError("Predictive methods do not inherit coefficient intervals.")
    cost = value["replications"] * sum(2 if m["operation"] == "inference" else 1 for m in methods)
    if cost > value["max_fits"]:
        raise ValueError(f"This design requires {cost} backend fits, exceeding {value['max_fits']}. No fits were started.")
    return StudySpec(**{**value, "scenario": ScenarioSpec(**scenario),
                        "methods": tuple(MethodSpec(**m) for m in methods)})


def seed_for(spec, replicate, stream):
    # Addressed streams do not depend on execution order or method inclusion.
    words = np.frombuffer(hashlib.sha256(packed([spec.seed, replicate, stream])).digest(), dtype="<u4")
    return np.random.Generator(np.random.PCG64(np.random.SeedSequence(words.tolist())))


def target(spec):
    if spec.estimand=="regression_intercept":return 1.,"Intercept"
    return (float(spec.scenario.parameters["tau"]),"d") if spec.scenario.kind=="confounding" else (float(spec.scenario.parameters["beta"]),"x")


def dataset(spec, replicate):
    s, p = spec.scenario, spec.scenario.parameters
    rng = seed_for(spec, replicate, "training")
    x, u, noise, eps = rng.normal(size=(4, s.n))
    if s.kind == "confounding":
        d = x + p["c"] * u + noise
        y = p["tau"] * d + x + p["gamma"] * u + p["sigma"] * eps
        columns, values, truth, term = ["d", "x", "u", "y"], np.column_stack([d, x, u, y]), p["tau"], "d"
    elif s.kind == "clustering":
        b = rng.normal(size=s.groups)
        group = np.repeat(np.arange(s.groups), s.group_size)
        y = 1 + p["beta"] * x + p["sigma"] * (np.sqrt(p["rho"]) * b[group] + np.sqrt(1-p["rho"]) * eps)
        columns, values, truth, term = ["x", "group", "y"], np.column_stack([x, group, y]), p["beta"], "x"
    else:
        y = 1 + p["beta"] * x + p["sigma"] * np.sqrt(1+p["h"]*x*x) * eps
        columns, values, truth, term = ["x", "y"], np.column_stack([x, y]), p["beta"], "x"
    rows = values.tolist()
    if s.kind == "clustering":
        for row in rows:
            row[1] = f"g{int(row[1])}"
    truth,term=target(spec)
    return {"name": f"Synthetic {s.kind}, replication {replicate+1}", "columns": columns, "rows": rows}, truth, term


def test_samples(spec, replicate, training):
    s, p, size = spec.scenario, spec.scenario.parameters, 1000
    rng = seed_for(spec, replicate, "evaluation")
    x, u, nu, eps = rng.normal(size=(4, size))
    if s.kind == "confounding":
        d = x+p["c"]*u+nu
        return {"fresh_observations": pd.DataFrame({"d":d,"x":x,"u":u,"y":p["tau"]*d+x+p["gamma"]*u+p["sigma"]*eps})}
    if s.kind != "clustering":
        return {"fresh_observations":pd.DataFrame({"x":x,"y":1+p["beta"]*x+p["sigma"]*np.sqrt(1+p["h"]*x*x)*eps})}
    # Recover the original random intercepts by replaying the addressed stream.
    tr = seed_for(spec, replicate, "training"); tr.normal(size=(4,s.n)); b=tr.normal(size=s.groups)
    group=rng.integers(0,s.groups,size=size); fresh_b=rng.normal(size=s.groups)
    common=1+p["beta"]*x; scale=p["sigma"]
    return {"existing_groups":pd.DataFrame({"x":x,"group":[f"g{i}" for i in group],"y":common+scale*(np.sqrt(p["rho"])*b[group]+np.sqrt(1-p["rho"])*eps)}),
            "new_groups":pd.DataFrame({"x":x,"group":[f"new{i}" for i in group],"y":common+scale*(np.sqrt(p["rho"])*fresh_b[group]+np.sqrt(1-p["rho"])*eps)})}


def analysis_request(spec, method, replicate, data):
    kind=spec.scenario.kind
    features=["d","x"] if kind=="confounding" else ["x"]
    if method.oracle: features.append("u")
    if kind=="clustering" and method.operation=="predict": features.append("group")
    return {"schema_version":1,"request_id":f"rep-{replicate}-{method.id}",
            "operation":"inference" if method.operation=="inference" else "fit", "model":method.model,
            "seed":int(seed_for(spec,replicate,"estimator").integers(0,2**32-1)),"dataset":data,
            "roles":{"target":"y","features":features,"group":"group" if kind=="clustering" else None,
                     "categorical":["group"] if "group" in features else []},
            "config":{"standardize":True,"degree":1,"alpha":.1,"trees":80,"depth":3,"min_leaf":3,"mtry":1},
            "inference":{"covariance":method.covariance,"alpha":.05},"max_fits":1000}


@contextmanager
def checkpoint_lock(path):
    path.parent.mkdir(parents=True,exist_ok=True)
    with path.with_suffix(".lock").open("a+b") as handle:
        handle.seek(0,2)
        if handle.tell()==0:handle.write(b"0");handle.flush()
        handle.seek(0)
        try:
            if os.name=="nt":
                import msvcrt
                msvcrt.locking(handle.fileno(),msvcrt.LK_NBLCK,1)
            else:
                import fcntl
                fcntl.flock(handle.fileno(),fcntl.LOCK_EX|fcntl.LOCK_NB)
        except OSError as e:
            raise ValueError("This study is already owned by a live worker.") from e
        try:yield
        finally:
            if os.name=="nt":
                handle.seek(0);msvcrt.locking(handle.fileno(),msvcrt.LK_UNLCK,1)
            else:fcntl.flock(handle.fileno(),fcntl.LOCK_UN)


def write_document(path, doc, progress):
    doc["integrity_sha256"]=digest({k:v for k,v in doc.items() if k!="integrity_sha256"})
    document_wire=json.dumps(doc,indent=2,allow_nan=False)
    wire=json.dumps({'schema_version':2,'document_wire':document_wire,'document_sha256':hashlib.sha256(document_wire.encode()).hexdigest()},allow_nan=False).encode()
    temporary=path.with_suffix(".partial")
    with temporary.open("wb") as f:f.write(wire);f.flush();os.fsync(f.fileno())
    os.replace(temporary,path)
    progress({"kind":"checkpoint","checkpoint_sha256":hashlib.sha256(wire).hexdigest()})


def read_document(path):
    if path.stat().st_size>64*1024*1024:raise ValueError("Study checkpoint exceeds 64 MiB.")
    envelope=json.loads(path.read_bytes())
    document_wire=envelope.get('document_wire')
    if not isinstance(document_wire,str) or envelope.get('document_sha256')!=hashlib.sha256(document_wire.encode()).hexdigest():raise ValueError('Study checkpoint integrity validation failed.')
    doc=json.loads(document_wire)
    if doc.get("schema_version")!=2 or doc.get("integrity_sha256")!=digest({k:v for k,v in doc.items() if k!="integrity_sha256"}):
        raise ValueError("Study checkpoint integrity or schema validation failed.")
    if digest(json.loads(doc["spec_wire"]))!=digest(doc["spec"]) or hashlib.sha256(doc["spec_wire"].encode()).hexdigest()!=doc["spec_sha256"]:
        raise ValueError("Study specification fingerprint mismatch.")
    validate_spec(doc["spec"])
    return doc


def checkpoint_path(request):
    root=Path(request.get("project_root","")).expanduser()
    path=Path(request.get("checkpoint_path","")).expanduser()
    if not root.is_absolute() or not path.is_absolute() or not root.is_dir() or not path.resolve().is_relative_to(root.resolve()):
        raise ValueError("Choose an existing project folder and a checkpoint inside it.")
    if path.suffix!=".json" or not path.parent.name.endswith(".studies") or path.name!=request["study_id"]+".json":
        raise ValueError("Checkpoint must be the named study's artifact inside a project .studies folder.")
    if len(request["study_id"])>120 or not all(c.isascii() and (c.isalnum()or c in "-_") for c in request["study_id"]):
        raise ValueError("Invalid study identity.")
    return path


def mean_mc(values):
    values=np.asarray(values,float)
    if not len(values):return {"value":None,"mc_se":None,"n":0}
    return {"value":float(values.mean()),"mc_se":float(values.std(ddof=1)/np.sqrt(len(values))) if len(values)>1 else None,"n":len(values)}


def summarize(spec, records):
    summary={"completed_replications":sum(len(r["methods"])==len(spec.methods) for r in records),"attempted_replications":len(records),"requested_replications":spec.replications,"methods":[],"paired":[]}
    for method in spec.methods:
        rows=[m for r in records for m in r["methods"] if m["method_id"]==method.id]
        valid=[r for r in rows if r["status"]=="completed"]
        estimates=[r["estimate"] for r in valid if r["estimate"] is not None]
        errors=[r["estimate"]-r["truth"] for r in valid if r["estimate"] is not None]
        intervals=[r for r in valid if r["lower"] is not None and r["upper"] is not None]
        cover=[float(r["lower"]<=r["truth"]<=r["upper"]) for r in intervals]
        item={"method_id":method.id,"label":method.label,"attempted":len(rows),"completed":len(valid),"failed":len(rows)-len(valid),
              "failure_rate":mean_mc([float(r["status"]!="completed")for r in rows]),
              "bias":mean_mc(errors),"mse":mean_mc(np.square(errors)),
              "empirical_sd":float(np.std(estimates,ddof=1)) if len(estimates)>1 else None,
              "coverage_valid_intervals":mean_mc(cover),"valid_and_covering":mean_mc([float(r["status"]=="completed" and r["lower"] is not None and r["lower"]<=r["truth"]<=r["upper"])for r in rows]) if method.operation=="inference" else mean_mc([]),
              "interval_width":mean_mc([r["upper"]-r["lower"]for r in intervals]),
              "prediction":{}}
        keys=sorted({k for r in valid for k in r["prediction_mse"]})
        item["prediction"]={k:mean_mc([r["prediction_mse"][k]for r in valid if k in r["prediction_mse"]])for k in keys}
        if len(estimates)>3:
            centered=np.asarray(estimates)-np.mean(estimates);variance=np.mean(centered**2)
            item["empirical_sd_mc_se"]=float(np.sqrt(max(0.,np.mean(centered**4)-variance**2)/(4*len(estimates)*variance))) if variance>0 else 0.
        else:item["empirical_sd_mc_se"]=None
        item["empirical_sd_mc_se_scope"]="Fourth-moment delta approximation; descriptive at small replication counts."
        summary["methods"].append(item)
    for i,a in enumerate(spec.methods):
        for b in spec.methods[i+1:]:
            paired=[];predicted={}
            for record in records:
                ma=next((m for m in record["methods"]if m["method_id"]==a.id),None)
                mb=next((m for m in record["methods"]if m["method_id"]==b.id),None)
                if ma and mb and ma["status"]==mb["status"]=="completed" and ma["estimate"] is not None and mb["estimate"] is not None:
                    paired.append(ma["estimate"]-mb["estimate"])
                if ma and mb and ma["status"]==mb["status"]=="completed":
                    for key in ma["prediction_mse"].keys() & mb["prediction_mse"].keys():predicted.setdefault(key,[]).append(ma["prediction_mse"][key]-mb["prediction_mse"][key])
            summary["paired"].append({"method_a":a.id,"method_b":b.id,"estimate_difference":mean_mc(paired),"prediction_difference":{k:mean_mc(v)for k,v in predicted.items()},"scope":"Paired complete replications only; information sets may differ for simulator-only oracles."})
    return summary


def run_method(spec, method, replicate, data, charge):
    class MeteredJob(science.Job):
        def pipeline(self,config=None):
            pipe=super().pipeline(config);original=pipe.named_steps["model"].fit
            def fitted(*args,**kw):charge();return original(*args,**kw)
            pipe.named_steps["model"].fit=fitted
            return pipe
        def fit(self,rows,config=None):
            self.last_pipe=super().fit(rows,config);return self.last_pipe
    original_ols=science.OLS
    class MeteredOLS(original_ols):
        def fit(self,*args,**kw):charge();return super().fit(*args,**kw)
    request=analysis_request(spec,method,replicate,data);job=MeteredJob(request)
    science.OLS=MeteredOLS
    try:
        if method.operation=="inference":result=job.inference()
        else:result=job.fitted(job.fit(job.rows),job.rows)
    finally:science.OLS=original_ols
    table=result.get("coefficient_table",[]);truth,term=target(spec)
    row=next((r for r in table if r["term"]==term),None)
    prediction={}
    with warnings.catch_warnings(record=True) as caught:
        warnings.simplefilter("always")
        for name,frame in test_samples(spec,replicate,data).items():
            predicted=job.last_pipe.predict(frame)
            score=float(np.mean((frame["y"].to_numpy()-predicted)**2))
            if not math.isfinite(score):raise ValueError("Nonfinite predictive risk.")
            prediction[name]=score
    compact={"method_id":method.id,"status":"completed","estimate":row["estimate"]if row else None,
             "std_error":row["std_error"]if row else None,"lower":row["lower"]if row else None,"upper":row["upper"]if row else None,
             "truth":float(truth),"prediction_mse":prediction,"error":None,
             "warnings":list(dict.fromkeys([*job.notices,*[str(w.message)for w in caught]]))}
    return compact,result,request


def run_study(request,spec,progress):
    path=checkpoint_path(request);wire=request["spec_wire"];fingerprint=hashlib.sha256(wire.encode()).hexdigest()
    started=time.monotonic()
    with checkpoint_lock(path):
        if path.exists():
            doc=read_document(path)
            if not request.get("resume"):raise ValueError("This study exists. Resume it explicitly or start a new study.")
            if doc["spec_sha256"]!=fingerprint or doc["study_id"]!=request["study_id"] or doc["provenance"]!=provenance():
                raise ValueError("Resume requires the identical specification, sources, Python and package versions. Fork a new study instead.")
            for reservation in doc["reservations"]:
                if reservation["status"]=="running":reservation["status"]="uncertain"
        else:
            if request.get("resume"):raise ValueError("No checkpoint exists to resume.")
            doc={"schema_version":2,"study_id":request["study_id"],"spec":asdict(spec),"spec_wire":wire,"spec_sha256":fingerprint,
                 "provenance":provenance(),"status":"running","records":[],"reservations":[],"summary":summarize(spec,[])}
        doc["status"]="running";doc.pop("stop_reason",None);write_document(path,doc,progress)
        try:
            for replicate in range(spec.replications):
                if time.monotonic()-started>spec.seconds:raise TimeoutError("Study reached its execution deadline.")
                data,truth,term=dataset(spec,replicate)
                record=next((r for r in doc["records"]if r["replicate"]==replicate),None)
                if record is None:
                    record={"replicate":replicate,"data_sha256":digest(data),"methods":[]};doc["records"].append(record)
                elif record["data_sha256"]!=digest(data):raise ValueError("Regenerated dataset fingerprint differs from the checkpoint.")
                for method in spec.methods:
                    if any(m["method_id"]==method.id for m in record["methods"]):continue
                    slots=2 if method.operation=="inference" else 1
                    spent=sum(r["slots"] if r["status"] in ("running","uncertain")else r["actual_fits"]for r in doc["reservations"])
                    if spent+slots>spec.max_fits:raise RuntimeError("Fit allowance exhausted, including interrupted reservations. Completed results were retained.")
                    reservation={"replicate":replicate,"method_id":method.id,"purpose":"study","slots":slots,"actual_fits":0,"status":"running"}
                    doc["reservations"].append(reservation);write_document(path,doc,progress)
                    def charge():
                        if time.monotonic()-started>spec.seconds:raise TimeoutError("Study reached its execution deadline.")
                        if reservation["actual_fits"]>=reservation["slots"]:raise RuntimeError("Numerical backend exceeded its preflight fit reservation.")
                        reservation["actual_fits"]+=1
                    try:
                        result,_,_=run_method(spec,method,replicate,data,charge)
                        if time.monotonic()-started>spec.seconds:raise TimeoutError("Study reached its execution deadline.")
                        reservation["status"]="completed"
                    except TimeoutError:raise
                    except Exception as error:
                        result={"method_id":method.id,"status":"failed","estimate":None,"std_error":None,"lower":None,"upper":None,"truth":truth,"prediction_mse":{},"error":f"{type(error).__name__}: {error}","warnings":[]}
                        reservation["status"]="failed"
                    record["methods"].append(result)
                    doc["summary"]=summarize(spec,doc["records"]);write_document(path,doc,progress)
            doc["status"]="completed"
        except (TimeoutError,RuntimeError) as error:
            doc["status"]="partial";doc["stop_reason"]=str(error)
            for r in doc["reservations"]:
                if r["status"]=="running":r["status"]="uncertain"
        doc["summary"]=summarize(spec,doc["records"]);write_document(path,doc,progress)
        return doc


def inspect_study(request,spec,progress):
    path=checkpoint_path(request)
    with checkpoint_lock(path):
        doc=read_document(path)
        if doc["spec_sha256"]!=hashlib.sha256(request["spec_wire"].encode()).hexdigest() or doc["provenance"]!=provenance():raise ValueError("Inspection requires the recorded source, specification and versions.")
        replicate=integer(request.get("replicate"),0,spec.replications-1,"Replication")
        record=next((r for r in doc["records"]if r["replicate"]==replicate),None)
        if record is None:raise ValueError("This replication has not been attempted.")
        data,truth,term=dataset(spec,replicate)
        if digest(data)!=record["data_sha256"]:raise ValueError("Regenerated observation identity mismatch.")
        method=next((m for m in spec.methods if m.id==request.get("method_id")),None)
        saved=next((m for m in record["methods"]if m["method_id"]==request.get("method_id")),None)
        if method is None or saved is None or saved["status"]!="completed":raise ValueError("Select a completed method result to inspect.")
        slots=2 if method.operation=="inference"else 1
        spent=sum(r["slots"]if r["status"]in("running","uncertain")else r["actual_fits"]for r in doc["reservations"])
        if spent+slots>spec.max_fits:raise ValueError("Inspection would exceed the recorded fit allowance.")
        reservation={"replicate":replicate,"method_id":method.id,"purpose":"inspection","slots":slots,"actual_fits":0,"status":"running"};doc["reservations"].append(reservation);write_document(path,doc,progress)
        def charge():
            if reservation["actual_fits"]>=slots:raise ValueError("Inspection exceeded its reservation.")
            reservation["actual_fits"]+=1
        try:
            actual,result,analysis=run_method(spec,method,replicate,data,charge)
            if not science.numerical_match(actual,saved):raise ValueError("Regenerated numerical result differs from the recorded replication.")
            reservation["status"]="completed"
        except Exception:
            reservation["status"]="failed";write_document(path,doc,progress);raise
        write_document(path,doc,progress)
        return {"replicate":replicate,"method_id":method.id,"data_sha256":record["data_sha256"],"verified":True,"analysis_request":analysis,"analysis_result":result,"observed_data":observed_data(data),"simulator_truth":truth,"truth_scope":"Known only because this dataset was generated by the declared simulator.","optimizer_trace":None}


def observed_data(data):
    indices=[i for i,c in enumerate(data["columns"])if c!="u"]
    return {"name":data["name"],"columns":[data["columns"][i]for i in indices],"rows":[[row[i]for i in indices]for row in data["rows"]]}


LESSONS={
 "heteroskedasticity":("Variance assumptions and interval coverage","Predict whether changing conditional error variance will change the target coefficient or the reliability of each interval.","HC3 changes the covariance calculation, not the OLS point estimate. Compare coverage with its Monte Carlo uncertainty; finite samples need not attain exactly 95%.","Set h to zero, then increase sample size. Explain which differences should diminish and which comparisons remain uncertain."),
 "confounding":("Prediction, association and intervention","Predict whether more observations or robust standard errors will remove bias for the simulator's treatment effect.","The target tau is supplied by the simulator. Observed regression need not identify it when U affects both treatment and outcome. The oracle uses otherwise unavailable information; robust covariance does not repair this identification failure.","Set c or gamma to zero. Explain the role of conditioning on X, and why this experiment does not validate exogeneity in an imported dataset."),
 "clustering":("Dependence and the generalization population","Predict how within-group error dependence changes interval coverage and the meaning of predictive validation.","Dependence affects targets differently. Here X is independent across observations, so group errors can affect the intercept much more strongly than the slope. Inspect both targets. Existing-group and new-group predictive errors answer different questions. Few clusters can make conventional inference unreliable.","Set rho to zero, then change group count while inspecting failures and Monte Carlo uncertainty. Explain why random and group-separated validation can target different populations.")}


def export_study(request,spec):
    path=checkpoint_path(request);doc=read_document(path)
    if doc["spec_sha256"]!=hashlib.sha256(request["spec_wire"].encode()).hexdigest():raise ValueError("Export specification does not match the selected study.")
    if doc["provenance"]!=provenance():raise ValueError("Export requires the recorded source and package versions.")
    audience=request.get("audience")
    if audience not in ("student","instructor","research"):raise ValueError("Choose a student, instructor or research export.")
    destination=Path(request.get("export_path","")).expanduser()
    if not destination.is_absolute()or destination.exists():raise ValueError("Choose a new absolute export folder; existing work is preserved.")
    title,prompt,answer,transfer=LESSONS[spec.scenario.kind]
    destination.mkdir(parents=True)
    try:
        if audience=="student":
            data,_,_=dataset(spec,integer(request.get("replicate",0),0,spec.replications-1,"Replication"));observed=observed_data(data)
            with (destination/'observed-data.csv').open('w',newline='',encoding='utf-8')as f:w=csv.writer(f);w.writerow(observed['columns']);w.writerows(observed['rows'])
            (destination/'worksheet.txt').write_text(title+'\n\nPrediction\n'+prompt+'\n\nYour reasoning:\n\n\nTransfer\n'+transfer+'\n\nYour reasoning:\n',encoding='utf-8')
            (destination/'student-worksheet.tex').write_text('\n'.join([r'\documentclass[11pt]{article}',r'\usepackage[margin=1in]{geometry}',r'\begin{document}',r'\section*{'+title+'}',r'\paragraph{Prediction}',prompt,r'\vspace{4cm}',r'\paragraph{Transfer}',transfer,r'\vspace{4cm}',r'\end{document}'])+'\n',encoding='utf-8')
            # This packet intentionally contains no generator recipe, seed, U,
            # oracle coefficients, ground truth, or instructor explanations.
            (destination/'README.txt').write_text('Student observation packet. Open observed-data.csv in Model Lab. The instructor retains the generating mechanism and answer key separately. No claim of secure examination or anti-cheating protection is made.\n',encoding='utf-8')
        else:
            if doc["provenance"]!=provenance():raise ValueError("Executable export requires the recorded worker and package versions.")
            for name in source_manifest():shutil.copy2(Path(__file__).parent/name,destination/name)
            (destination/'study.json').write_text(json.dumps(doc,indent=2,allow_nan=False),encoding='utf-8')
            (destination/'study-spec.json').write_text(doc['spec_wire'],encoding='utf-8')
            (destination/'instructor-key.txt').write_text(title+'\n\n'+answer+'\n\nTransfer: '+transfer+'\n\nAll claims concern the declared simulation, not identification in an observed research dataset.\n',encoding='utf-8')
            fields=['replicate','method_id','status','estimate','std_error','lower','upper','truth','error']
            with (destination/'replications.csv').open('w',newline='',encoding='utf-8')as f:
                w=csv.DictWriter(f,fieldnames=fields);w.writeheader()
                for r in doc['records']:
                    for m in r['methods']:w.writerow({'replicate':r['replicate'],**{k:m.get(k)for k in fields if k!='replicate'}})
            (destination/'requirements.txt').write_text(''.join(f'{k}=={v}\n'for k,v in doc['provenance']['packages'].items()),encoding='utf-8')
            (destination/'reproduce.py').write_text(REPRODUCE,encoding='utf-8')
            export_figure(spec,doc,destination)
            def escaped(text):return ''.join({'\\':r'\textbackslash{}','_':r'\_','&':r'\&','%':r'\%','#':r'\#','$':r'\$','{':r'\{','}':r'\}'}.get(c,c)for c in str(text))
            text=[r'\documentclass[11pt]{article}',r'\usepackage[margin=1in]{geometry}',r'\usepackage{graphicx}',r'\begin{document}',r'\section*{'+escaped(title)+'}',escaped(spec.aim),r'\par',escaped('Estimand: '+spec.estimand+'. Synthetic truth is known only under this simulator.'),r'\par',escaped('Generating template: '+spec.scenario.kind+' version 1. Parameters: '+json.dumps(spec.scenario.parameters,sort_keys=True)),r'\par',escaped(answer),r'\par',escaped('Summary: '+json.dumps(doc['summary'],sort_keys=True)),r'\par',r'\includegraphics[width=\linewidth]{figure.pdf}',r'\end{document}']
            # Full machine-readable summaries are in study.json; short prose
            # avoids placing a large unbreakable JSON table into the report.
            text=[t for t in text if not t.startswith('Summary:')]
            (destination/'report.tex').write_text('\n'.join(text)+'\n',encoding='utf-8')
            (destination/'README.txt').write_text('Research/instructor bundle: includes known simulation truth, generating recipe, seeds and answers. Do not distribute as a student packet. Run reproduce.py with the pinned scientific Python packages; the script installs nothing. It checks source integrity and reproduces completed replication results, including failures. Interrupted attempts are retained in the original study ledger, not erased by reproduction. Monte Carlo results describe this design and do not establish universal validity or causal identification.\n',encoding='utf-8')
            if audience=='instructor':export_study({**request,'audience':'student','export_path':str(destination/'student-packet')},spec)
        hashes={p.relative_to(destination).as_posix():hashlib.sha256(p.read_bytes()).hexdigest()for p in destination.rglob('*')if p.is_file()}
        (destination/'manifest.json').write_text(json.dumps({'schema_version':2,'audience':audience,'files':hashes},indent=2)+'\n',encoding='utf-8')
        return {'destination':str(destination),'audience':audience,'files':[*hashes,'manifest.json']}
    except Exception:shutil.rmtree(destination);raise


def export_figure(spec,doc,destination):
    import matplotlib
    matplotlib.use('Agg')
    import matplotlib.pyplot as plt
    with plt.rc_context({'font.family':'DejaVu Serif','font.size':10,'axes.spines.top':False,'axes.spines.right':False}):
        fig,ax=plt.subplots(figsize=(7,4.5),layout='constrained')
        if any(m['estimate'] is not None for r in doc['records']for m in r['methods']):
            for method in spec.methods:
                values=[m['estimate']for r in doc['records']for m in r['methods']if m['method_id']==method.id and m['status']=='completed'and m['estimate']is not None]
                if values:ax.hist(values,bins=min(20,max(3,len(values)//5)),histtype='step',label=method.label)
            ax.axvline(target(spec)[0],color='black',ls='--',label='Simulator target')
            ax.set_xlabel('Coefficient estimate (synthetic units)');ax.set_ylabel('Replications');ax.set_title('Sampling distributions; failed estimates remain in study.json')
        else:
            index=0;labels=[]
            for method in spec.methods:
                rows=[m for r in doc['records']for m in r['methods']if m['method_id']==method.id and m['status']=='completed']
                for regime in sorted({k for m in rows for k in m['prediction_mse']}):
                    values=[m['prediction_mse'][regime]for m in rows if regime in m['prediction_mse']]
                    if not values:continue
                    metric=mean_mc(values);ax.scatter(index+np.linspace(-.1,.1,len(values)),values,s=8,color='black',alpha=.5)
                    ax.errorbar(index,metric['value'],yerr=metric['mc_se']or 0,fmt='o',color='black',capsize=4)
                    labels.append(method.label+' / '+regime);index+=1
            ax.set_xticks(range(index),labels,rotation=25,ha='right');ax.set_ylabel('Independent evaluation-sample MSE');ax.set_title('Prediction risk: replications and mean ± Monte Carlo SE')
        handles,labels=ax.get_legend_handles_labels()
        if handles:ax.legend(frameon=False,fontsize=8)
        fig.savefig(destination/'figure.pdf');fig.savefig(destination/'figure.svg');plt.close(fig)


REPRODUCE='''"""Reproduce completed simulation results; installs nothing and verifies sources."""
from pathlib import Path
import hashlib,json,sys,tempfile
root=Path(__file__).resolve().parent
expected=json.loads((root/'study.json').read_bytes())
for name,sha in expected['provenance']['sources'].items():
    if name not in ('study_worker.py','research_worker.py') or hashlib.sha256((root/name).read_bytes()).hexdigest()!=sha:raise SystemExit('Worker source integrity failed.')
sys.path.insert(0,str(root))
import study_worker as worker
spec=worker.validate_spec(expected['spec'])
wire=expected['spec_wire']
if hashlib.sha256(wire.encode()).hexdigest()!=expected['spec_sha256'] or json.loads(wire)!=expected['spec']:raise SystemExit('Specification integrity failed.')
drift={k:[v,worker.provenance()['packages'].get(k)]for k,v in expected['provenance']['packages'].items()if worker.provenance()['packages'].get(k)!=v}
with tempfile.TemporaryDirectory()as folder:
    request={'schema_version':2,'request_id':'reproduce','operation':'simulate','study_id':'reproduction','study_spec':expected['spec'],'spec_wire':wire,'project_root':folder,'checkpoint_path':str(Path(folder)/'reproduction.studies/reproduction.json')}
    actual=worker.execute(request)['result']
    # A partial study is reproduced by checking its recorded completed outcomes;
    # the rerun may finish more replications. Original failures are compared too.
    actual_by_id={r['replicate']:r for r in actual['records']}
    for record in expected['records']:
        target=actual_by_id.get(record['replicate'])
        if target is None or target['data_sha256']!=record['data_sha256']:raise SystemExit('Observation regeneration failed.')
        for method in record['methods']:
            found=next((m for m in target['methods']if m['method_id']==method['method_id']),None)
            if not worker.science.numerical_match(found,method):raise SystemExit('Numerical reproduction failed. Version drift: '+json.dumps(drift))
report={'status':'passed','source_verified':True,'spec_verified':True,'tolerance':1e-7,'package_version_drift':drift,'python_version_drift':[expected['provenance']['python'],worker.provenance()['python']]if expected['provenance']['python']!=worker.provenance()['python']else None}
(root/'reproduction-verification.json').write_text(json.dumps(report,indent=2)+'\\n')
print(json.dumps(report))
'''


def execute(request,progress=None):
    if request.get('schema_version')!=2 or not isinstance(request.get('request_id'),str)or not request['request_id']:raise ValueError('Unsupported study protocol or missing identity.')
    progress=progress or (lambda event:None)
    def emit(event):progress({'schema_version':2,'request_id':request['request_id'],**event})
    spec=validate_spec(request.get('study_spec'))
    wire=request.get('spec_wire','')
    if not isinstance(wire,str)or json.loads(wire)!=request['study_spec']:raise ValueError('Study specification does not match its exact wire representation.')
    operation=request.get('operation')
    if operation=='study_preflight':result={'planned_backend_fits':spec.replications*sum(2 if m.operation=='inference'else 1 for m in spec.methods),'fit_allowance':spec.max_fits,'sources':source_manifest(),'optimizer_trace':None}
    elif operation=='study_preview':
        data,truth,term=dataset(spec,0);result={'observed_data':observed_data(data),'simulator_truth':truth,'target_term':term,'truth_scope':'Known only under this synthetic generating mechanism.','data_sha256':digest(data),'latent_columns':['u']if spec.scenario.kind=='confounding'else[]}
    elif operation=='simulate':result=run_study(request,spec,emit)
    elif operation=='study_inspect':result=inspect_study(request,spec,emit)
    elif operation=='study_export':result=export_study(request,spec)
    else:raise ValueError('Unsupported study operation.')
    return {'schema_version':2,'request_id':request['request_id'],'operation':operation,'status':'completed','worker_sha256':source_manifest()['study_worker.py'],'worker_bundle_sha256':digest(source_manifest()),'sources':source_manifest(),'engine_version':'0.8.0','result':result}


def main():
    parser=argparse.ArgumentParser();parser.add_argument('--request',required=True);parser.add_argument('--response',required=True);args=parser.parse_args()
    path=Path(args.request)
    if path.stat().st_size>32*1024*1024:raise ValueError('Request exceeds 32 MiB.')
    wire=path.read_bytes();request=json.loads(wire)
    try:
        def progress(event):print(json.dumps(event,allow_nan=False),flush=True)
        response=execute(request,progress)
    except Exception as e:response={'schema_version':2,'request_id':request.get('request_id'),'operation':request.get('operation'),'status':'failed','error':f'{type(e).__name__}: {e}','worker_sha256':source_manifest()['study_worker.py'],'worker_bundle_sha256':digest(source_manifest())}
    response['request_sha256']=hashlib.sha256(wire).hexdigest()
    output=Path(args.response);temporary=output.with_suffix('.partial');temporary.write_text(json.dumps(response,allow_nan=False,indent=2),encoding='utf-8');os.replace(temporary,output)
    return 0 if response['status']=='completed'else 1


if __name__=='__main__':raise SystemExit(main())
