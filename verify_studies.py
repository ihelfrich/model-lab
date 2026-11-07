"""Independent finite tests for the actual Model Lab 0.8 study worker.

Design: Gaussian generators; n=80/320, R=80, seeds 471008/471009.
Check biases against known targets with MC uncertainty, and reject deliberately
shifted coefficients / misnormalized intervals. No student data are used.
"""
import copy
import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock
import numpy as np

ROOT=Path(__file__).resolve().parent
sys.path.insert(0,str(ROOT/'python'))
import study_worker as worker


def spec(kind='heteroskedasticity',replications=4,n=200):
    methods=[{'id':'classical','label':'OLS classical','model':'ols','operation':'inference','covariance':'classical','oracle':False},
             {'id':'robust','label':'OLS HC3','model':'ols','operation':'inference','covariance':'HC3','oracle':False}]
    if kind=='clustering':methods[1].update(id='cluster',label='OLS cluster',covariance='cluster')
    if kind=='confounding':
        methods[0].update(id='observed',label='Observed regression',covariance='HC3')
        methods[1].update(id='oracle',label='Simulator-only oracle',oracle=True)
    return {'version':1,'aim':'Check the declared coefficient target and inferential assumptions.',
            'estimand':'structural_effect'if kind=='confounding'else'regression_coefficient',
            'scenario':{'kind':kind,'version':1,'n':n,'groups':20,'group_size':10,
                        'parameters':{'beta':1.,'tau':1.,'sigma':1.,'h':1.,'c':1.,'gamma':1.,'rho':.5}},
            'methods':methods,'replications':replications,'seed':471008,'max_fits':1000,'seconds':300}


def request(folder,s=None,operation='simulate',identity='study'):
    s=copy.deepcopy(s or spec())
    return {'schema_version':2,'request_id':'test-'+identity,'study_id':identity,'operation':operation,
            'study_spec':s,'spec_wire':json.dumps(s,indent=2),'project_root':str(folder),
            'checkpoint_path':str(Path(folder)/'test.studies'/f'{identity}.json')}


class StudyTests(unittest.TestCase):
    def test_frozen_worker_and_structured_preflight(self):
        self.assertEqual(hashlib.sha256((ROOT/'python/research_worker.py').read_bytes()).hexdigest(),worker.FROZEN_V07)
        with tempfile.TemporaryDirectory()as f:
            q=request(f,spec(replications=200),operation='study_preflight')
            self.assertEqual(worker.execute(q)['result']['planned_backend_fits'],800)
            q['study_spec']['replications']=500;q['spec_wire']=json.dumps(q['study_spec'])
            with self.assertRaisesRegex(ValueError,'No fits were started'):worker.execute(q)
            self.assertFalse(Path(q['checkpoint_path']).exists())
        for value in [True,3.5,0,501]:
            s=spec();s['replications']=value
            with self.assertRaises(ValueError):worker.validate_spec(s)
        s=spec();s['methods'][0]['model']='forest'
        with self.assertRaisesRegex(ValueError,'OLS'):worker.validate_spec(s)

    def test_generators_and_zero_limits(self):
        s=spec();s['scenario']['n']=2000;s['scenario']['parameters']['h']=0
        d=worker.dataset(worker.validate_spec(s),0)[0];a=np.array(d['rows'])
        rng=worker.seed_for(worker.validate_spec(s),0,'training');x,u,nu,e=rng.normal(size=(4,2000))
        np.testing.assert_allclose(a[:,1],1+x+e,atol=1e-14)
        self.assertLess(abs(np.mean(a[:,1]-1-x)),.08)
        s=spec('clustering');s['scenario']['parameters']['rho']=0
        d=worker.dataset(worker.validate_spec(s),0)[0]
        rng=worker.seed_for(worker.validate_spec(s),0,'training');x,u,nu,e=rng.normal(size=(4,200))
        np.testing.assert_allclose(np.array([r[2]for r in d['rows']]),1+x+e)
        for kind in ['heteroskedasticity','confounding','clustering']:
            s=worker.validate_spec(spec(kind));a=worker.dataset(s,2)[0]
            self.assertEqual(worker.digest(a),worker.digest(worker.dataset(s,2)[0]))
            self.assertNotEqual(worker.digest(a),worker.digest(worker.dataset(s,3)[0]))

    def test_source_wire_and_path_validation(self):
        with tempfile.TemporaryDirectory()as folder:
            q=request(folder);q['study_spec']['seed']+=1
            with self.assertRaisesRegex(ValueError,'wire'):worker.execute(q)
            q=request(folder);q['checkpoint_path']=str(Path(folder).parent/'escaped.studies/study.json')
            with self.assertRaisesRegex(ValueError,'inside'):worker.execute(q)

    def test_actual_fit_accounting_and_independent_ols(self):
        with tempfile.TemporaryDirectory()as folder:
            q=request(folder);doc=worker.execute(q)['result']
            self.assertEqual(doc['status'],'completed')
            self.assertEqual(sum(r['actual_fits']for r in doc['reservations']),16)
            self.assertEqual(doc['records'][0]['methods'][0]['estimate'],doc['records'][0]['methods'][1]['estimate'])
            values=np.array(worker.dataset(worker.validate_spec(q['study_spec']),0)[0]['rows'])
            X=np.column_stack([np.ones(len(values)),values[:,0]]);y=values[:,1]
            B=np.linalg.inv(X.T@X);beta=np.linalg.solve(X.T@X,X.T@y);residual=y-X@beta
            h=np.einsum('ij,jk,ik->i',X,B,X)
            covariance=B@(X.T@(X*(residual/(1-h))[:,None]**2))@B
            m=doc['records'][0]['methods'][1]
            self.assertAlmostEqual(m['estimate'],beta[1],places=10)
            self.assertAlmostEqual(m['std_error'],np.sqrt(covariance[1,1]),places=10)
            self.assertEqual(doc['summary']['methods'][0]['coverage_valid_intervals']['n'],4)

    def test_failures_remain_in_denominators(self):
        original=worker.run_method
        def failed(spec,method,replicate,data,charge):
            if replicate==0 and method.id=='robust':raise ValueError('Deliberate numerical failure')
            return original(spec,method,replicate,data,charge)
        with tempfile.TemporaryDirectory()as folder,mock.patch.object(worker,'run_method',failed):
            doc=worker.execute(request(folder))['result']
        row=doc['summary']['methods'][1]
        self.assertEqual(row['failed'],1);self.assertEqual(row['completed'],3)
        self.assertEqual(row['coverage_valid_intervals']['n'],3)
        self.assertEqual(row['valid_and_covering']['n'],4)
        self.assertEqual(doc['summary']['paired'][0]['estimate_difference']['n'],3)

    def test_interruption_resume_and_conservative_reservations(self):
        with tempfile.TemporaryDirectory()as folder:
            q=request(folder)
            def interrupt(event):
                if event.get('kind')=='checkpoint':
                    d=worker.read_document(Path(q['checkpoint_path']))
                    if len(d['reservations'])==1:raise KeyboardInterrupt()
            with self.assertRaises(KeyboardInterrupt):worker.execute(q,interrupt)
            d=worker.read_document(Path(q['checkpoint_path']));self.assertEqual(d['reservations'][0]['status'],'running')
            q['resume']=True;resumed=worker.execute(q)['result']
            self.assertEqual(resumed['reservations'][0]['status'],'uncertain')
            self.assertEqual(resumed['reservations'][0]['slots'],2)
            clean=worker.execute(request(folder,identity='clean'))['result']
            self.assertEqual(resumed['records'],clean['records'])
            altered=copy.deepcopy(q);altered['study_spec']['seed']+=1;altered['spec_wire']=json.dumps(altered['study_spec'])
            with self.assertRaisesRegex(ValueError,'identical'):worker.execute(altered)
            d=worker.read_document(Path(q['checkpoint_path']));d['provenance']['packages']['numpy']='different';worker.write_document(Path(q['checkpoint_path']),d,lambda e:None)
            with self.assertRaisesRegex(ValueError,'identical'):worker.execute(q)

    def test_deadline_and_checkpoint_integrity(self):
        with tempfile.TemporaryDirectory()as folder:
            q=request(folder);q['study_spec']['seconds']=1;q['spec_wire']=json.dumps(q['study_spec'])
            with mock.patch.object(worker.time,'monotonic',side_effect=[0,2]):d=worker.execute(q)['result']
            self.assertEqual(d['status'],'partial');self.assertEqual(d['records'],[])
            path=Path(q['checkpoint_path']);data=json.loads(path.read_bytes());data['document_sha256']='wrong';path.write_text(json.dumps(data))
            with self.assertRaisesRegex(ValueError,'integrity'):worker.read_document(path)

    def test_regeneration_and_audience_safe_exports(self):
        with tempfile.TemporaryDirectory()as folder:
            q=request(folder,spec('confounding'));doc=worker.execute(q)['result']
            inspect={**q,'operation':'study_inspect','replicate':1,'method_id':'oracle'}
            result=worker.execute(inspect)['result'];self.assertTrue(result['verified']);self.assertIsNone(result['optimizer_trace'])
            self.assertNotIn('u',result['observed_data']['columns'])
            for audience in ['student','research']:
                destination=Path(folder)/audience
                worker.execute({**q,'operation':'study_export','audience':audience,'export_path':str(destination),'replicate':1})
                if audience=='student':
                    self.assertEqual(set(p.name for p in destination.iterdir()),{'observed-data.csv','worksheet.txt','student-worksheet.tex','README.txt','manifest.json'})
                    self.assertNotIn('u',(destination/'observed-data.csv').read_text().splitlines()[0].split(','))
                else:
                    done=subprocess.run([sys.executable,'-I','-B',str(destination/'reproduce.py')],capture_output=True,text=True,timeout=45)
                    self.assertEqual(done.returncode,0,done.stderr)
                    self.assertEqual(json.loads((destination/'reproduction-verification.json').read_text())['status'],'passed')
                with self.assertRaisesRegex(ValueError,'existing'):worker.execute({**q,'operation':'study_export','audience':audience,'export_path':str(destination)})

    def test_predictive_information_sets_and_training_only_transforms(self):
        s=spec('clustering',replications=2)
        s['methods']=[{'id':model,'label':model,'model':model,'operation':'predict','covariance':'none','oracle':False}for model in ['ols','ridge','forest']]
        typed=worker.validate_spec(s);data=worker.dataset(typed,0)[0]
        r,result,analysis=worker.run_method(typed,typed.methods[1],0,data,lambda:None)
        self.assertIsNone(r['estimate']);self.assertIsNone(r['lower'])
        self.assertEqual(set(r['prediction_mse']),{'existing_groups','new_groups'})
        expected=np.array([row[0]for row in data['rows']]).mean()
        self.assertAlmostEqual(result['preprocessing']['design_mean'][0],expected)
        self.assertEqual(len(result['preprocessing']['training_rows']),200)
        with tempfile.TemporaryDirectory()as f:
            doc=worker.execute(request(f,s))['result']
            self.assertEqual(doc['summary']['methods'][2]['coverage_valid_intervals']['n'],0)

    def test_outcome_unit_scaling_and_intercept_target(self):
        s=worker.validate_spec(spec());data=worker.dataset(s,0)[0]
        a,_,_=worker.run_method(s,s.methods[1],0,data,lambda:None)
        changed=copy.deepcopy(data)
        for row in changed['rows']:row[-1]*=2
        b,_,_=worker.run_method(s,s.methods[1],0,changed,lambda:None)
        for key in ['estimate','std_error','lower','upper']:self.assertAlmostEqual(b[key],2*a[key],places=10)
        s=spec('clustering',replications=80);s['estimand']='regression_intercept'
        with tempfile.TemporaryDirectory()as f:
            doc=worker.execute(request(f,s))['result']
        classical,cluster=doc['summary']['methods']
        self.assertGreater(cluster['coverage_valid_intervals']['value']-classical['coverage_valid_intervals']['value'],.15)
        self.assertEqual(worker.dataset(worker.validate_spec(s),0)[2],'Intercept')


def sensitivity_experiment():
    results=[]
    with tempfile.TemporaryDirectory()as folder:
        for seed in [471008,471009]:
            for n in [80,320]:
                s=spec('confounding',replications=80,n=n);s['seed']=seed
                doc=worker.execute(request(folder,s,identity=f'seed{seed}-n{n}'))['result']
                observed,oracle=doc['summary']['methods']
                assert abs(observed['bias']['value']-.5)<5*observed['bias']['mc_se']+.03
                assert abs(oracle['bias']['value'])<5*oracle['bias']['mc_se']+.02
                values=[m for r in doc['records']for m in r['methods']if m['method_id']=='oracle']
                # A deliberately wrong coefficient and artificially narrowed
                # intervals must be detected by the same evaluation quantities.
                wrong_bias=worker.mean_mc([v['estimate']+.5-v['truth']for v in values])
                assert wrong_bias['value']>5*wrong_bias['mc_se']
                broken=sum(abs(v['estimate']-v['truth'])<=(v['upper']-v['lower'])/(2*np.sqrt(n))for v in values)/len(values)
                coverage=oracle['coverage_valid_intervals']['value'];assert coverage-broken>.5
                results.append({'seed':seed,'n':n,'replications':80,'observed_bias':observed['bias'],'oracle_bias':oracle['bias'],'coverage':oracle['coverage_valid_intervals'],'misnormalized_coverage':broken})
        for parameter in ['c','gamma']:
            s=spec('confounding',replications=80,n=320);s['scenario']['parameters'][parameter]=0
            d=worker.execute(request(folder,s,identity='zero-'+parameter))['result']['summary']['methods'][0]
            assert abs(d['bias']['value'])<5*d['bias']['mc_se']+.02
    return results


if __name__=='__main__':
    result=unittest.TextTestRunner(verbosity=2).run(unittest.defaultTestLoader.loadTestsFromTestCase(StudyTests))
    if not result.wasSuccessful():raise SystemExit(1)
    report={'status':'passed','test_groups':result.testsRun,'design':'n=80/320, R=80, seeds 471008/471009; independent normal variables; c=gamma=1; zero-c/gamma sensitivity; paired methods.',
            'sensitivity':sensitivity_experiment(),'scope':'Finite synthetic checks; not universal inferential validity, causal identification or learning effectiveness.'}
    (ROOT/'verification-studies.json').write_text(json.dumps(report,indent=2)+'\n')
