"""
The binary must be built with `--features acceptance`; shipped builds omit the flags.
Independent NumPy/scikit-learn checks against actual native fixture output.

Run with an existing scientific uv environment. The CLI fixture generator must
be built first. This script does not install dependencies or alter student data.
"""
from pathlib import Path
import argparse
import json
import subprocess
import sys
import numpy as np
import sklearn
from sklearn.linear_model import LinearRegression, Ridge, Lasso, ElasticNet, LogisticRegression

ROOT = Path(__file__).resolve().parent

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--binary', default=str(ROOT/'target/debug/tree-ensemble-lab'))
    args = parser.parse_args()
    target = ROOT/'verification-linear-fixtures.json'
    subprocess.run([args.binary, '--linear-fixtures', '--report', str(target)], check=True, timeout=60)
    fixtures = json.loads(target.read_text())['fixtures']
    results = []
    for f in fixtures:
        X, y = np.asarray(f['data']['x']), np.asarray(f['data']['y'])
        train = np.array(f['partition']['train'])
        valid = np.array(f['partition']['valid'])
        config = f['config']
        mu, scale = X[train].mean(axis=0), X[train].std(axis=0)
        scale[scale < 1e-14] = 1
        Z = (X-mu)/scale
        method = config['model']
        alpha = config['alpha']
        model = {
            'ols': lambda: LinearRegression(),
            'ridge': lambda: Ridge(alpha=len(train)*alpha, solver='svd'),
            'lasso': lambda: Lasso(alpha=alpha, tol=1e-12, max_iter=100000),
            'elastic_net': lambda: ElasticNet(alpha=alpha, l1_ratio=config['mix'], tol=1e-12, max_iter=100000),
            'logistic': lambda: LogisticRegression(C=np.inf, tol=1e-12, max_iter=10000),
        }[method]()
        model.fit(Z[train], y[train])
        reference = model.predict_proba(Z)[:,1] if method=='logistic' else model.predict(Z)
        native = np.asarray(f['final_predictions'])
        error = np.max(np.abs(native-reference))
        coefficient = np.asarray(f['trace'][-1]['coefficients'][1])
        expected_coef = model.coef_.reshape(-1)/scale
        coefficient_error = np.max(np.abs(coefficient-expected_coef))
        assert f['converged'], (method, f['warning'])
        assert error < 2e-6, (method, error)
        assert coefficient_error < 3e-6, (method, coefficient_error)
        assert not set(train)&set(valid)
        assert len(train)+len(valid)==len(y)
        objectives = [s['objective'] for s in f['trace']]
        assert all(b<=a+1e-10*(1+abs(a)) for a,b in zip(objectives,objectives[1:])), method
        results.append({'name':f['name'],'model':method,'max_prediction_error':float(error),
                        'max_coefficient_error':float(coefficient_error),
                        'stationarity':f['trace'][-1]['stationarity'],'states':len(f['trace'])})
    report={'status':'passed','fixtures':len(results),'python':sys.version.split()[0],
            'numpy':np.__version__,'sklearn':sklearn.__version__,'results':results,
            'conventions':'Training-only population-standard-deviation scaling; Ridge alpha_library=n_train*alpha; logistic C=infinity.',
            'scope':'Selected independent fixtures, not complete package parity or a proof over all inputs.'}
    (ROOT/'verification-linear-reference.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps({'status':'passed','fixtures':len(results),
                      'max_prediction_error':max(r['max_prediction_error'] for r in results),
                      'max_coefficient_error':max(r['max_coefficient_error'] for r in results)}))

if __name__ == '__main__':
    main()
