#!/usr/bin/env python3
"""Freeze newly trained Lens revisions and create matched test plans."""
import importlib.util
import argparse
import json
from pathlib import Path
import random

spec=importlib.util.spec_from_file_location('prepare',Path(__file__).with_name('prepare-experiment.py'))
prep=importlib.util.module_from_spec(spec);spec.loader.exec_module(prep)
p=argparse.ArgumentParser();p.add_argument('root');p.add_argument('--version',default='candidate-v1');p.add_argument('--include-controlled-baseline',action='store_true');a=p.parse_args()
root=Path(a.root).resolve()
base=json.loads((root/'baseline-plan.json').read_text())
new_baseline=[];candidate=[]
versions=([('baseline-controlled','baseline-controlled-training','baseline_agent_eval')] if a.include_controlled_baseline else [])+[(a.version,a.version+'-training',a.version.replace('-','_')+'_agent_eval')]
for version,training,binary in versions:
    learned=[prep.export(root/f'{training}/train-{i}',root/f'frozen/{version}-learned-{i}.json') for i in range(3)]
    for cell in base['cells']:
        if version=='baseline-controlled' and cell['condition']!='D':continue
        trial=cell['trial'];condition=cell['condition'];suite=json.loads(Path(cell['suite']).read_text())
        if condition=='D':
            for scenario in suite['scenarios']:scenario['seed']['lens']=learned[trial]
        suite_path=root/f'suites/{version}-{condition}-{trial}.json'
        prep.write(suite_path,suite)
        item={'trial':trial,'condition':condition,'version':version,'binary':str(root/'binaries'/binary),'suite':str(suite_path),'output':str(root/f'{version}-development/{condition}-{trial}')}
        (new_baseline if version=='baseline-controlled' else candidate).append(item)
settings={k:v for k,v in base['settings'].items() if k not in ['version','binary']}
if new_baseline:
    prep.write(root/'baseline-primary-plan.json',{'concurrency':3,'settings':base['settings'],'cells':[c for c in base['cells'] if c['condition']!='D']+new_baseline})
prep.write(root/(a.version+'-plan.json'),{'concurrency':3,'settings':{**settings,'version':a.version,'binary':str(root/'binaries'/(a.version.replace('-','_')+'_agent_eval'))},'cells':candidate})
cells=new_baseline+candidate;random.Random(2026102601).shuffle(cells)
prep.write(root/('development-round1-plan.json' if new_baseline else a.version+'-run-plan.json'),{'concurrency':3,'settings':settings,'cells':cells})
print(json.dumps({'new_baseline_cells':len(new_baseline),'candidate_cells':len(candidate),'new_test_turns':12*len(cells)}))
