"""Independent numerical and boundary tests of Model Lab's actual adapter.

No real client data. Covariance oracles below use direct NumPy matrix formulas,
not statsmodels covariance helpers. CV checks mutate held-out outcomes and verify
fold-specific preprocessing and group/time boundaries.
"""
from pathlib import Path
import copy
import importlib.util
import json
import unittest
import tempfile
import hashlib
import subprocess
import sys

import numpy as np
from scipy import stats

ROOT = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location('research_worker', ROOT/'python/research_worker.py')
worker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(worker)


def fixture(model='ols', seed=471007, n=160):
    rng = np.random.default_rng(seed)
    x = rng.normal(size=(n, 3))
    e = rng.normal(size=n)*(0.3+np.abs(x[:, 0]))
    y = .7+x@np.array([1.5, -.8, .2])+e
    if model == 'logistic':
        y = rng.binomial(1, 1/(1+np.exp(-(.1+x@np.array([.8, -.5, .2])))))
    rows = [[*a, float(b), str(i//4), i] for i, (a, b) in enumerate(zip(x, y))]
    return {'schema_version': 1, 'request_id': f'fixture-{model}', 'operation': 'inference', 'model': model,
            'seed': seed, 'dataset': {'name':'Synthetic independent reference', 'columns':['a', 'b', 'c', 'y', 'group', 'time'], 'rows':rows},
            'roles': {'target':'y', 'features':['a', 'b', 'c'], 'group':'group', 'time':'time'},
            'config': {'standardize':True, 'alpha':.1, 'mix':.4, 'degree':1},
            'inference': {'covariance':'classical', 'alpha':.05, 'lags':3},
            'resampling': {'kind':'random', 'folds':3, 'inner_folds':3, 'repeats':1, 'test_fraction':.2, 'gap':0, 'alpha_grid':[.01,.1,1.]}}


def independent_ols(request, kind):
    rows = np.asarray(request['dataset']['rows'], dtype=object)
    X = np.column_stack([np.ones(len(rows)), rows[:, :3].astype(float)])
    y = rows[:, 3].astype(float)
    n, k = X.shape
    B = np.linalg.inv(X.T@X)
    beta = np.linalg.lstsq(X, y, rcond=None)[0]
    e = y-X@beta
    leverage = np.einsum('ij,jk,ik->i', X, B, X)
    if kind == 'classical':
        return beta, B*(e@e)/(n-k), n-k
    if kind.startswith('HC'):
        scale = e**2
        if kind == 'HC1': scale *= n/(n-k)
        if kind == 'HC2': scale /= 1-leverage
        if kind == 'HC3': scale /= (1-leverage)**2
        return beta, B@(X.T@(X*scale[:, None]))@B, n-k
    scores = X*e[:, None]
    if kind == 'cluster':
        groups = rows[:, 4]
        sums = np.array([scores[groups==g].sum(axis=0) for g in np.unique(groups)])
        G = len(sums)
        return beta, B@(sums.T@sums)@B*(G/(G-1))*((n-1)/(n-k)), G-1
    meat = scores.T@scores
    L = request['inference']['lags']
    for lag in range(1, L+1):
        gamma = scores[lag:].T@scores[:-lag]
        meat += (1-lag/(L+1))*(gamma+gamma.T)
    return beta, B@meat@B*n/(n-k), n-k


class ResearchTests(unittest.TestCase):
    def test_formula_is_structural_and_matches_independent_design(self):
        q=fixture();q['formula']='y ~ 1 + a + b^2 + a:b';r=worker.execute(q)['result']
        raw=np.asarray(q['dataset']['rows'],object);a,b=raw[:,0].astype(float),raw[:,1].astype(float);X=np.column_stack([np.ones(len(a)),a,b*b,a*b]);expected=np.linalg.lstsq(X,raw[:,3].astype(float),rcond=None)[0]
        np.testing.assert_allclose([v['estimate']for v in r['coefficient_table']],expected,rtol=1e-9,atol=1e-10)
        q['formula']="y ~ __import__('os').system('anything')"
        with self.assertRaisesRegex(ValueError,'Unknown'):worker.execute(q)

    def test_large_research_sample_and_allocation_preflight(self):
        q=fixture(n=50000);r=worker.execute(q)['result'];expected,cov,df=independent_ols(q,'classical')
        self.assertEqual(r['n'],50000);np.testing.assert_allclose([v['estimate']for v in r['coefficient_table']],expected,rtol=1e-9)
        np.testing.assert_allclose(r['covariance'],cov,rtol=1e-9,atol=1e-11)
        q=fixture(n=600);q['roles']['features']=['group'];q['roles']['categorical']=['group'];q['dataset']['rows']=[[*row[:4],str(i),row[5]]for i,row in enumerate(q['dataset']['rows'])]
        with self.assertRaisesRegex(ValueError,'No dense expansion'):worker.execute(q)
        q=fixture('ridge');q['operation']='resample';q['max_fits']=2
        with self.assertRaisesRegex(ValueError,'No fits were started'):worker.execute(q)
    def test_ols_covariance_and_reference_distributions(self):
        for cov in worker.OLS_COV:
            q = fixture();q['inference']['covariance'] = cov
            r = worker.execute(q)['result']
            beta, covariance, df = independent_ols(q, cov)
            np.testing.assert_allclose([v['estimate'] for v in r['coefficient_table']], beta, rtol=1e-10, atol=1e-11)
            np.testing.assert_allclose(r['covariance'], covariance, rtol=1e-9, atol=1e-11)
            self.assertEqual(r['inference_df'], df)
            se = np.sqrt(np.diag(covariance))
            np.testing.assert_allclose([v['lower'] for v in r['coefficient_table']], beta-stats.t.ppf(.975, df)*se, rtol=1e-9)
            np.testing.assert_allclose([v['p_value'] for v in r['coefficient_table']], 2*stats.t.sf(np.abs(beta/se), df), atol=1e-12)

    def test_joint_restriction_independent_formula(self):
        q = fixture();q['inference']['covariance'] = 'HC3'
        R = np.array([[0,1,0,0],[0,0,1,0]],float);v = np.array([1.5,-.8])
        q['inference']['restriction'] = {'R':R.tolist(),'q':v.tolist()}
        r = worker.execute(q)['result'];b,C,df = independent_ols(q,'HC3')
        delta = R@b-v;F = delta@np.linalg.solve(R@C@R.T,delta)/2
        self.assertAlmostEqual(F,r['joint_test']['statistic'],places=10)
        self.assertAlmostEqual(stats.f.sf(F,2,df),r['joint_test']['p_value'],places=10)

    def test_logit_information_score_and_delta_method(self):
        for cov in worker.LOGIT_COV:
            q = fixture('logistic');q['inference']['covariance'] = cov
            r = worker.execute(q)['result'];raw = np.asarray(q['dataset']['rows'],object)
            X = np.column_stack([np.ones(len(raw)),raw[:,:3].astype(float)]);y = raw[:,3].astype(float)
            beta = np.array([v['estimate'] for v in r['coefficient_table']]);p = 1/(1+np.exp(-X@beta));v = p*(1-p)
            self.assertLess(np.max(np.abs(X.T@(y-p))),1e-7)
            B = np.linalg.inv(X.T@(v[:,None]*X));scores = X*(y-p)[:,None]
            if cov == 'classical': C = B
            elif cov == 'HC0': C = B@(scores.T@scores)@B
            else:
                groups = raw[:,4];sums=np.array([scores[groups==g].sum(0)for g in np.unique(groups)]);n,k=X.shape;G=len(sums)
                C=B@(sums.T@sums)@B*(G/(G-1))*((n-1)/(n-k))
            np.testing.assert_allclose(r['covariance'],C,atol=1e-10,rtol=1e-9)
            for j, effect in enumerate(r['marginal_effects'],1):
                gradient=beta[j]*np.mean((v*(1-2*p))[:,None]*X,axis=0);gradient[j]+=v.mean()
                self.assertAlmostEqual(effect['effect'],beta[j]*v.mean(),places=10)
                self.assertAlmostEqual(effect['std_error'],np.sqrt(gradient@C@gradient),places=10)

    def test_rank_separation_and_capability_failures(self):
        q=fixture();q['roles']['features']=['a','a']
        with self.assertRaises(ValueError): worker.execute(q)
        q=fixture();q['dataset']['rows']=[[*row[:2],row[0],*row[3:]]for row in q['dataset']['rows']]
        with self.assertRaisesRegex(ValueError,'full-rank'): worker.execute(q)
        q=fixture('ridge')
        with self.assertRaisesRegex(ValueError,'capability'): worker.execute(q)
        q=fixture('logistic');q['dataset']['rows']=[[*row[:3],float(row[0]>0),*row[4:]]for row in q['dataset']['rows']]
        with self.assertRaisesRegex(ValueError,'separat'): worker.execute(q)
        q=fixture('logistic',n=20);q['roles']['features']=['a']
        q['dataset']['rows']=[[float(i%3-1),0.,0.,float(i%3==2 or (i%3==1 and i%2==0)),str(i//4),i]for i in range(20)]
        with self.assertRaisesRegex(ValueError,'quasi separation'): worker.execute(q)

    def test_nested_cv_has_no_test_disclosure_and_training_only_transforms(self):
        q=fixture('ridge');q['operation']='resample'
        r=worker.execute(q)['result'];test=set(r['test_rows']);development=set(r['development_rows'])
        self.assertFalse(test & development);self.assertIsNone(r['test_loss']);self.assertIsNone(r['test_predictions'])
        raw=np.array(q['dataset']['rows'],object)[:,:3].astype(float)
        for fold in r['folds']:
            train,valid=fold['training_rows'],fold['validation_rows']
            self.assertFalse(set(train)&set(valid));self.assertFalse((set(train)|set(valid))&test)
            np.testing.assert_allclose(fold['preprocessing']['design_mean'],raw[train].mean(0))
            for candidate in fold['inner_selection']:
                for inner in candidate['folds']:
                    self.assertTrue(set(inner['train_rows'])<=set(train));self.assertTrue(set(inner['validation_rows'])<=set(train))
                    np.testing.assert_allclose(inner['preprocessing']['design_mean'],raw[inner['train_rows']].mean(0))
        mutant=copy.deepcopy(q)
        for i in test: mutant['dataset']['rows'][i][3] += 100000
        changed=worker.execute(mutant)['result']
        self.assertEqual(r['selected_config'],changed['selected_config']);self.assertEqual(r['cv_loss'],changed['cv_loss'])
        q['operation']='evaluate_test';opened=worker.execute(q)['result'];self.assertIsNotNone(opened['test_loss'])

    def test_group_and_temporal_boundaries(self):
        for kind in ['group','temporal']:
            q=fixture('ridge',n=240);q['operation']='resample';q['resampling']['kind']=kind;q['resampling']['alpha_grid']=[.1]
            q['resampling']['gap']=1 if kind=='temporal' else 0
            result=worker.execute(q)['result'];raw=np.array(q['dataset']['rows'],object)
            for train,valid in [(result['development_rows'],result['test_rows'])]+[(f['training_rows'],f['validation_rows'])for f in result['folds']]:
                if kind=='group': self.assertFalse(set(raw[train,4])&set(raw[valid,4]))
                else: self.assertLess(max(raw[train,5]),min(raw[valid,5])-1)

    def test_missingness_categories_and_expansion_fit_inside_fold(self):
        q=fixture('ridge');q['operation']='resample';q['resampling']['alpha_grid']=[.1]
        q['roles']['features']=['a','b','group'];q['dataset']['rows'][0][0]=None;q['config']['degree']=2
        result=worker.execute(q)['result']
        for f in result['folds']:
            audit=f['preprocessing'];self.assertEqual(audit['numeric'],['a','b','group'])  # numeric-looking group strings are numeric unless declared otherwise
            self.assertTrue(all(np.isfinite(audit['design_mean'])))
        q['dataset']['rows'][1][3]=None
        with self.assertRaisesRegex(ValueError,'explicit omission'):worker.execute(q)
        q['roles']['omit_missing_outcome']=True;self.assertIn(1,worker.execute(q)['excluded_rows'])

    def test_declared_categories_use_training_levels(self):
        q=fixture('ridge');q['operation']='resample';q['resampling']['alpha_grid']=[.1]
        q['roles']['features']=['a','group'];q['roles']['categorical']=['group']
        result=worker.execute(q)['result'];raw=q['dataset']['rows']
        for fold in result['folds']:
            levels=fold['preprocessing']['categorical_levels'][0]
            expected=sorted({str(raw[i][4])for i in fold['training_rows']})
            self.assertEqual(levels,expected)

    def test_large_offset_and_extreme_log_loss(self):
        q=fixture();baseline=worker.execute(q)['result'];q['dataset']['rows']=[[*[float(v)+1e9 for v in r[:3]],*r[3:]]for r in q['dataset']['rows']]
        actual=worker.execute(q)['result']
        np.testing.assert_allclose([r['estimate']for r in actual['coefficient_table'][1:]],[r['estimate']for r in baseline['coefficient_table'][1:]],rtol=2e-6,atol=2e-6)
        np.testing.assert_allclose([r['std_error']for r in actual['coefficient_table'][1:]],[r['std_error']for r in baseline['coefficient_table'][1:]],rtol=2e-6)
        q=fixture('logistic');job=worker.Job(q,lambda event:None)
        class Extreme:
            def predict_proba(self,frame):return np.column_stack([np.ones(len(frame)),np.zeros(len(frame))])
            def decision_function(self,frame):return np.full(len(frame),-1000.)
        self.assertAlmostEqual(job.loss(Extreme(),job.rows),1000*job.y.mean())

    def test_protocol_capabilities_and_export_reproduction(self):
        c=worker.execute({'schema_version':1,'request_id':'manifest','operation':'capabilities'})
        self.assertEqual(len(c['result']['models']),9)
        q=fixture();q['inference']['covariance']='HC3';response=worker.execute(q)
        wire=json.dumps(q,indent=2);response['request_sha256']=hashlib.sha256(wire.encode()).hexdigest()
        with tempfile.TemporaryDirectory(prefix='model-lab-export-test-') as folder:
            destination=Path(folder)/'bundle'
            request={**q,'operation':'export','request_id':'export','source_request':q,'source_response':response,'source_wire':wire,'export_path':str(destination)}
            exported=worker.execute(request)
            self.assertIn('figure.pdf',exported['result']['files'])
            self.assertEqual((destination/'request.json').read_bytes(),wire.encode())
            completed=subprocess.run([sys.executable,str(destination/'reproduce.py')],capture_output=True,text=True,timeout=30)
            self.assertEqual(completed.returncode,0,completed.stderr)
            self.assertEqual(json.loads((destination/'reproduction-verification.json').read_text())['status'],'passed')
            self.assertNotIn('confidence procedure',(destination/'student-worksheet.tex').read_text())
            with self.assertRaisesRegex(ValueError,'already exists'):worker.execute(request)
            original=(destination/'request.json').read_bytes();self.assertEqual(original,wire.encode())


def coverage_experiment():
    # Declared design: normal OLS model, n in {60,240}, 300 repetitions, seed471007.
    # Deliberately wrong baseline divides covariance by n. Monte Carlo SE is
    # sqrt(p*(1-p)/R). This finite experiment checks normalization sensitivity.
    rng=np.random.default_rng(471007);R=300;results=[]
    for n in [60,240]:
        covered=broken=0
        for _ in range(R):
            x=rng.normal(size=n);y=.7+1.5*x+rng.normal(size=n);X=np.column_stack([np.ones(n),x])
            b=np.linalg.lstsq(X,y,rcond=None)[0];e=y-X@b;B=np.linalg.inv(X.T@X);se=np.sqrt(B[1,1]*(e@e)/(n-2));c=stats.t.ppf(.975,n-2)
            request={'schema_version':1,'request_id':'coverage','operation':'inference','model':'ols','dataset':{'columns':['x','y'],'rows':np.column_stack([x,y]).tolist()},'roles':{'target':'y','features':['x']},'inference':{'covariance':'classical'}}
            actual=worker.execute(request)['result']['coefficient_table'][1]
            assert abs(actual['std_error']-se)<1e-10
            covered+=actual['lower']<=1.5<=actual['upper'];broken+=abs(b[1]-1.5)<=c*se/np.sqrt(n)
        p=covered/R;bad=broken/R;mc=np.sqrt(p*(1-p)/R)
        assert abs(p-.95)<4*mc+.015 and p-bad>.5,(n,p,bad,mc)
        results.append({'n':n,'replications':R,'coverage':p,'mc_standard_error':mc,'misnormalized_coverage':bad})
    return results


if __name__=='__main__':
    suite=unittest.defaultTestLoader.loadTestsFromTestCase(ResearchTests)
    result=unittest.TextTestRunner(verbosity=2).run(suite)
    if not result.wasSuccessful():raise SystemExit(1)
    report={'status':'passed','tests':result.testsRun,'covariance_oracle':'Independent NumPy matrix formulas; includes HC0–HC3, cluster CRV1, HAC, logistic score/information and marginal-effect delta method.',
            'coverage_experiment':coverage_experiment(),'scope':'Bounded synthetic tests; not universal inferential validity or causal identification.'}
    (ROOT/'verification-research-numerics.json').write_text(json.dumps(report,indent=2)+'\n')
