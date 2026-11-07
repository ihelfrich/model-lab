"""Check authored structure, numerical anchors and executable stdout questions."""
import contextlib
import io
import json
from pathlib import Path
bank=json.loads(Path(__file__).with_name('questions.json').read_text())
assert len(bank)==100
assert len({(q['stem'],q['code']) for q in bank})==100
executed=0
for item in bank:
    assert item['correct']==sorted(set(item['correct']))
    assert len(item['options'])==4 and all(item['options'])
    assert all(0<=i<4 for i in item['correct'])
    if 'code_stdout' in item:
        stream=io.StringIO()
        with contextlib.redirect_stdout(stream):
            exec(compile(item['code'],f"authored-question-{item['id']}",'exec'),{})
        assert stream.getvalue()==item['code_stdout'],item['title']
        assert item['code_stdout'].strip() in [item['options'][i] for i in item['correct']],item['title']
        executed+=1
assert abs(0.8*0.75*0.4-0.24)<1e-14
assert 6+0.5*(-3)+0.5*(-1.5)==3.75
assert (1+4)/2==2.5
print(json.dumps({'questions':len(bank),'topics':len({q['topic'] for q in bank}),'code_questions':sum(bool(q['code']) for q in bank),'executed_stdout_questions':executed,'status':'selected structural, executable and numeric checks passed'}))
