#!/usr/bin/env python3
"""Measure observed provider request bytes and join actual persisted token usage.

Byte counts label specific context components; they are not token estimates.
Only usage reported by the production assistant receipt is called tokens.
"""
import argparse
import collections
import hashlib
import json
from pathlib import Path
import re
import statistics


def size(value):
    return len((value if isinstance(value,str) else json.dumps(value,ensure_ascii=False,separators=(',',':'))).encode())


def current_turn(request):
    for item in reversed(request.get('input') or []):
        if item.get('role')!='user':continue
        content=item.get('content',[])
        if isinstance(content,str):content=[{'text':content}]
        for part in reversed(content):
            text=part.get('text','');match=re.search(r'<current_user_message>\n(.*?)\n</current_user_message>',text,re.S)
            if match:
                envelope=re.search(r'<turn_envelope>\n(.*?)\n</turn_envelope>',text,re.S)
                return match[1],json.loads(envelope[1]) if envelope else {}
    return None,None


def measure(contexts):
    results=[]
    for path in sorted(contexts.glob('request-*.jsonl')):
        requests=[]
        for line in path.read_text().splitlines():
            try:requests.append(json.loads(line))
            except json.JSONDecodeError:raise ValueError(f'Partial request record in {path}')
        turns=collections.defaultdict(list)
        for req in requests:
            user,envelope=current_turn(req)
            cwd=re.search(r'<cwd>\s*(.*?)\s*</cwd>',req.get('instructions',''),re.S)
            if user is not None:turns[(user,cwd[1] if cwd else None,envelope.get('live_time',{}).get('current_utc'))].append((req,envelope))
        for (user,_,turn_at),parts in turns.items():
            first,envelope=parts[0];calls={};unique_results={};current_results=set();additional={}
            for req,_ in parts:
                items=req.get('input') or []
                current_index=max((i for i,item in enumerate(items) if item.get('role')=='user' and '<current_user_message>' in json.dumps(item)),default=-1)
                for i,item in enumerate(items):
                    if item.get('type')=='function_call':calls[item['call_id']]=item
                    if item.get('type')=='function_call_output':
                        unique_results[item['call_id']]=item.get('output','')
                        if i>current_index:current_results.add(item['call_id'])
                    if item.get('type')=='additional_tools':
                        for tool in item.get('tools',[]):additional[tool['name']]=tool
            selected=[]
            for id,output in unique_results.items():
                call=calls.get(id,{})
                try:args=json.loads(call.get('arguments','{}'))
                except json.JSONDecodeError:args={}
                category='lens' if call.get('name')=='lens_read' else 'skill' if call.get('name')=='read' and '/skills/' in args.get('path','') else None
                if category:
                    selected.append({'kind':category,'call_id':id,'path':args.get('path'),'returned_bytes':size(output),'loaded_this_turn':id in current_results})
            normalized=first.get('instructions','')
            cwd=re.search(r'<cwd>\s*(.*?)\s*</cwd>',normalized,re.S)
            results.append({'pid':first['pid'],'capture':str(path),'turn_at':turn_at,'workspace':str(Path(cwd[1]).resolve()) if cwd else None,'user_sha256':hashlib.sha256(user.encode()).hexdigest(),'user':user,'request_count':len(parts),'first_system_bytes':size(normalized),'first_tool_schema_bytes':size(first.get('tools',[])),'automatic_memory_envelope_bytes':size(envelope.get('memory_protocol',{})),'first_user_bytes':size(user),'selected_loads':selected,'selected_unique_result_bytes':sum(v['returned_bytes'] for v in selected),'selected_current_result_bytes':sum(v['returned_bytes'] for v in selected if v['loaded_this_turn']),'additional_tool_schema_unique_bytes':sum(size(v) for v in additional.values()),'captured_request_bytes':sum(size({k:v for k,v in r.items() if k not in ['pid','at']}) for r,_ in parts),'transmitted_system_bytes':sum(size(r.get('instructions','')) for r,_ in parts),'transmitted_tool_schema_bytes':sum(size(r.get('tools',[]))+sum(size(i.get('tools',[])) for i in r.get('input',[]) if i.get('type')=='additional_tools') for r,_ in parts),'first_system_sha256':hashlib.sha256(normalized.encode()).hexdigest()})
    return results


def match_plan(rows,plan_path):
    plan=json.loads(plan_path.read_text());matched=[]
    for cell in plan['cells']:
        profiles={};current=None
        for line in (Path(cell['output'])/'runner.log').read_text().splitlines():
            found=re.match(r'^(\S+) trial=',line)
            if found:current=found[1]
            if line.startswith('kept '):profiles[current]=str((Path(line[5:])/'workspace').resolve())
        suite=json.loads(Path(cell['suite']).read_text())
        for case in suite['scenarios']:
            for session in case['sessions']:
                for i,turn in enumerate(session['turns']):
                    candidates=[r for r in rows if r['workspace']==profiles.get(case['id']) and r['user']==turn['user']]
                    matched.append({'version':cell.get('version',plan['settings'].get('version')),'condition':cell.get('condition',plan['settings'].get('condition')),'trial':cell['trial'],'case_id':case['id'],'session_id':session['id'],'turn_index':i,'measurement':candidates[0] if len(candidates)==1 else None,'matching_captures':len(candidates)})
    summaries=[]
    for version,condition in sorted({(r['version'],r['condition']) for r in matched}):
        group=[r for r in matched if (r['version'],r['condition'])==(version,condition)];observed=[r['measurement'] for r in group if r['measurement']]
        fields=['first_system_bytes','first_tool_schema_bytes','automatic_memory_envelope_bytes','additional_tool_schema_unique_bytes','selected_current_result_bytes','captured_request_bytes','transmitted_system_bytes','transmitted_tool_schema_bytes','request_count']
        summaries.append({'version':version,'condition':condition,'planned':len(group),'matched':len(observed),'means':{k:statistics.mean(r[k] for r in observed) if observed else None for k in fields},'new_loads':{kind:sum(sum(x['kind']==kind and x['loaded_this_turn'] for x in r['selected_loads']) for r in observed) for kind in ['lens','skill']}})
    return {'rows':matched,'summaries':summaries}


if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--contexts',required=True);p.add_argument('--output',required=True);p.add_argument('--plan');a=p.parse_args()
    rows=measure(Path(a.contexts));out=Path(a.output)
    with out.open('x') as f:json.dump({'schema':'lens-context-bytes/2','measurements':rows,'matched_plan':match_plan(rows,Path(a.plan)) if a.plan else None,'note':'Serialized UTF-8 bytes are not tokens. Actual aggregate input/cache/output tokens are in production trace usage. Captures include full system text, user envelope, dynamically loaded schemas, and tool continuations.'},f,ensure_ascii=False,indent=2)
    print(json.dumps({'turns_captured':len(rows),'requests':sum(r['request_count'] for r in rows),'output':str(out)}))
