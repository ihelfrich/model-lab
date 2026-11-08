"""Bounded native launch/render/close regression for the macOS KVO abort.

Exercises the real viewport Close path after all nine models and seven views.
This verifies that path, not every OS version or accessibility configuration.
Uses synthetic examples only; does not read a user's datasets or messages.
The binary must be built with `--features acceptance`; shipped builds omit the flags.
"""
import argparse
import json
from pathlib import Path
import platform
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parent

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--binary', default=str(ROOT/'dist/Model Lab 0.7.app/Contents/MacOS/tree-ensemble-lab'))
    parser.add_argument('--runs', type=int, default=10)
    args = parser.parse_args()
    if platform.system() != 'Darwin' or not 1 <= args.runs <= 20:
        raise SystemExit('Use macOS and 1–20 bounded runs.')
    results = []
    with tempfile.TemporaryDirectory(prefix='model-lab-native-check-') as folder:
        folder = Path(folder)
        for index in range(args.runs):
            report = folder/'report.json'
            if report.exists():
                report.unlink()
            cmd = [args.binary, '--workbench-ui-smoke-test', '--report', str(report), '--screenshot', str(folder/'preview.png')]
            if index % 2:
                cmd.append('--compact-workbench')
            completed = subprocess.run(cmd, capture_output=True, text=True, timeout=60)
            if completed.returncode != 0:
                raise RuntimeError(f'Native lifecycle run {index+1} exited {completed.returncode}: {completed.stderr[-4000:]}')
            value = json.loads(report.read_text())
            assert value['status'] == 'passed' and len(value['models_rendered']) == 9
            assert len(value['views_rendered']) == 8 and value['screenshot']
            results.append({'run': index+1, 'compact': bool(index % 2), 'exit_code': 0, 'native_frames': value['native_frames']})
            print(f'Native launch/render/close {index+1}/{args.runs}: passed', flush=True)
    result = {'status':'passed', 'macos':platform.mac_ver()[0], 'architecture':platform.machine(), 'runs':results,
              'scope':'Real native viewport-close path; nine models and seven views; synthetic data; Mac accessibility adapter disabled.'}
    (ROOT/'verification-macos-lifecycle.json').write_text(json.dumps(result, indent=2)+'\n')

if __name__ == '__main__':
    main()
