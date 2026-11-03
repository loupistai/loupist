#!/usr/bin/env python3
"""Verify each disposable test Loupist contains only its declared frozen seeds."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import sqlite3

p=argparse.ArgumentParser();p.add_argument('--plan',required=True);p.add_argument('--output',required=True);a=p.parse_args()
plan=json.loads(Path(a.plan).read_text());checks=[]
for cell in plan['cells']:
    suite=json.loads(Path(cell['suite']).read_text());scenarios={s['id']:s for s in suite['scenarios']}
    out=Path(cell['output']);log=(out/'runner.log').read_text();profiles={};current=None
    for line in log.splitlines():
        m=re.match(r'^(\S+) trial=',line)
        if m:current=m[1]
        if line.startswith('kept '):
            assert current in scenarios
            profiles[current]=Path(line[5:]).resolve()
    for id,scenario in scenarios.items():
        if id not in profiles:
            checks.append({'case_id':id,'condition':cell['condition'],'trial':cell['trial'],'status':'unknown','reason':'no retained completed profile'});continue
        root=profiles[id];db=root/'workspace/.loupist/loupist.sqlite3'
        c=sqlite3.connect(str(db));c.execute('pragma query_only=on')
        counts=dict(c.execute('select kind,count(*) from knowledge_records group by kind').fetchall())
        sources=scenario.get('seed',{}).get('sources',[]);lenses=scenario.get('seed',{}).get('lens',[])
        source_digests=sorted(c.execute('select digest,byte_length from knowledge_source_representations').fetchall())
        expected_source=sorted((hashlib.sha256(s['text'].encode()).hexdigest(),len(s['text'].encode())) for s in sources)
        stored_lenses=c.execute("select r.title,v.payload_json from knowledge_records r join knowledge_record_heads h on h.record_id=r.record_id join knowledge_record_revisions v on v.revision_id=h.revision_id where r.kind='lens'").fetchall()
        observed={title:json.loads(payload) for title,payload in stored_lenses}
        docs_match=all(observed.get(l['title'])=={k:v for k,v in l.items() if k!='title'} for l in lenses)
        notes=c.execute('select count(*) from knowledge_research_records').fetchone()[0]
        chats=[json.loads(r[0]) for r in c.execute('select data_json from chat_sessions').fetchall()]
        messages=[m for chat in chats for m in chat['messages']]
        user_texts=[m['content'] for m in messages if m['role']=='user']
        expected_users=[t['user'] for s in scenario['sessions'] for t in s['turns']]
        # The harness creates one standing perspective even when Memory is off.
        predicates={'source_content_identity':source_digests==expected_source,'lens_document_identity':docs_match,'record_inventory':counts=={'lens':1+len(lenses),**({'source':len(sources)} if sources else {})},'no_notes_or_decisions':notes==0,'only_test_chats':len(chats)==len(scenario['sessions']),'only_test_user_messages':user_texts==expected_users}
        checks.append({'case_id':id,'condition':cell['condition'],'trial':cell['trial'],'profile':str(root),'predicates':predicates,'status':'pass' if all(predicates.values()) else 'fail'})
with Path(a.output).open('x') as f:json.dump({'schema':'lens-state-audit/1','checks':checks},f,ensure_ascii=False,indent=2)
print(json.dumps({'planned':len(checks),'passed':sum(c['status']=='pass' for c in checks),'failed':sum(c['status']=='fail' for c in checks),'unknown':sum(c['status']=='unknown' for c in checks)}))
raise SystemExit(0 if all(c['status']=='pass' for c in checks) else 1)
