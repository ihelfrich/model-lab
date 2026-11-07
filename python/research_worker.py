"""Model Lab scientific adapter, protocol 1. Local, explicit, reproducible jobs.

The desktop writes a bounded request file. This worker writes an atomic response;
stdout contains progress events only. The same module runs exported research jobs.
"""
from __future__ import annotations

import argparse
import hashlib
import importlib.metadata
import json
import math
import os
from pathlib import Path
import sys
import time
import warnings

import numpy as np
import pandas as pd
from scipy import stats
from scipy.optimize import linprog
from sklearn.base import BaseEstimator, TransformerMixin
from sklearn.compose import ColumnTransformer
from sklearn.ensemble import GradientBoostingRegressor, RandomForestRegressor
from sklearn.impute import SimpleImputer
from sklearn.linear_model import ElasticNet, Lasso, LinearRegression, LogisticRegression, Ridge
from sklearn.model_selection import GroupKFold, GroupShuffleSplit, KFold, StratifiedKFold, train_test_split
from sklearn.pipeline import Pipeline
from sklearn.preprocessing import OneHotEncoder, PolynomialFeatures, StandardScaler
from sklearn.tree import DecisionTreeRegressor
from statsmodels.regression.linear_model import OLS
from statsmodels.discrete.discrete_model import Logit

SCHEMA = 1
ENGINE = "0.7.0"
METHODS = ("ols", "logistic", "ridge", "lasso", "elastic_net", "tree", "boosting", "stochastic", "forest")
OLS_COV = ("classical", "HC0", "HC1", "HC2", "HC3", "cluster", "HAC")
LOGIT_COV = ("classical", "HC0", "cluster")


def split_formula(text, separator):
    parts=[];start=0;quoted=False;i=0
    while i<len(text):
        if text[i]=='`':
            if quoted and i+1<len(text)and text[i+1]=='`':i+=2;continue
            quoted=not quoted
        elif text[i]==separator and not quoted:parts.append(text[start:i].strip());start=i+1
        i+=1
    if quoted:raise ValueError('Unclosed backtick in formula.')
    return parts+[text[start:].strip()]


def parse_formula(text, target, allowed):
    """A structural grammar only: intercept, names, C(name), x:y and x^2.

    Nothing in this grammar is evaluated as Python or a dataframe expression.
    Backticks quote column names, with doubled backticks escaping a backtick.
    """
    def name(token):
        token=token.strip()
        if token.startswith('`')and token.endswith('`'):token=token[1:-1].replace('``','`')
        if token not in allowed:raise ValueError(f'Unknown or unchecked formula predictor: {token}')
        return token
    sides=split_formula(text,'~')
    if len(sides)!=2:raise ValueError('Use response ~ predictor + predictor^2 + predictor:predictor.')
    lhs=sides[0].strip('`').replace('``','`')
    if lhs!=target:raise ValueError('The formula response must match the selected response role.')
    features=[];categories=[];terms=[]
    for term in split_formula(sides[1],'+'):
        if term=='1':continue
        if term in ('0','-1',''):raise ValueError('This formula implementation includes an intercept and requires at least one predictor term.')
        if term.startswith('C(')and term.endswith(')'):
            c=name(term[2:-1]);categories.append(c);factors={c:1}
        else:
            factors={}
            for factor in split_formula(term,':'):
                power=split_formula(factor,'^')
                if len(power)>2 or len(power)==2 and power[1]!='2':raise ValueError('Only numeric squares and pairwise interactions are supported.')
                c=name(power[0]);factors[c]=factors.get(c,0)+(2 if len(power)==2 else 1)
            if sum(factors.values())>2:raise ValueError('Formula numeric terms have maximum total degree two.')
        for c in factors:
            if c not in features:features.append(c)
        if factors not in terms:terms.append(factors)
    if not features or len(terms)>512:raise ValueError('Formula needs 1–512 supported terms.')
    return features,categories,terms


class MonomialTerms(TransformerMixin,BaseEstimator):
    """Compute requested terms directly; never allocate an unused full expansion."""
    def __init__(self,powers):self.powers=powers
    def fit(self,X,y=None):self.n_features_in_=X.shape[1];return self
    def transform(self,X):
        X=np.asarray(X);out=np.ones((len(X),len(self.powers)))
        for column,powers in enumerate(self.powers):
            for j,power in enumerate(powers):
                if power:out[:,column]*=X[:,j]**power
        return out
    def get_feature_names_out(self,input_features=None):
        names=list(input_features)if input_features is not None else [f'x{i}'for i in range(self.n_features_in_)]
        return np.array([' '.join(str(names[j])+(f'^{power}'if power!=1 else '')for j,power in enumerate(powers)if power)for powers in self.powers],dtype=object)


def digest(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(",", ":"), allow_nan=False).encode()).hexdigest()


def capabilities():
    return {"schema_version": SCHEMA, "backend": "scientific-python", "engine_version": ENGINE,
            "models": [{"id": m, "task": "binary" if m == "logistic" else "regression",
                        "fit": True, "resampling": True, "optimizer_trace": False,
                        "coefficients": m in METHODS[:5],
                        "inference": list(LOGIT_COV if m == "logistic" else OLS_COV if m == "ols" else ()),
                        "marginal_effects": m == "logistic"} for m in METHODS],
            "splitters": ["random", "stratified", "group", "temporal"],
            "formula": "Intercept, checked column names, C(name), numeric squares and pairwise interactions; no expression evaluation.",
            "limits": {"rows": 50000, "request_mib":32, "input_predictors": 64, "encoded_predictors": 512, "fits": 1000}}


class Job:
    def __init__(self, request, progress=None):
        self.request = request
        if request.get("schema_version") != SCHEMA:
            raise ValueError("Unsupported adapter protocol version.")
        if not isinstance(request.get("request_id"), str) or not request["request_id"]:
            raise ValueError("Missing request identity.")
        self.progress = progress or (lambda event: None)
        self.started = time.monotonic()
        self.fits = 0
        self.max_fits = int(request.get("max_fits", 500))
        if not 1 <= self.max_fits <= 1000:
            raise ValueError("The fit budget must be between 1 and 1,000.")
        self.notices = []
        self.data = request.get("dataset", {})
        if request.get("operation") == "capabilities":
            self.excluded = []
            return
        columns, records = self.data.get("columns", []), self.data.get("rows", [])
        if not 2 <= len(columns) <= 67 or len(set(columns)) != len(columns) or any(not isinstance(c, str) or not c for c in columns):
            raise ValueError("Use distinct nonempty column names, with at most 67 columns including roles.")
        if not 4 <= len(records) <= 50000 or any(not isinstance(r, list) or len(r) != len(columns) for r in records):
            raise ValueError("Use a rectangular dataset with 4–50,000 observations within the request byte limit.")
        self.roles = request.get("roles", {})
        self.target = self.roles.get("target")
        self.features = self.roles.get("features", [])
        if self.target not in columns or not self.features or len(set(self.features)) != len(self.features) or len(self.features) > 64:
            raise ValueError("Select one response and 1–64 distinct predictors.")
        if self.target in self.features or any(c not in columns for c in self.features):
            raise ValueError("The response cannot also be a predictor.")
        tokens=self.roles.get('missing_tokens',[])
        if not isinstance(tokens,list)or len(tokens)>32 or any(not isinstance(v,str)for v in tokens):raise ValueError('Missing-value tokens must be a list of at most 32 exact strings.')
        self.frame = pd.DataFrame(records, columns=columns).map(lambda v: np.nan if isinstance(v,str) and (not v.strip() or v in tokens) else v)
        formula=request.get('formula','').strip();self.formula_terms=None;forced=[]
        if formula:self.features,forced,self.formula_terms=parse_formula(formula,self.target,self.features)
        for role in ("group", "time"):
            if self.roles.get(role) is not None and self.roles[role] not in columns:
                raise ValueError(f"Unknown {role} column.")
            if self.roles.get(role) == self.target:
                raise ValueError("The outcome cannot determine group/time partition identities.")
        self.y = pd.to_numeric(self.frame[self.target], errors="raise").to_numpy(dtype=float)
        self.rows = np.arange(len(records), dtype=int)
        valid = np.isfinite(self.y)
        if not valid.all() and not self.roles.get("omit_missing_outcome", False):
            raise ValueError("Missing/nonfinite outcomes require explicit omission. No rows were silently removed.")
        self.excluded = self.rows[~valid].tolist()
        self.rows = self.rows[valid]
        if len(self.rows) < 4 or np.any(np.abs(self.y[self.rows]) > 1e12):
            raise ValueError("Too few complete outcomes or outcomes outside the supported numerical domain.")
        self.model = request.get("model", "ols")
        if self.model not in METHODS:
            raise ValueError("Unknown model adapter.")
        self.binary = self.model == "logistic"
        if self.binary and (not np.isin(self.y[self.rows], [0, 1]).all() or len(np.unique(self.y[self.rows])) != 2):
            raise ValueError("Binary logistic regression needs both 0 and 1 outcomes.")
        self.seed = int(request.get("seed", 4710))
        if not 0 <= self.seed < 2**32:
            raise ValueError("Seed must be an unsigned 32-bit integer.")
        self.numeric, self.categorical = [], []
        declared_categories = self.roles.get("categorical", [])+forced
        if any(c not in columns or c==self.target for c in declared_categories):
            raise ValueError("Categorical role declarations must refer to predictor columns.")
        for c in self.features:
            values = self.frame[c]
            numeric = pd.to_numeric(values, errors="coerce")
            if c not in declared_categories and numeric.notna().sum() == values.notna().sum():
                a = numeric.to_numpy(dtype=float)
                if np.any(np.isinf(a)) or np.any(np.abs(a[np.isfinite(a)]) > 1e12):
                    raise ValueError(f"Nonfinite or excessively large predictor values in {c}.")
                self.frame[c] = numeric
                self.numeric.append(c)
            else:
                # Strings preserve categorical identity; missing values remain missing.
                self.frame[c] = values.map(lambda v: str(v) if pd.notna(v) else np.nan)
                self.categorical.append(c)
        self.config = dict(request.get("config", {}))
        self.numeric_powers=None
        if self.formula_terms is not None:
            self.numeric_powers=[]
            for factors in self.formula_terms:
                if any(c in self.categorical for c in factors):
                    if len(factors)!=1 or sum(factors.values())!=1:raise ValueError('Categorical interactions are not implemented in formula protocol 1.')
                    continue
                self.numeric_powers.append([factors.get(c,0)for c in self.numeric])
        self.expanded=any(sum(p)>1 for p in self.numeric_powers)if self.numeric_powers is not None else int(self.config.get('degree',1))==2

    def tick(self, phase, **details):
        if time.monotonic() - self.started > 300:
            raise TimeoutError("Scientific job exceeded its 300-second execution budget.")
        self.progress({"schema_version": SCHEMA, "request_id": self.request["request_id"], "kind": "progress",
                       "phase": phase, "fits": self.fits, **details})

    def pipeline(self, config=None):
        c = {**self.config, **(config or {})}
        alpha, mix = float(c.get("alpha", .1)), float(c.get("mix", .5))
        degree = int(c.get("degree", 1))
        if not math.isfinite(alpha) or not 0 <= alpha <= 1e8 or not 0 <= mix <= 1 or degree not in (1, 2):
            raise ValueError("Invalid penalty or transformation settings.")
        transformers = []
        if self.numeric:
            expand=MonomialTerms(self.numeric_powers)if self.numeric_powers is not None else PolynomialFeatures(degree=degree, include_bias=False)
            numeric = Pipeline([("impute", SimpleImputer(strategy="median", keep_empty_features=True)),("expand",expand)])
            transformers.append(("numeric", numeric, self.numeric))
        if self.categorical:
            categorical = Pipeline([("impute", SimpleImputer(strategy="most_frequent", keep_empty_features=True)),
                                    ("encode", OneHotEncoder(drop="first", handle_unknown="ignore", sparse_output=False))])
            transformers.append(("category", categorical, self.categorical))
        preprocess = ColumnTransformer(transformers, sparse_threshold=0, verbose_feature_names_out=False)
        p = len(self.features)
        if self.model == "ols":
            estimator = LinearRegression()
        elif self.model == "logistic":
            estimator = LogisticRegression(C=np.inf, tol=1e-10, max_iter=1000)
        elif self.model == "ridge":
            # alpha is n-normalized in Model Lab; set its actual value at fit time.
            estimator = Ridge(alpha=alpha, solver="svd")
        elif self.model in ("lasso", "elastic_net"):
            estimator = (Lasso(alpha=alpha, tol=1e-9, max_iter=20000) if self.model == "lasso" else
                         ElasticNet(alpha=alpha, l1_ratio=mix, tol=1e-9, max_iter=20000))
        else:
            depth, leaf, count = int(c.get("depth", 3)), int(c.get("min_leaf", 3)), int(c.get("trees", 80))
            rate, fraction = float(c.get("rate", .1)), float(c.get("fraction", .7))
            if not 1 <= depth <= 10 or not 1 <= leaf <= 1000 or not 1 <= count <= 300 or not 0 < rate <= 1 or not .05 <= fraction <= 1:
                raise ValueError("Invalid tree configuration.")
            common = dict(max_depth=depth, min_samples_leaf=leaf, random_state=self.seed)
            if self.model == "tree":
                estimator = DecisionTreeRegressor(**common)
            elif self.model == "forest":
                estimator = RandomForestRegressor(n_estimators=count, max_features=int(c.get("mtry", p)), n_jobs=1, **common)
            else:
                estimator = GradientBoostingRegressor(n_estimators=count, learning_rate=rate,
                    subsample=fraction if self.model == "stochastic" else 1., **common)
        return Pipeline([("design", preprocess), ("scale", StandardScaler(with_std=bool(c.get("standardize", True)))), ("model", estimator)])

    def fit(self, rows, config=None):
        self.fits += 1
        if self.fits > self.max_fits:
            raise ValueError("Requested operation exceeded its declared fit budget.")
        self.tick("fit", observations=len(rows))
        if len(rows) < 3 or (self.binary and len(np.unique(self.y[rows])) != 2):
            raise ValueError("A training fold has too few rows or only one outcome class.")
        pipe = self.pipeline(config)
        c={**self.config,**(config or {})};width=len(self.numeric_powers)if self.numeric_powers is not None else len(self.numeric)*(len(self.numeric)+3)//2 if int(c.get('degree',1))==2 else len(self.numeric)
        width+=sum(max(0,self.frame.iloc[rows][column].nunique(dropna=True)-1)for column in self.categorical)
        if not 1<=width<=512:raise ValueError('Training-defined encoded design exceeds 512 predictors or has no predictors. No dense expansion was allocated.')
        design = pipe.named_steps["design"].fit_transform(self.frame.iloc[rows])
        if not 1 <= design.shape[1] <= 512:
            raise ValueError("Encoded design exceeds 512 predictors or has no identifiable predictors.")
        z = pipe.named_steps["scale"].fit_transform(design)
        estimator = pipe.named_steps["model"]
        if self.model=='forest'and not 1<=estimator.max_features<=design.shape[1]:raise ValueError('Features per split must be within the actual encoded predictor count.')
        if self.binary:
            # A bounded LP detects complete or quasi separation. Zero is always
            # feasible; a positive mean margin with no negative margins supplies
            # a separating direction. This is a numerical diagnostic, not an
            # assertion over arbitrary floating-point conditioning.
            signed = (2*self.y[rows]-1)[:,None]*np.column_stack([np.ones(len(rows)),z])
            diagnostic = linprog(-signed.mean(axis=0), A_ub=-signed, b_ub=np.zeros(len(rows)),
                                 bounds=[(-1.,1.)]*signed.shape[1], method="highs",
                                 options={"primal_feasibility_tolerance":1e-9,"dual_feasibility_tolerance":1e-9})
            if not diagnostic.success:
                raise ValueError("The numerical separation diagnostic failed; no logistic result was accepted.")
            margin = signed @ diagnostic.x
            if margin.min() >= -1e-8 and margin.mean() > 1e-7:
                raise ValueError("Complete or quasi separation was detected; no finite unpenalized logistic MLE was established.")
        if self.model == "ridge":
            estimator.set_params(alpha=len(rows) * float((config or {}).get("alpha", self.config.get("alpha", .1))))
        with warnings.catch_warnings(record=True) as caught:
            warnings.simplefilter("always")
            estimator.fit(z, self.y[rows])
        notices = [str(w.message) for w in caught]
        self.notices.extend(notices)
        if any("converg" in str(w.message).lower() for w in caught):
            raise ValueError("A scientific fit reported nonconvergence; it was not accepted for selection.")
        if self.binary:
            eta = estimator.decision_function(z)
            if np.all((2*self.y[rows]-1)*eta > 1e-8):
                raise ValueError("A separating score was found; no finite unpenalized logistic MLE was established.")
        return pipe

    def predict(self, pipe, rows):
        with warnings.catch_warnings(record=True) as caught:
            warnings.simplefilter("always")
            pred = pipe.predict_proba(self.frame.iloc[rows])[:, 1] if self.binary else pipe.predict(self.frame.iloc[rows])
        for w in caught:
            text = str(w.message)
            if text not in self.notices:
                self.notices.append(text)
        if not np.isfinite(pred).all():
            raise ValueError("Nonfinite predictions were rejected.")
        return np.asarray(pred)

    def loss(self, pipe, rows):
        pred = self.predict(pipe, rows)
        if self.binary:
            eta = pipe.decision_function(self.frame.iloc[rows])
            return float(np.where(self.y[rows] == 1, np.logaddexp(0, -eta), np.logaddexp(0, eta)).mean())
        return float(np.mean((self.y[rows]-pred)**2))

    def audit(self, pipe, rows):
        design = pipe.named_steps["design"]
        scale = pipe.named_steps["scale"]
        return {"training_rows": np.asarray(rows).tolist(), "feature_names": design.get_feature_names_out().tolist(),
                "design_mean": scale.mean_.tolist(), "design_scale": (scale.scale_ if scale.with_std else np.ones(len(scale.mean_))).tolist(),
                "numeric": self.numeric, "categorical": self.categorical,
                "numeric_imputation": design.named_transformers_["numeric"].named_steps["impute"].statistics_.tolist() if self.numeric else [],
                "categorical_levels": [a.tolist() for a in design.named_transformers_["category"].named_steps["encode"].categories_] if self.categorical else []}

    def fitted(self, pipe, rows):
        estimator, scaler = pipe.named_steps["model"], pipe.named_steps["scale"]
        result = {"training_rows": np.asarray(rows).tolist(), "prediction_rows": self.rows.tolist(),
                  "predictions": self.predict(pipe, self.rows).tolist(), "training_loss": self.loss(pipe, rows),
                  "preprocessing": self.audit(pipe, rows), "optimizer_trace": None}
        if hasattr(estimator, "coef_"):
            scale = scaler.scale_ if scaler.with_std else np.ones(len(scaler.mean_))
            slopes = estimator.coef_.reshape(-1)/scale
            intercept = float(np.asarray(estimator.intercept_).reshape(-1)[0] - scaler.mean_ @ slopes)
            result["coefficients"] = {"names": ["Intercept", *pipe.named_steps["design"].get_feature_names_out().tolist()],
                                      "values": [intercept, *slopes.tolist()]}
        return result

    def estimation_rows(self):
        rows = self.request.get("sample_rows", self.rows.tolist())
        if not isinstance(rows, list) or not rows or any(type(i) is not int for i in rows) or len(set(rows)) != len(rows):
            raise ValueError("Estimation sample must contain distinct integer row identities.")
        available = set(self.rows.tolist())
        if any(i not in available for i in rows):
            raise ValueError("Estimation sample includes an omitted or invalid row.")
        return np.array(rows, dtype=int)

    def inference(self):
        rows = self.estimation_rows()
        cov = self.request.get("inference", {}).get("covariance", "HC3" if not self.binary else "classical")
        allowed = LOGIT_COV if self.binary else OLS_COV if self.model == "ols" else ()
        if cov not in allowed:
            raise ValueError("This model does not expose the requested inferential capability. Penalized inference is not ordinary OLS inference.")
        pipe = self.fit(rows)
        # Inference is expressed in the original encoded feature units.
        encoded = pipe.named_steps["design"].transform(self.frame.iloc[rows])
        scaler = pipe.named_steps["scale"]
        design = np.column_stack([np.ones(len(rows)), scaler.transform(encoded)])
        scales = scaler.scale_ if scaler.with_std else np.ones(encoded.shape[1])
        transform = np.eye(design.shape[1])
        transform[0, 1:] = -scaler.mean_ / scales
        transform[1:, 1:] = np.diag(1 / scales)
        names = ["Intercept", *pipe.named_steps["design"].get_feature_names_out().tolist()]
        rank, k, n = np.linalg.matrix_rank(design), design.shape[1], len(rows)
        if rank != k or n <= k:
            raise ValueError("Ordinary inference requires a full-rank design and positive residual degrees of freedom.")
        options, kw, df, order = self.request.get("inference", {}), {}, n-k, np.arange(n)
        if cov == "cluster":
            group = self.roles.get("group")
            if not group or self.frame.iloc[rows][group].isna().any():
                raise ValueError("Cluster inference requires a complete, explicitly selected group column.")
            groups = self.frame.iloc[rows][group].astype(str).to_numpy()
            count = len(np.unique(groups))
            if count < 2:
                raise ValueError("Cluster inference requires at least two clusters.")
            kw = {"groups": groups, "use_correction": True, "df_correction": True}
            df = count-1
            if count < 30:
                self.notices.append(f"Only {count} clusters: conventional cluster inference can be unreliable with few clusters.")
        if cov == "HAC":
            time_column = self.roles.get("time")
            if not time_column:
                raise ValueError("HAC inference requires an explicit chronological time column.")
            t = pd.to_numeric(self.frame.iloc[rows][time_column], errors="raise").to_numpy(dtype=float)
            if not np.isfinite(t).all() or len(np.unique(t)) != n:
                raise ValueError("This HAC implementation requires finite, distinct time values; panel HAC is a separate capability.")
            order = np.argsort(t, kind="stable")
            lags = int(options.get("lags", 1))
            if not 0 <= lags < n:
                raise ValueError("HAC lags must be between zero and n-1.")
            kw = {"maxlags": lags, "kernel": "bartlett", "use_correction": True}
        model = Logit(self.y[rows][order], design[order]) if self.binary else OLS(self.y[rows][order], design[order])
        arguments = {"cov_type": "nonrobust" if cov == "classical" else cov, "cov_kwds": kw, "use_t": not self.binary}
        with warnings.catch_warnings(record=True) as caught:
            warnings.simplefilter("always")
            fit = model.fit(disp=False, maxiter=300, **arguments) if self.binary else model.fit(**arguments)
        self.notices.extend(str(w.message) for w in caught)
        if self.binary and (not fit.mle_retvals.get("converged") or any("separat" in str(w.message).lower() for w in caught)):
            raise ValueError("Logistic inference did not establish a converged finite MLE.")
        alpha = float(options.get("alpha", .05))
        if not .001 <= alpha <= .5:
            raise ValueError("Interval alpha must be between .001 and .5.")
        stable_covariance = np.asarray(fit.cov_params())
        if not np.isfinite(stable_covariance).all() or np.any(np.diag(stable_covariance) <= 0) or np.linalg.matrix_rank(stable_covariance) != k:
            raise ValueError("Singular or nonfinite inference covariance was rejected.")
        beta = transform @ fit.params
        covariance = transform @ stable_covariance @ transform.T
        native = np.array(self.fitted(pipe, rows)["coefficients"]["values"])
        if np.max(np.abs(native-beta)/(1+np.abs(beta))) > 2e-5:
            raise ValueError("Inference and prediction backends disagree on fitted coefficients; no table was attached.")
        errors = np.sqrt(np.diag(covariance))
        reference = stats.norm if self.binary else stats.t(df)
        critical = reference.ppf(1-alpha/2)
        statistics = beta/errors
        table = [{"term": name, "estimate": float(beta[j]), "std_error": float(errors[j]),
                  "statistic": float(statistics[j]), "p_value": float(2*reference.sf(abs(statistics[j]))),
                  "lower": float(beta[j]-critical*errors[j]), "upper": float(beta[j]+critical*errors[j])} for j, name in enumerate(names)]
        result = {**self.fitted(pipe, rows), "coefficient_table": table, "covariance": covariance.tolist(),
                  "covariance_type": cov, "n": n, "parameter_count": k, "rank": int(rank),
                  "reference_distribution": "normal" if self.binary else "Student t",
                  "inference_df": None if self.binary else int(df), "alpha": alpha,
                  "assumptions": self.inference_assumptions(cov), "ordered_sample_rows": rows[order].tolist()}
        restriction = options.get("restriction")
        if restriction:
            R, q = np.array(restriction["R"], float), np.array(restriction["q"], float)
            if R.ndim != 2 or R.shape[1] != k or q.shape != (R.shape[0],) or not np.isfinite(R).all() or not np.isfinite(q).all() or np.linalg.matrix_rank(R) != len(q):
                raise ValueError("Joint restrictions require a finite full-row-rank R and conformable q.")
            stable_R = R @ transform
            middle = stable_R @ stable_covariance @ stable_R.T
            if np.linalg.matrix_rank(middle) != len(q):
                raise ValueError("The restriction covariance is singular.")
            delta = stable_R @ fit.params-q
            wald = float(delta @ np.linalg.solve(middle, delta))
            result["joint_test"] = {"distribution": "chi-square" if self.binary else "F", "restrictions": len(q),
                "statistic": wald if self.binary else wald/len(q), "p_value": float(stats.chi2.sf(wald, len(q)) if self.binary else stats.f.sf(wald/len(q), len(q), df)),
                "R": R.tolist(), "q": q.tolist()}
        if self.binary and self.numeric and not self.expanded:
            # Evaluate marginal derivatives in encoded feature units; omit category indicators.
            probability = fit.predict(design)
            variance = probability*(1-probability)
            effects = []
            for j in range(len(self.numeric)):
                slope = beta[j+1]
                gradient = slope*np.mean((variance*(1-2*probability))[:,None]*design,axis=0)
                gradient[j+1] += variance.mean()/scales[j]
                effect = slope*variance.mean()
                error = np.sqrt(gradient @ stable_covariance @ gradient)
                z = stats.norm.ppf(1-alpha/2)
                effects.append({"term":names[j+1],"effect":float(effect),"std_error":float(error),"lower":float(effect-z*error),"upper":float(effect+z*error)})
            result["marginal_effects"] = effects
            result["marginal_effect_scope"] = "Average continuous-variable derivatives over the estimation sample; not categorical contrasts or causal effects."
        elif self.binary:
            self.notices.append("Continuous marginal effects are implemented for degree-one numeric designs only; categorical contrasts and expanded-design derivatives were not computed.")
        return result

    def inference_assumptions(self, covariance):
        common = ["The fitted relationship and sample selection require substantive justification; computation does not establish causal identification.",
                  "Intervals condition on the chosen specification and preprocessing; they do not account for prior model selection or single-imputation uncertainty."]
        if self.binary:
            common.append("A correctly specified conditional probability and a finite identifiable MLE are required for model-based inference.")
        if covariance == "classical":
            common.append("OLS Student t inference requires the classical homoskedastic normal-error model for an exact finite-sample reference.") if not self.binary else None
        elif covariance == "cluster":
            common.append("Clusters must be independent in the relevant asymptotic approximation; within-cluster dependence is allowed. Group count matters.")
        elif covariance == "HAC":
            common.append("Bartlett HAC requires a justified chronological ordering, lag choice and suitable weak-dependence asymptotics.")
        else:
            common.append("Heteroskedasticity robustness does not correct serial dependence, clustering, misspecification or endogeneity.")
        return common

    def groups(self, rows):
        column = self.roles.get("group")
        if not column or self.frame.iloc[rows][column].isna().any():
            raise ValueError("Group splitting requires complete group identities.")
        return self.frame.iloc[rows][column].astype(str).to_numpy()

    def time_blocks(self, rows):
        column = self.roles.get("time")
        if not column:
            raise ValueError("Temporal splitting requires an explicit time column.")
        values = pd.to_numeric(self.frame.iloc[rows][column], errors="raise").to_numpy(float)
        if not np.isfinite(values).all():
            raise ValueError("Time values must be finite numeric values.")
        unique = np.unique(values)
        return unique, values

    def holdout(self, design):
        fraction, kind = float(design.get("test_fraction", .2)), design.get("kind", "stratified" if self.binary else "random")
        if not .1 <= fraction <= .4 or kind not in ("random", "stratified", "group", "temporal"):
            raise ValueError("Use a supported design and a test fraction between .1 and .4.")
        if kind == "temporal":
            unique, values = self.time_blocks(self.rows)
            count = max(1, math.ceil(len(unique)*fraction))
            gap = int(design.get("gap", 0))
            if gap < 0 or len(unique)-count-gap < 3:
                raise ValueError("Too few time blocks for the requested holdout and gap.")
            test = self.rows[np.isin(values, unique[-count:])]
            pool = self.rows[np.isin(values, unique[:len(unique)-count-gap])]
        elif kind == "group":
            a, b = next(GroupShuffleSplit(n_splits=1, test_size=fraction, random_state=self.seed).split(self.rows, groups=self.groups(self.rows)))
            pool, test = self.rows[a], self.rows[b]
        else:
            pool, test = train_test_split(self.rows, test_size=fraction, random_state=self.seed,
                                         stratify=self.y[self.rows] if kind == "stratified" else None)
        return np.asarray(pool), np.asarray(test)

    def folds(self, rows, design, seed):
        k, repeats = int(design.get("folds", 3)), int(design.get("repeats", 1))
        kind = design.get("kind", "stratified" if self.binary else "random")
        if not 2 <= k <= 10 or not 1 <= repeats <= 5 or len(rows) <= k:
            raise ValueError("Use 2–10 folds, 1–5 repeats, and enough observations.")
        if kind == "temporal":
            if repeats != 1:
                raise ValueError("Temporal folds are deterministic; repeated shuffled temporal folds are not supported.")
            unique, values = self.time_blocks(rows)
            chunks = np.array_split(unique, k+1)
            gap = int(design.get("gap", 0))
            for i in range(k):
                prior = np.concatenate(chunks[:i+1])
                if gap < 0 or len(prior) <= gap or not len(chunks[i+1]):
                    raise ValueError("The temporal gap leaves an empty fold.")
                a = rows[np.isin(values, prior[:len(prior)-gap])]
                b = rows[np.isin(values, chunks[i+1])]
                yield 0, i, a, b
            return
        for repeat in range(repeats):
            if kind == "group":
                splitter = GroupKFold(n_splits=k, shuffle=True, random_state=seed+repeat)
                iterator = splitter.split(rows, self.y[rows], self.groups(rows))
            elif kind == "stratified":
                if not self.binary:
                    raise ValueError("Outcome stratification is available for binary classification only.")
                if min(np.bincount(self.y[rows].astype(int), minlength=2)) < k:
                    raise ValueError("Each class needs at least one observation per fold.")
                iterator = StratifiedKFold(k, shuffle=True, random_state=seed+repeat).split(rows, self.y[rows])
            elif kind == "random":
                iterator = KFold(k, shuffle=True, random_state=seed+repeat).split(rows)
            else:
                raise ValueError("Unknown resampling design.")
            for i, (a, b) in enumerate(iterator):
                yield repeat, i, rows[a], rows[b]

    def candidates(self, design):
        grid = design.get("alpha_grid", [self.config.get("alpha", .1)]) if self.model in ("ridge", "lasso", "elastic_net") else [None]
        if not 1 <= len(grid) <= 12 or any(v is not None and (not isinstance(v, (float, int)) or not math.isfinite(v) or v < 0 or v > 1e8) for v in grid):
            raise ValueError("Use 1–12 finite nonnegative candidate penalties.")
        return [{} if v is None else {"alpha": float(v)} for v in grid]

    def select(self, rows, design, candidates, seed):
        if len(candidates) == 1:
            return candidates[0], []
        inner = {**design, "folds": int(design.get("inner_folds", 3)), "repeats": 1}
        splits = list(self.folds(rows, inner, seed))
        evaluated = []
        for candidate in candidates:
            records = []
            for _, fold, train, valid in splits:
                pipe = self.fit(train, candidate)
                records.append({"fold": fold, "train_rows": train.tolist(), "validation_rows": valid.tolist(), "loss": self.loss(pipe, valid), "preprocessing": self.audit(pipe, train)})
            loss = sum(r["loss"]*len(r["validation_rows"]) for r in records)/sum(len(r["validation_rows"]) for r in records)
            evaluated.append({"config": candidate, "loss": loss, "folds": records})
        best = min(evaluated, key=lambda r: r["loss"])
        return best["config"], evaluated

    def resample(self, reveal=False):
        design = dict(self.request.get("resampling", {}))
        pool, test = self.holdout(design)
        candidates = self.candidates(design)
        outer=list(self.folds(pool,design,self.seed+101))
        inner_count=int(design.get('inner_folds',3))if len(candidates)>1 else 0
        planned=len(outer)*(len(candidates)*inner_count+1)+len(candidates)*inner_count+1
        if planned>self.max_fits:raise ValueError(f'This design requires {planned} fits, exceeding the declared budget of {self.max_fits}. No fits were started.')
        if inner_count:
            inner={**design,'folds':inner_count,'repeats':1}
            for _,_,train,_ in outer:list(self.folds(train,inner,self.seed+10001))
            list(self.folds(pool,inner,self.seed+20001))
        records = []
        for repeat, fold, train, valid in outer:
            chosen, selection = self.select(train, design, candidates, self.seed+10001+repeat*100+fold)
            pipe = self.fit(train, chosen)
            pred = self.predict(pipe, valid)
            records.append({"repeat": repeat, "fold": fold, "training_rows": train.tolist(), "validation_rows": valid.tolist(),
                            "validation_loss": self.loss(pipe, valid), "predictions": pred.tolist(), "selected_config": chosen,
                            "preprocessing": self.audit(pipe, train), "inner_selection": selection})
            self.tick("outer-fold", completed=len(records))
        chosen, selection = self.select(pool, design, candidates, self.seed+20001)
        final = self.fit(pool, chosen)
        weighted = sum(r["validation_loss"]*len(r["validation_rows"]) for r in records)/sum(len(r["validation_rows"]) for r in records)
        # A sealed run contains no test predictions or test loss. Only an explicit
        # evaluate_test operation computes them. Test identities remain auditable.
        result = {"design": design, "development_rows": pool.tolist(), "test_rows": test.tolist(), "folds": records,
                  "cv_loss": weighted, "fold_loss_sd": float(np.std([r["validation_loss"] for r in records], ddof=1)),
                  "fold_variation_interpretation": "Descriptive fold dispersion; not a standard error or confidence interval.",
                  "selected_config": chosen, "final_selection": selection, "final_preprocessing": self.audit(final, pool),
                  "development_training_loss": self.loss(final, pool), "test_revealed": bool(reveal),
                  "test_loss": None, "test_predictions": None}
        if reveal:
            result.update(test_loss=self.loss(final, test), test_predictions=self.predict(final, test).tolist())
        return result

    def run(self):
        action = self.request.get("operation", "fit")
        if action == "capabilities":
            result = capabilities()
        elif action == "fit":
            rows = self.estimation_rows()
            result = self.fitted(self.fit(rows), rows)
        elif action == "inference":
            result = self.inference()
        elif action in ("resample", "evaluate_test"):
            result = self.resample(reveal=action == "evaluate_test")
        elif action == "export":
            result = export_bundle(self.request)
        else:
            raise ValueError("Unsupported adapter operation.")
        return result


def execute(request, progress=None):
    job = Job(request, progress)
    result = job.run()
    response = {"schema_version": SCHEMA, "request_id": request["request_id"], "request_sha256": digest(request),
                "worker_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(), "engine_version": ENGINE,
                "backend": "scientific-python", "operation": request.get("operation", "fit"), "status": "completed",
                "versions": {p: importlib.metadata.version(p) for p in ("numpy", "scipy", "pandas", "scikit-learn", "statsmodels", "matplotlib")},
                "python": sys.version.split()[0], "dataset_sha256": digest(request.get("dataset", {})),
                "excluded_rows": job.excluded, "warnings": list(dict.fromkeys(job.notices)), "fits": job.fits,
                "elapsed_seconds": time.monotonic()-job.started, "result": result}
    if hasattr(job,"frame"):
        response["data_profile"]={"input_rows":len(job.frame),"eligible_outcomes":len(job.rows),"omitted_rows":job.excluded,
            "columns":[{"name":c,"missing":int(job.frame[c].isna().sum()),"distinct_nonmissing":int(job.frame[c].nunique(dropna=True)),
                        "role":"response" if c==job.target else "predictor" if c in job.features else "group" if c==job.roles.get("group") else "time" if c==job.roles.get("time") else "unused",
                        "kind":"numeric" if c in job.numeric else "categorical" if c in job.categorical else "metadata"}for c in job.frame.columns]}
    # Reject nonfinite outputs rather than serializing nonstandard JSON numbers.
    json.dumps(response, allow_nan=False)
    return response


def numerical_match(a, b, tolerance=1e-7):
    """Compare scientific results, not elapsed time, request IDs or progress."""
    if isinstance(a, bool) or isinstance(b, bool):
        return a == b
    if isinstance(a, (int, float)) and isinstance(b, (int, float)):
        return math.isfinite(a) and math.isfinite(b) and abs(a-b) <= tolerance*(1+abs(a))
    if isinstance(a, list) and isinstance(b, list):
        return len(a) == len(b) and all(numerical_match(x,y,tolerance) for x,y in zip(a,b))
    if isinstance(a, dict) and isinstance(b, dict):
        return a.keys() == b.keys() and all(numerical_match(a[k],b[k],tolerance) for k in a)
    return a == b


def export_bundle(request):
    """Export a completed scientific artifact without silently refitting it."""
    import csv
    import shutil
    import textwrap
    source, response = request.get("source_request", {}), request.get("source_response", {})
    wire = request.get("source_wire", "")
    if json.loads(wire) != source or hashlib.sha256(wire.encode()).hexdigest() != response.get("request_sha256"):
        raise ValueError("The export request does not match the completed analysis bytes.")
    if response.get("worker_sha256") != hashlib.sha256(Path(__file__).read_bytes()).hexdigest():
        raise ValueError("This historical analysis used different worker source. Reproduce it with the current engine before exporting executable code.")
    if response.get("request_id") != source.get("request_id") or response.get("status") != "completed":
        raise ValueError("Only a completed matching analysis can be exported.")
    if source.get("operation") not in ("fit","inference","resample","evaluate_test") or response.get("operation")!=source.get("operation"):
        raise ValueError("Only model fitting, inference or resampling analyses can be exported.")
    destination = Path(request.get("export_path", "")).expanduser()
    if not str(request.get("export_path", "")).strip() or not destination.is_absolute():
        raise ValueError("Choose an absolute export destination.")
    if destination.exists():
        raise ValueError("Export destination already exists. Choose a new folder so existing work is preserved.")
    destination.mkdir(parents=True)
    try:
        (destination/'request.json').write_bytes(wire.encode())
        (destination/'result.json').write_text(json.dumps(response, indent=2, allow_nan=False)+'\n')
        (destination/'research_worker.py').write_bytes(Path(__file__).read_bytes())
        with (destination/'data.csv').open('w',newline='') as f:
            writer=csv.writer(f);writer.writerow(source['dataset']['columns']);writer.writerows(source['dataset']['rows'])
        versions=response['versions']
        (destination/'requirements.txt').write_text(''.join(f'{name}=={version}\n' for name,version in versions.items()))
        (destination/'reproduce.py').write_text(textwrap.dedent('''\
            """Reproduce this recorded analysis with the included worker and verify results."""
            import hashlib
            import json
            from pathlib import Path
            root=Path(__file__).resolve().parent
            expected=json.loads((root/'result.json').read_text())
            source=(root/'research_worker.py').read_bytes()
            if hashlib.sha256(source).hexdigest()!=expected['worker_sha256']:
                raise SystemExit('Worker source integrity check failed; no analysis was run.')
            wire=(root/'request.json').read_bytes()
            if hashlib.sha256(wire).hexdigest()!=expected['request_sha256']:
                raise SystemExit('Request integrity check failed; no analysis was run.')
            import sys
            sys.path.insert(0,str(root))
            import research_worker as worker
            actual=worker.execute(json.loads(wire))
            drift={name:[version,actual['versions'].get(name)] for name,version in expected['versions'].items() if actual['versions'].get(name)!=version}
            if not worker.numerical_match(expected['result'],actual['result']):
                raise SystemExit('Numerical reproduction failed. Version drift: '+json.dumps(drift))
            report={'status':'passed','version_drift':drift,'tolerance':1e-7,'source_verified':True,'request_verified':True}
            (root/'reproduction-verification.json').write_text(json.dumps(report,indent=2)+'\\n')
            print(json.dumps(report))
            '''))
        result=response['result']
        def escaped(text):
            return ''.join({'\\':r'\textbackslash{}','&':r'\&','%':r'\%','$':r'\$','#':r'\#','_':r'\_','{':r'\{','}':r'\}'}.get(c,c) for c in str(text))
        tex=[r'\documentclass[11pt]{article}',r'\usepackage[margin=1in]{geometry}',r'\usepackage{booktabs,graphicx,longtable,array}',r'\begin{document}',r'\section*{Model Lab research result}',
             escaped(f"Model: {source['model']}. Operation: {source['operation']}. Dataset: {source['dataset'].get('name','Selected data')}."),r'\par']
        tex += [r'\section*{Specification}',escaped(source.get('formula') or 'Predictors: '+', '.join(source['roles']['features'])),r'\par',escaped('Settings: '+json.dumps(source.get('config',{}),sort_keys=True)),r'\par']
        table=result.get('coefficient_table',[])
        if table:
            with (destination/'coefficients.csv').open('w',newline='') as f:
                writer=csv.DictWriter(f,fieldnames=list(table[0]));writer.writeheader();writer.writerows(table)
            tex += [r'\begin{longtable}{p{0.29\linewidth}rrrr}',r'\toprule Term & Estimate & Std. error & Lower & Upper \\',r'\midrule']
            for row in table:tex.append(escaped(row['term'])+' & '+ ' & '.join(f"{row[k]:.6g}"for k in ['estimate','std_error','lower','upper'])+r' \\')
            tex += [r'\bottomrule',r'\end{longtable}',r'\par',escaped(f"Covariance: {result['covariance_type']}. Estimation observations: {result['n']}. Reference: {result['reference_distribution']}; degrees of freedom: {result['inference_df']}.")]
        if 'folds' in result:
            with (destination/'folds.csv').open('w',newline='') as f:
                writer=csv.writer(f);writer.writerow(['repeat','fold','training_rows','validation_rows','validation_loss','selected_config'])
                for fold in result['folds']:writer.writerow([fold['repeat'],fold['fold'],json.dumps(fold['training_rows']),json.dumps(fold['validation_rows']),fold['validation_loss'],json.dumps(fold['selected_config'])])
            tex += [r'\par',escaped(f"Cross-validation loss: {result['cv_loss']:.6g}. Test disclosed: {result['test_revealed']}. Fold dispersion is descriptive, not a confidence interval.")]
        assumptions=result.get('assumptions',[])
        if assumptions:tex += [r'\section*{Assumptions and interpretation}',*[escaped(a)+r'\par'for a in assumptions]]
        figures=export_figures(source,response,destination)
        tex += [r'\section*{Figure}',r'\includegraphics[width=\linewidth]{figure.pdf}',r'\end{document}']
        (destination/'report.tex').write_text('\n'.join(tex)+'\n')
        lesson=teaching_snapshot(source,response)
        (destination/'teaching-snapshot.json').write_text(json.dumps(lesson,indent=2)+'\n')
        worksheet=[r'\documentclass[11pt]{article}',r'\usepackage[margin=1in]{geometry}',r'\begin{document}',r'\section*{Research interpretation worksheet}',escaped(lesson['context'])]
        for prompt in lesson['questions']:worksheet += [r'\paragraph{'+escaped(prompt['title'])+'}',escaped(prompt['prompt']),r'\vspace{1.5cm}']
        (destination/'student-worksheet.tex').write_text('\n'.join(worksheet+[r'\end{document}'])+'\n')
        (destination/'instructor-key.txt').write_text('\n\n'.join(q['title']+'\n'+q['answer']for q in lesson['questions'])+'\n')
        (destination/'README.txt').write_text('Model Lab reproducible research bundle\n\nRun reproduce.py with a scientific Python environment containing the pinned packages in requirements.txt. The application bundled Python can be used. No package installation is performed by reproduce.py.\n\nThe script verifies source/request hashes, reruns the recorded procedure, compares numerical results and reports software-version drift. Reproducing an already disclosed test result does not make it an untouched test.\n\nreport.tex and the worksheet are LaTeX sources; figure.pdf and figure.svg are vector figures. The bundle includes instructor answers; share student-worksheet.tex separately for students. result.json retains exact sample/fold identities, warnings and conventions. data.csv contains the selected data; choose sharing destinations accordingly.\n')
        hashes={p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in destination.iterdir() if p.is_file()}
        (destination/'manifest.json').write_text(json.dumps({'schema_version':1,'source_request_id':source['request_id'],'source_worker_sha256':response['worker_sha256'],'files':hashes},indent=2)+'\n')
        return {'destination':str(destination),'files':sorted([*hashes,'manifest.json']),'source_request_id':source['request_id'],'figures':figures}
    except Exception:
        shutil.rmtree(destination)
        raise


def export_figures(source,response,destination):
    import matplotlib
    matplotlib.use('Agg')
    import matplotlib.pyplot as plt
    result=response['result']
    with plt.rc_context({'font.family':'DejaVu Serif','font.size':10,'axes.spines.top':False,'axes.spines.right':False}):
        fig,ax=plt.subplots(figsize=(7,4.5),layout='constrained')
        table=result.get('coefficient_table')
        if table:
            if len(table)>30:ax.text(0,1.02,'First 30 coefficients shown; complete table in coefficients.csv.',transform=ax.transAxes,fontsize=8)
            table=table[:30]
            fig.set_size_inches(7,min(7.2,max(4.5,len(table)*.22)))
            labels=[r['term'] for r in table];y=np.arange(len(table));b=np.array([r['estimate']for r in table])
            intervals=np.array([[r['lower'],r['upper']]for r in table]);ax.errorbar(b,y,xerr=np.column_stack([b-intervals[:,0],intervals[:,1]-b]).T,fmt='o',color='black',capsize=3)
            ax.set_yticks(y,labels);ax.invert_yaxis();ax.axvline(0,color='.6',lw=.8);ax.set_xlabel('Coefficient and confidence interval');ax.set_title(f"{source['model']} — {result['covariance_type']} covariance")
        elif 'folds' in result:
            values=[f['validation_loss']for f in result['folds']];ax.plot(np.arange(1,len(values)+1),values,'o-',color='black');ax.axhline(result['cv_loss'],ls='--',color='.5',label='Weighted mean');ax.set_xlabel('Completed outer fold');ax.set_ylabel('Log loss'if source['model']=='logistic'else'Mean squared error');ax.legend(frameon=False);ax.set_title('Out-of-fold prediction performance')
        else:
            ids=result['prediction_rows'];columns=source['dataset']['columns'];target=columns.index(source['roles']['target']);observed=np.array([source['dataset']['rows'][i][target]for i in ids],float)
            ax.scatter(observed,result['predictions'],s=12,color='black',alpha=.6);ax.set_xlabel('Observed outcome');ax.set_ylabel('Predicted probability'if source['model']=='logistic'else'Prediction');ax.set_title(source['model'])
        fig.savefig(destination/'figure.pdf');fig.savefig(destination/'figure.svg');plt.close(fig)
    return ['figure.pdf','figure.svg']


def teaching_snapshot(source,response):
    result=response['result']
    questions=[]
    if 'coefficient_table' in result:
        row=result['coefficient_table'][1];df=result['inference_df'];reference=result['reference_distribution']
        context=f"{source['model']} on {result['n']} estimation observations; {result['covariance_type']} covariance; {reference} reference with df={df}."
        questions=[{'title':'Parameter interpretation','prompt':f"Interpret the estimate {row['estimate']:.5g} for {row['term']}, stating the held-fixed quantities and units.",'answer':'Interpretation concerns the specified encoded predictor and fitted relationship. A causal effect requires additional identification assumptions.'},
                   {'title':'Uncertainty','prompt':f"Explain the interval [{row['lower']:.5g}, {row['upper']:.5g}] and what the selected covariance permits you to assume.",'answer':'The confidence procedure has its stated repeated-sampling interpretation under the displayed assumptions; it does not assign a posterior probability to a fixed parameter.'},
                   {'title':'Specification and selection','prompt':'Does this interval account for choosing this specification after examining alternatives?','answer':'No. The reported covariance conditions on the chosen specification and preprocessing; ordinary intervals do not include prior selection uncertainty.'}]
    elif 'folds' in result:
        fold=result['folds'][0];context=f"{source['model']}; {result['design'].get('kind','random')} resampling; weighted outer loss {result['cv_loss']:.5g}; test disclosed={result['test_revealed']}."
        questions=[{'title':'Preprocessing boundary','prompt':f"The first fold has {len(fold['training_rows'])} training and {len(fold['validation_rows'])} validation observations. Which rows may determine its imputation values and scales?",'answer':'Only this fold’s training rows. Its validation rows, outer held-out rows and final test rows cannot fit those transformations.'},
                   {'title':'Tuning and evaluation','prompt':'What does the outer cross-validation loss evaluate when penalties were selected inside each outer training fold?','answer':'It evaluates the specified model-selection procedure, including its inner tuning, on outer held-out observations.'},
                   {'title':'Fold variation','prompt':f"The displayed fold-loss SD is {result['fold_loss_sd']:.5g}. Is it a confidence interval or the standard error of the mean loss?",'answer':'Neither. It is descriptive dispersion across dependent fold results. A valid uncertainty procedure requires a separate justification.'}]
    else:
        context=f"Completed {source['model']} fit on {len(result['training_rows'])} observations."
        questions=[{'title':'Sample','prompt':'Identify the estimation observations and preprocessing sample.','answer':'Use the exact training_rows and preprocessing records in result.json.'},
                   {'title':'Generalization','prompt':'Does training performance establish performance on new data?','answer':'No. Use an appropriate held-out or resampling design.'}]
    return {'schema_version':1,'source_request_id':source['request_id'],'context':context,'questions':questions}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--request", required=True)
    parser.add_argument("--response", required=True)
    args = parser.parse_args()
    request_path, destination = Path(args.request), Path(args.response)
    if request_path.stat().st_size > 32*1024*1024:
        raise ValueError("Request exceeds 32 MiB.")
    wire = request_path.read_bytes()
    request = json.loads(wire)
    def progress(event):
        print(json.dumps(event, allow_nan=False), flush=True)
    try:
        result = execute(request, progress)
    except Exception as error:
        result = {"schema_version": SCHEMA, "request_id": request.get("request_id"), "status": "failed",
                  "error_type": type(error).__name__, "error": str(error)}
    result["request_sha256"] = hashlib.sha256(wire).hexdigest()
    result["worker_sha256"] = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
    temporary = destination.with_suffix(destination.suffix+".partial")
    temporary.write_text(json.dumps(result, allow_nan=False, indent=2)+"\n")
    os.replace(temporary, destination)
    return 0 if result["status"] == "completed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
