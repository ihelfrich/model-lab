"""Assemble Model Lab 0.8 with a relocatable, signed scientific Python runtime.

Run through the project's uv environment after the release binary is built.
Earlier application bundles and release archives are never modified.
"""
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import stat
import subprocess
import sys
import sysconfig

ROOT=Path(__file__).resolve().parent.parent
APP=ROOT/'dist/Model Lab 0.8.app'
RUNTIME=APP/'Contents/Resources/Python'
MARKER=APP/'Contents/Resources/scientific-runtime.json'


def main():
    if sys.platform!='darwin' or platform.machine()!='arm64':
        raise SystemExit('This package target is macOS Apple silicon.')
    binary=Path(os.environ.get('CARGO_TARGET_DIR',str(ROOT/'target')))/'release/tree-ensemble-lab'
    if not binary.is_file():raise SystemExit('Build the release executable first.')
    (APP/'Contents/MacOS').mkdir(parents=True,exist_ok=True)
    (APP/'Contents/Resources').mkdir(parents=True,exist_ok=True)
    shutil.copy2(binary,APP/'Contents/MacOS/tree-ensemble-lab')
    shutil.copy2(ROOT/'packaging/Info.plist',APP/'Contents/Info.plist')
    manifest={'python':platform.python_version(),'architecture':platform.machine(),
              'lock_sha256':hashlib.sha256((ROOT/'uv.lock').read_bytes()).hexdigest(),'frozen_runtime':True}
    previous=json.loads(MARKER.read_text())if MARKER.is_file()else None
    if previous!=manifest or not (RUNTIME/'bin/python3').is_file():
        if RUNTIME.exists():
            for directory in [RUNTIME,*[p for p in RUNTIME.rglob('*')if p.is_dir()and not p.is_symlink()]]:directory.chmod(directory.stat().st_mode|stat.S_IWUSR)
            shutil.rmtree(RUNTIME)
        prefix=Path(sys.base_prefix).resolve()
        ignore=shutil.ignore_patterns('__pycache__','*.pyc','*.pyo')
        shutil.copytree(prefix,RUNTIME,symlinks=True,ignore=ignore)
        site=RUNTIME/'lib'/f'python{sys.version_info.major}.{sys.version_info.minor}'/'site-packages'
        shutil.copytree(Path(sysconfig.get_paths()['purelib']),site,dirs_exist_ok=True,symlinks=True,ignore=ignore)
        (site/'model_lab_runtime.pth').write_text('import sys; sys.dont_write_bytecode = True\n')
        for link in RUNTIME.rglob('*'):
            if not link.is_symlink():continue
            target=Path(os.readlink(link))
            if target.is_absolute():
                try:relative=target.resolve().relative_to(prefix)
                except ValueError:raise RuntimeError(f'Unportable absolute runtime symlink: {link.relative_to(RUNTIME)}')
                link.unlink();link.symlink_to(os.path.relpath(RUNTIME/relative,link.parent))
            if not link.resolve().is_relative_to(RUNTIME.resolve()):
                raise RuntimeError(f'Runtime symlink points outside the bundle: {link.relative_to(RUNTIME)}')
        native=[]
        magic={b'\xcf\xfa\xed\xfe',b'\xfe\xed\xfa\xcf',b'\xca\xfe\xba\xbe',b'\xbe\xba\xfe\xca'}
        for path in RUNTIME.rglob('*'):
            if path.is_symlink()or not path.is_file():continue
            if path.suffix not in ('.so','.dylib')and not(path.parent==RUNTIME/'bin'and path.name.startswith('python')):continue
            with path.open('rb')as f:header=f.read(4)
            if header in magic:native.append(path)
        for path in native:
            path.chmod(path.stat().st_mode|stat.S_IWUSR)
            subprocess.run(['codesign','--force','--sign','-',str(path)],check=True,capture_output=True,timeout=20)
        # Frozen runtime contents stay unchanged after signing. Startup -B and
        # the .pth also avoid bytecode writes; permissions cover early imports.
        for path in RUNTIME.rglob('*'):
            if path.is_symlink():continue
            path.chmod(0o555 if path.is_dir()or path.stat().st_mode&0o111 else 0o444)
        RUNTIME.chmod(0o555)
        MARKER.write_text(json.dumps(manifest,indent=2)+'\n')
        print(f'Copied and signed scientific runtime ({len(native)} native components).',flush=True)
    subprocess.run(['codesign','--force','--sign','-',str(APP)],check=True,capture_output=True,timeout=90)
    subprocess.run(['codesign','--verify','--deep','--strict',str(APP)],check=True,capture_output=True,timeout=60)
    probe=subprocess.run([str(RUNTIME/'bin/python3'),'-I','-B','-c',
        'import json,sys,numpy,scipy,pandas,sklearn,statsmodels,matplotlib;print(json.dumps({"prefix":sys.prefix,"python":sys.version.split()[0]}))'],
        check=True,capture_output=True,text=True,timeout=60)
    details=json.loads(probe.stdout)
    if Path(details['prefix']).resolve()!=RUNTIME.resolve():raise RuntimeError('Runtime prefix did not relocate into the application bundle.')
    subprocess.run(['codesign','--verify','--deep','--strict',str(APP)],check=True,capture_output=True,timeout=60)
    print(f'Built {APP.name}; bundled Python {details["python"]} verified.',flush=True)


if __name__=='__main__':main()
