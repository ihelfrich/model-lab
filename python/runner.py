"""Run the chosen local student script and trace its executed source lines."""
import json
import runpy
import sys
import time
import traceback
from pathlib import Path

script = Path(sys.argv[1]).resolve()
sys.path.insert(0, str(script.parent))
last_emit = 0.0
last_line = None
break_lines = {i for i, line in enumerate(script.read_text().splitlines(), 1)
               if line.strip().startswith(("tree =", "residuals =", "report("))}


def trace(frame, event, arg):
    global last_emit, last_line
    if frame.f_code.co_filename == str(script) and frame.f_code.co_name == "<module>" and event == "line":
        now = time.monotonic()
        if now - last_emit > 0.025 or (frame.f_lineno in break_lines and frame.f_lineno != last_line) or last_line is None:
            last_emit, last_line = now, frame.f_lineno
            names = ("round_no", "learning_rate", "max_depth", "n_estimators", "sample_fraction")
            visible = {k: frame.f_locals[k] for k in names if isinstance(frame.f_locals.get(k), (int, float))}
            print("@LAB@" + json.dumps({"kind": "line", "line": frame.f_lineno, "locals": visible}), flush=True)
    return trace


try:
    sys.settrace(trace)
    runpy.run_path(str(script), run_name="__main__")
except BaseException:
    sys.settrace(None)
    traceback.print_exc()
    sys.exit(1)
finally:
    sys.settrace(None)
print("@LAB@" + json.dumps({"kind": "done"}), flush=True)
