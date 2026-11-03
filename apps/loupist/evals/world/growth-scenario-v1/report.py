#!/usr/bin/env python3
"""Render recorded evidence; never infer a successful mutation from answer text."""
import argparse
import collections
import hashlib
import json
from pathlib import Path


def usage_total(rows):
    totals = collections.Counter()
    unknown = 0
    for row in rows:
        usage = row.get('usage')
        if not isinstance(usage, dict):
            unknown += 1
            continue
        for key, value in usage.items():
            if isinstance(value, (int, float)) and not isinstance(value, bool):
                totals[key] += value
    return dict(totals), unknown


def write_report(rows, source, output):
    text = ['# Recorded growth scenario', '',
            f'Raw session evidence: `{source}`.', '',
            'All inputs are fictional; model `openai-codex/gpt-6.1-sol`, effort `medium`. '
            'Automatic method learning is off in sessions 1–2 and enabled before session 3. '
            'No method or World is manually seeded. Costs are not estimated.', '']
    previous = {}
    for row in rows:
        s = row['snapshot']
        counts = {**{f'node:{k}': v for k, v in s['nodes_by_kind'].items()},
                  **{f'edge:{k}': v for k, v in s['edges_by_kind'].items()},
                  'Map nodes': len(s['knowledgeMap']['nodes']),
                  'Map edges': len(s['knowledgeMap']['edges']),
                  'principles': len(s['principles']), 'methods': s['nodes_by_kind'].get('lens_method', 0),
                  'Journal entries': s['journal_count'], 'briefing items': len(s['inbox']),
                  'growth events': len(s['growth_events']),
                  'learning reviews': len(s['learning_reviews'])}
        text += [f"## {row['session']}", '', '| Measure | Total | Delta |', '| --- | ---: | ---: |']
        for key in sorted(set(previous) | set(counts)):
            value = counts.get(key, 0)
            text.append(f'| {key} | {value} | {value - previous.get(key, 0):+d} |')
        previous = counts
        chat, unknown_chat = usage_total(row['turns'])
        helpers, unknown_helpers = usage_total(row['helper_inference'])
        text += ['', f'Chat/briefing usage: `{json.dumps(chat, sort_keys=True)}` '
                 f'({unknown_chat} calls without usage).', '',
                 f'World/learning helper usage: `{json.dumps(helpers, sort_keys=True)}` '
                 f'over {len(row["helper_inference"])} calls ({unknown_helpers} without usage).', '',
                 f'World status: `{row["processing"].get("status") if isinstance(row["processing"], dict) else "not requested"}`; '
                 f'post-session processing: `{row["post_processing"].get("status")}`.', '']
        for turn in row['turns']:
            text += [f'Turn status: `{turn.get("status")}`; observed model: `{turn.get("observed_model")}`.', '']
        if s['learning_reviews']:
            text += ['Learning review receipts:', '']
            for review in s['learning_reviews']:
                text.append(f'- {review["status"]}: {review.get("rationale") or review.get("reason") or "No rationale"} (evidence: {review["evidenceCount"]}).')
            text.append('')
    final = rows[-1]['snapshot']
    revisions = [e for e in final['growth_events'] if e['kind'] == 'lens_revised']
    text += ['## Then / Now revision evidence', '']
    if not revisions:
        text += ['No `lens_revised` events were recorded. This scenario starts without a method and '
                 'provides one learning batch; creation alone cannot produce Then / Now revision history. '
                 'This is a scenario coverage gap, not evidence that the revision UI failed.', '']
    for event in reversed(revisions):
        text += [f'- At `{event["atMs"]}`: {event["summary"]}',
                 f'  - Then: {event.get("beforeSummary")}', f'  - Now: {event.get("afterSummary")}',
                 f'  - Rationale: {event.get("rationale")}', '']
    text += ['## Session 5 answer', '', rows[-1].get('final_answer') or '(No final answer.)', '',
             '## Verification boundaries', '',
             '- Source dates are fictional publication dates; acquisition times are the actual run times.',
             '- Journal count is expected to stay zero: no user judgment was requested.',
             '- Matching wording is not enough to prove a saved principle or learned method; inspect the receipts and memory reads in the raw trace.',
             '- The preview remaps opaque IDs and uniformly shifts stored timestamps so the final acquisition fits the fixed preview clock; publication dates and data are unchanged.', '']
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text('\n'.join(text))


def write_preview(rows, source, output):
    s = rows[-1]['snapshot']
    # Allowlist only the data needed by the renderer fixture, not tool arguments,
    # environment details, full chats, runtime paths, or provider diagnostics.
    payload = {key: s[key] for key in ['knowledgeMap', 'growth_events', 'learning_reviews', 'principles', 'methods', 'inbox']}
    payload = json.loads(json.dumps(payload))
    for document in payload['methods']:
        node = next(n for n in payload['knowledgeMap']['nodes'] if n.get('recordId') == document['record']['recordId'])
        document['lens'].update(title=document['record']['title'],
                                lensKind=node['kind'].removeprefix('lens_'),
                                isStanding=node['kind'] == 'lens_perspective',
                                readOnly=False, archived=node.get('archived', False))
    ids = {}
    def stable(value):
        if value not in ids:
            ids[value] = f'growth-{len(ids) + 1:04d}'
        return ids[value]
    def id_key(key):
        return key.endswith('Id') or key.endswith('_id') or key in {'id', 'source', 'target'}
    def visit(value, key=''):
        if isinstance(value, dict):
            return {k: visit(v, k) for k, v in value.items()}
        if isinstance(value, list):
            return [visit(v, key) for v in value]
        if isinstance(value, str) and id_key(key):
            return stable(value)
        return value
    payload = visit(payload)
    # The standalone preview freezes time; retain relative observed timing.
    frozen = 1793145600000  # 2026-10-28T00:00:00Z
    dates = [n.get('firstSeenAtMs', 0) for n in payload['knowledgeMap']['nodes']]
    latest = max(dates + [payload['knowledgeMap']['generatedAtMs']])
    shift = frozen - latest
    def shift_times(value, key=''):
        if isinstance(value, dict):
            return {k: shift_times(v, k) for k, v in value.items()}
        if isinstance(value, list):
            return [shift_times(v, key) for v in value]
        if isinstance(value, int) and (key.endswith('AtMs') or key in {'atMs', 'created_at_ms', 'updated_at_ms'}):
            return value + shift
        return value
    payload = shift_times(payload)
    payload['fixturePending'] = False
    payload['sessions'] = [{'id': row['session'], 'nodes': len(row['snapshot']['knowledgeMap']['nodes']), 'edges': len(row['snapshot']['knowledgeMap']['edges'])} for row in rows]
    payload['provenance'] = {'schema': 'loupist-growth-preview/1', 'scenario': 'growth-scenario-v1',
                             'traceSha256': hashlib.sha256(source.read_bytes()).hexdigest(),
                             'session': rows[-1]['session'], 'timestampShiftMs': shift,
                             'model': rows[-1]['model'], 'thinking': rows[-1]['thinking']}
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(payload, indent=2, ensure_ascii=False) + '\n')


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('trace', type=Path)
    parser.add_argument('--report', required=True, type=Path)
    parser.add_argument('--preview', required=True, type=Path)
    args = parser.parse_args()
    rows = [json.loads(line) for line in args.trace.read_text().splitlines() if line.strip()]
    if [r['session'] for r in rows] != [f'session-{n}' for n in range(1, 6)]:
        raise SystemExit('Expected exactly five ordered session snapshots; refusing partial preview')
    write_report(rows, args.trace, args.report)
    write_preview(rows, args.trace, args.preview)

if __name__ == '__main__':
    main()
