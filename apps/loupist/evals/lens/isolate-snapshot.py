#!/usr/bin/env python3
"""Install matched, evaluation-only file controls in a copied source tree.

Never apply to the product checkout. Native calculations retain their production
schema. File tools stay inside the clean workspace. There is no shell tool.
No model prompt or tool schema changes.
"""
import argparse
import hashlib
import json
from pathlib import Path

p = argparse.ArgumentParser()
p.add_argument('--snapshot', required=True)
p.add_argument('--context-output', required=True)
p.add_argument('--prior-context-output', required=True)
a = p.parse_args()
root = Path(a.snapshot).resolve()
if '/evals/output/' not in str(root):
    raise SystemExit('An ignored evaluation snapshot is required')
agent = root/'apps/loupist/agent'
contexts = Path(a.context_output).resolve()
contexts.mkdir(parents=True, exist_ok=False)
files = {}

def change(name, old, new):
    path = agent/name
    before = path.read_text()
    if before.count(old) != 1:
        raise SystemExit(f'Expected one patch site in {name}')
    after = before.replace(old, new)
    path.write_text(after)
    files[name] = {'before': hashlib.sha256(before.encode()).hexdigest(),
                   'after': hashlib.sha256(after.encode()).hexdigest()}

change('loupist-extension.mjs', 'import { isDeepStrictEqual } from "node:util";', 'import { isDeepStrictEqual } from "node:util";\nimport * as evalFs from "node:fs";\nimport * as evalPath from "node:path";')

# The product wrapper is deliberately preserved in the checkout. This patch is
# part of the matched experiment control, not a candidate product improvement.
change('loupist-extension.mjs', '      originalPi.registerTool(tool);', '''      originalPi.registerTool({ ...tool, execute(id, input, ...rest) {
        const allowed = new Set(['read','write','edit','run_python','ls','find','grep','capability_load','world_search','world_read','lens_search','lens_read','world_job','conversation_memory','run_results_list','artifact_list','artifact_read','lens_file','source_save','note_save','chat_history']);
        if (!allowed.has(tool.name)) throw new Error('Evaluation control: this operation is unavailable.');
        if (tool.name === 'capability_load' && input.id !== 'memory-save')
          throw new Error('Evaluation control: only Memory capabilities are available.');
        if (['read','write','edit','ls','find','grep'].includes(tool.name)) {
          const pathModule = evalPath;
          const fsModule = evalFs;
          const folder = fsModule.realpathSync(hostContext.workspace.root);
          const skills = pathModule.join(pathModule.dirname(new URL(import.meta.url).pathname), 'skills');
          const requested = pathModule.resolve(folder, input.path ?? '.');
          let actual;
          try { actual = fsModule.realpathSync(requested); }
          catch { actual = pathModule.join(fsModule.realpathSync(pathModule.dirname(requested)), pathModule.basename(requested)); }
          const inside = (p, root) => p === root || p.startsWith(root + '/');
          if ((!inside(actual, folder) && !(tool.name === 'read' && inside(actual, skills))) || inside(actual, folder + '/.loupist'))
            throw new Error('Evaluation control: files outside the clean workspace and bundled Skill reads are unavailable.');
        }
        return tool.execute(id, input, ...rest);
      }});''')
# Replace the old observer destination without changing the captured fields.
path = agent/'loupist-extension.mjs'
s = path.read_text()
old = str(Path(a.prior_context_output).resolve())
if s.count(old) != 1:
    raise SystemExit('Expected the existing request observer')
path.write_text(s.replace(old, str(contexts)))
files['loupist-extension.mjs']['after'] = hashlib.sha256(path.read_bytes()).hexdigest()
(root.parent/'isolation.json').write_text(json.dumps({'schema':'lens-eval-isolation/1','snapshot':str(root),'files':files,'network':'no shell; run_python has no network; no external connections','file_policy':'fresh workspace except .loupist; bundled Skill reads; system runtimes','model_inputs_changed':False},indent=2)+'\n')
print(json.dumps({'snapshot':str(root),'patched':list(files)}))
