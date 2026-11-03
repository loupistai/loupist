#!/usr/bin/env python3
"""Prepare a small matched Lens experiment using the existing production runner.

No credential access, model execution, grader answers in suites, or edits to
saved learning. Export only canonical Lens documents from disposable train Loupists.
"""
import argparse
import datetime
import hashlib
import json
from pathlib import Path
import random
import re
import sqlite3


def sha(b):
    return hashlib.sha256(b).hexdigest()


def write(path, obj):
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open('x') as f:
        f.write(json.dumps(obj, ensure_ascii=False, indent=2)+'\n')


def export(train, destination):
    rows = [json.loads(line) for line in (train/'traces.jsonl').read_text().splitlines()]
    assert len(rows) == 4 and all(row['status']=='completed' for row in rows)
    profile = Path(re.search(r'^kept (.+)$', (train/'runner.log').read_text(), re.M)[1])
    db = profile/'workspace/.loupist/loupist.sqlite3'
    c = sqlite3.connect(str(db))
    c.execute('PRAGMA query_only=ON')
    docs=[]; evidence=[]
    ids = set(v['record_id'] for row in rows for v in row['memory_commits'] if v['kind']=='lens')
    for id in sorted(ids):
        title, revision, digest, payload = c.execute('SELECT r.title, v.revision_id, v.revision_digest, v.payload_json FROM knowledge_records r JOIN knowledge_record_heads h ON h.record_id=r.record_id JOIN knowledge_record_revisions v ON v.revision_id=h.revision_id WHERE r.record_id=?', (id,)).fetchone()
        doc=json.loads(payload)
        assert set(doc)=={'markdown','applicability','assumptions','procedure','principles','limitations'}
        assert not re.search(r'(?:rec_|rev_|rsh_|chat-)[a-f0-9]{16}', json.dumps(doc))
        docs.append({'title':title,**doc})
        evidence.append({'training_run_id':rows[-1]['run_id'],'record_id':id,'revision_id':revision,'revision_digest':digest,'payload_sha256':sha(payload.encode()),'export_sha256':sha(json.dumps(docs[-1],sort_keys=True,ensure_ascii=False).encode())})
    assert docs
    result={'lenses':docs,'provenance':evidence,'training_trace_sha256':sha((train/'traces.jsonl').read_bytes()),'training_profile':str(profile)}
    write(destination,result)
    return docs


def dev_cases(repo):
    fixture=json.loads((repo/'apps/loupist/evals/knowledge-lens-eval-v1.json').read_text())
    chosen={'T01','T05','S03','F01','E03','D02','H04','E01'}
    cases=[{'id':'DEV-'+v['id'],'family':v['domain'],'language':v['language'],
            'user':v['question']+'\n\n'+('아래는 가상의 연구 자료이며, 이 자료만 사용해 답해 줘.\n' if v['language']=='ko' else 'Fictional research facts; answer using only these facts.\n')+v['facts'],
            'oracle':{'accuracy':v['judgment']}} for v in fixture['cases'] if v['id'] in chosen]
    cases += [
      {'id':'DEV-MIX','family':'composition','language':'ko','user':'가상 교육 프로그램 결과야. 첫해에는 기초반 168/240명, 심화반 54/60명이 통과했고, 다음 해에는 기초반 36/60명, 심화반 204/240명이 통과했어. 통과율이 오른 것을 프로그램 성과라고 발표해도 될까? 전체와 각 반의 변화, 첫해 반 구성으로 맞춘 다음 해 통과율을 계산하고, 프로그램 효과에 관해 말할 수 있는 범위를 설명해 줘. 두 해의 학생은 다르고 무작위 배정은 없었어. 주어진 자료만 사용해 줘.', 'oracle':{'accuracy':'Overall 74% to 80%, +6pp; basic 70% to 60%, advanced 90% to 85%. First-year weights 80/20 imply year2 65%, -9pp on fixed composition. Overall increase and within-level declines both true. No identified causal program effect.'}},
      {'id':'DEV-RECIPROCAL','family':'reciprocal','language':'en','user':'A fictional rendering service changed from 42 seconds to 30 seconds per identical job on the same single worker, continuously busy with no other bottleneck. A slide says throughput rose by the same percentage as time fell. Compute both changes. Separately, the deployed cluster added workers and changed job complexity; does this prove its throughput gain is due to the software? Use only the supplied facts.', 'oracle':{'accuracy':'Time fell 12/42 =28.5714%; capacity throughput rose 42/30-1=40%. Conditional single worker result does not identify cluster software effect.'}},
      {'id':'DEV-CORRECTION','family':'correction','language':'es','user':'Datos ficticios. El boletín inicial de una fábrica dice: 32 de 80 lotes rechazados, 640 piezas producidas y una tasa de rechazo del 40%. Una fe de erratas cambia únicamente los lotes rechazados a 20; mantiene los 80 lotes y las 640 piezas. El gerente suma ambos boletines como dos estudios independientes y dice que la tasa cayó 15%. Resume el resultado válido, distingue puntos porcentuales de cambio relativo e indica cuántos estudios hay. Usa solo estos datos.', 'oracle':{'accuracy':'Corrected rate 20/80=25%; correction reduces reported rate by15pp or37.5% relative to old40%; not observed temporal improvement. One study, unchanged80 lots and640 pieces preserved.'}},
      {'id':'DEV-SIMPLE','family':'simple','language':'ko','user':'17과 28을 더해 줘. 답만 알려줘.', 'oracle':{'accuracy':'45 only; no audit procedure, research, search, method application, or save needed.'}}
    ]
    return cases


def prepare(root,repo,old,pi_agent_dir):
    training=json.loads((repo/'apps/loupist/evals/lens/training-v1.json').read_text())
    sources=training['scenarios'][0]['seed']['sources']
    curated=json.loads((old/'curated-author/result.json').read_text())
    assert curated['disposition']=='completed'
    learned=[export(old/f'baseline-sol/train-{i}',root/f'frozen/baseline-learned-{i}.json') for i in range(3)]
    write(root/'frozen/training.json',training)
    write(root/'frozen/curated.json',curated)
    cases=dev_cases(repo)
    write(root/'frozen/development.json',{'schema':'lens-quality-cases/1','cases':cases})
    cells=[]
    for trial in range(3):
        order=['A','B','C','D'];random.Random(260026+trial).shuffle(order)
        for condition in order:
            seed={'sources':[] if condition=='A' else sources,'lens':[]}
            if condition=='C':seed['lens']=[curated['body']]
            if condition=='D':seed['lens']=learned[trial]
            scenarios=[{'id':case['id'],'title':case['id'],'tags':['development',case['family']], 'connections':[], 'seed':seed,'workspace_files':{}, 'sessions':[{'id':'test','role':'probe','memory':'off' if condition=='A' else 'read_only','turns':[{'user':case['user']}]}]} for case in cases]
            random.Random(991+trial).shuffle(scenarios)
            suite=root/f'suites/baseline-{condition}-{trial}.json'
            write(suite,{'schema':'loupist-agent-suite/1','suite':'lens-causal-development-v1','version':1,'scenarios':scenarios})
            cells.append({'trial':trial,'condition':condition,'suite':str(suite),'output':str(root/f'baseline/{condition}-{trial}')})
    write(root/'baseline-plan.json',{'concurrency':3,'settings':{'binary':str(root/'binaries/baseline_agent_eval'),'pi_agent_dir':str(pi_agent_dir),'version':'baseline-controlled','model':'openai-codex/gpt-6.1-sol','thinking':'high','turn_timeout_ms':600000},'cells':cells})
    write(root/'protocol.json',{'schema':'lens-causal-protocol/1','created_at':datetime.datetime.now(datetime.timezone.utc).isoformat(), 'primary_contrast':'D-B; candidate minus baseline contrast','secondary_contrasts':['C-B','B-A','D-C'],'conditions':{'A':'Memory off','B':'training Source originals only','C':'same Sources plus independently authored curated Lens','D':'same Sources plus canonical product-learned Lens from corresponding independent training trial'},'trials':3,'trial_unit':'independent research and correction conversation; repeated test answers nested in training trial','planned_development_cells':len(cells)*len(cases),'model':'openai-codex/gpt-6.1-sol','thinking':'high','turn_timeout_ms':600000,'timeout_note':'Identical per-turn operational timeout, not total work budget. No output cap. No automatic paid retries.','test_state':'Fresh Loupist and fresh Chat for every question. Canonical Source/Lens seeding. read_only for B/C/D. No training Chat, Decision, note or workspace file imported.','isolation':'Evaluation-only native file guard and macOS child-shell sandbox, identical across conditions; external connections empty. Exact tool schemas and system prompt unchanged by controls.','quality_dimensions':['accuracy','evidence','method','negative_transfer','correction'],'quality_scale':{'0':'materially wrong or missing','1':'partial with an important omission','2':'correct and sufficient; no unnecessary method requirement'},'blinding':'Judge sees full question, full answer, case oracle, no condition/version/training. Sealed final set authored in tool-less independent provider context; product suites contain questions only.','acceptance':'Use scenario-level paired estimates and cluster bootstrap intervals; report per-trial signs, all attempts and denominator. No success claim if D-B is uncertain or negative. Adopt changes only with observed causal rationale, no material quality regression, and measured context/tool savings or justified quality gain.','limitations':['No human calibration yet. Existing 40-case fixture is development material here; its original not_executed metadata remains historical.','Provider hidden sampling and cross-run cache effects cannot be fixed; exact observed model and configured effort are required.','Sources train general methods already potentially known to the model; ceiling effects must be checked.'],'source_artifacts':str(old)})
    print(json.dumps({'planned_turns':len(cells)*len(cases),'suites':len(cells),'sources_each':len(sources),'learned_lenses':[len(x) for x in learned]}))


if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--root',required=True);p.add_argument('--repo',default='.');p.add_argument('--prior',required=True);p.add_argument('--pi-agent-dir',required=True);a=p.parse_args()
    prepare(Path(a.root).resolve(),Path(a.repo).resolve(),Path(a.prior).resolve(),Path(a.pi_agent_dir).resolve())
