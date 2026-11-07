"""Public Engel workflow: independent NumPy + base R, actual Rust project/export.

Reference estimates are computation checks, never labelled population truth.
Requires --binary; uses the project's installed statsmodels dataset, no download.
"""
import argparse
import csv
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import numpy as np
from scipy import stats
from statsmodels.datasets import engel

ROOT=Path(__file__).resolve().parent

def main():
    parser=argparse.ArgumentParser();parser.add_argument('--binary',required=True);parser.add_argument('--r-reference');args=parser.parse_args()
    frame=engel.load_pandas().data;rows=frame[['income','foodexp']].to_numpy().tolist()
    reference=[];results=[]
    data_sha=hashlib.sha256(json.dumps({'columns':['income','foodexp'],'rows':rows},sort_keys=True,separators=(',',':')).encode()).hexdigest()
    with tempfile.TemporaryDirectory(prefix='model-lab-engel-')as folder:
        folder=Path(folder);data=folder/'engel.csv';frame.to_csv(data,index=False)
        rscript=shutil.which('Rscript')
        if rscript:
            completed=subprocess.run([rscript,'--vanilla',str(ROOT/'verify_engel.R'),str(data),str(folder/'r.csv')],capture_output=True,text=True,timeout=30)
            if completed.returncode:raise RuntimeError(completed.stderr)
            r_version=completed.stdout.strip();r_path=folder/'r.csv'
        elif args.r_reference:
            r_path=Path(args.r_reference);metadata=json.loads(r_path.with_suffix('.json').read_text())
            if metadata['data_sha256']!=data_sha or metadata['reference_sha256']!=hashlib.sha256(r_path.read_bytes()).hexdigest():raise RuntimeError('R fixture data/source identity mismatch.')
            r_version=metadata['r_version']+' (previously executed fixture; R not rerun on this host)'
        else:raise RuntimeError('R is unavailable; supply the checked R reference fixture.')
        with r_path.open(newline='')as f:r_rows=list(csv.DictReader(f))
        for degree in [1,2]:
            label='linear'if degree==1 else'quadratic'
            q={'schema_version':1,'request_id':'engel-'+label,'operation':'inference','model':'ols','seed':471008,
               'dataset':{'name':'Public Engel household expenditure reference','columns':['income','foodexp'],'rows':rows},
               'roles':{'target':'foodexp','features':['income']},'config':{'degree':degree,'standardize':True},'inference':{'covariance':'HC3','alpha':.05}}
            request=folder/(label+'.json');request.write_text(json.dumps(q));report=folder/(label+'-report.json')
            process=subprocess.run([args.binary,'--research-project-check',str(request),'--report',str(report)],capture_output=True,text=True,timeout=120)
            if process.returncode:raise RuntimeError(process.stderr+' '+(report.read_text()if report.exists()else''))
            response=json.loads(report.read_text());assert response['project_restored']and response['exported_reproduction']
            table=response['response']['result']['coefficient_table'];x=frame['income'].to_numpy();raw=np.column_stack([x**j for j in range(1,degree+1)])
            center=raw.mean(axis=0);scale=raw.std(axis=0);Z=np.column_stack([np.ones(len(x)),(raw-center)/scale]);y=frame['foodexp'].to_numpy()
            beta=np.linalg.lstsq(Z,y,rcond=None)[0];B=np.linalg.inv(Z.T@Z);h=np.einsum('ij,jk,ik->i',Z,B,Z);scores=Z*((y-Z@beta)/(1-h))[:,None]
            C=B@(scores.T@scores)@B;J=np.eye(degree+1);J[0,1:]=-center/scale;J[1:,1:]=np.diag(1/scale);est=J@beta;se=np.sqrt(np.diag(J@C@J.T));critical=stats.t.ppf(.975,len(x)-degree-1)
            numpy=np.column_stack([est,se,est-critical*se,est+critical*se]);actual=np.array([[r[k]for k in ['estimate','std_error','lower','upper']]for r in table]);R=np.array([[float(r[k])for k in ['estimate','std_error','lower','upper']]for r in r_rows if r['model']==label])
            np.testing.assert_allclose(actual,numpy,rtol=1e-7,atol=1e-9);np.testing.assert_allclose(actual,R,rtol=1e-7,atol=1e-9)
            results.append({'model':label,'observations':len(x),'max_numpy_error':float(np.max(abs(actual-numpy))),'max_r_error':float(np.max(abs(actual-R))),'project_restored':True,'exported_reproduction':True})
        if rscript:
            destination=ROOT/'reference/engel-r.csv';destination.parent.mkdir(exist_ok=True);shutil.copyfile(r_path,destination)
            destination.with_suffix('.json').write_text(json.dumps({'r_version':r_version,'data_sha256':data_sha,'reference_sha256':hashlib.sha256(destination.read_bytes()).hexdigest(),'r_source_sha256':hashlib.sha256((ROOT/'verify_engel.R').read_bytes()).hexdigest()},indent=2)+'\n')
    report={'status':'passed','dataset':'statsmodels Engel / quantreg reference data','scope':'Public observed-data computational workflow; no population truth or causal-effect claim. Human research and teaching sessions remain pending.','r_version':r_version,'models':results}
    (ROOT/'verification-engel.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report))

if __name__=='__main__':main()
