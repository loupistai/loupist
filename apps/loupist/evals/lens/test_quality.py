import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

spec=importlib.util.spec_from_file_location('quality',Path(__file__).with_name('quality.py'))
q=importlib.util.module_from_spec(spec);spec.loader.exec_module(q)


class QualityContracts(unittest.TestCase):
    def fixture(self,root,condition='D',trace=None):
        out=root/condition;out.mkdir()
        suite=root/f'{condition}.json'
        suite.write_text(json.dumps({'scenarios':[{'id':'case','sessions':[{'id':'test','role':'probe','memory':'read_only','turns':[{'user':'What is 2+3?'}]}]}]}))
        if trace is not None:(out/'traces.jsonl').write_text(json.dumps(trace)+'\n')
        return {'trial':0,'condition':condition,'suite':str(suite),'output':str(out)}

    def trace(self,**overrides):
        return {'scenario_id':'case','session_id':'test','turn_index':0,'user':'What is 2+3?','status':'completed','observed_model':'p/m','model':{'thinking':'high'},'memory_mode':'read_only','memory_commits':[],'answer':'5','tool_calls':[],'usage':{'input':3,'output':1,'cache_read':0,'cache_write':0},'duration_ms':12,**overrides}

    def plan(self,root,cells):
        path=root/'plan.json';path.write_text(json.dumps({'settings':{'version':'baseline','model':'p/m','thinking':'high'},'cells':cells}));return path

    def test_missing_and_failed_remain_in_original_denominator(self):
        with tempfile.TemporaryDirectory() as d:
            root=Path(d);a=self.fixture(root,'D');b=self.fixture(root,'B',self.trace(status='failed',observed_model=None,answer='',usage=None))
            rows,attempts=q.collect(root,self.plan(root,[a,b]))
            self.assertEqual(len(rows),2);self.assertEqual(len(attempts),2)
            self.assertIsNone(rows[0]['trace']);self.assertEqual(rows[1]['trace']['status'],'failed')

    def test_completed_model_mismatch_and_memory_write_are_rejected(self):
        for override in [{'observed_model':'p/other'},{'memory_commits':[{'kind':'lens'}]}]:
            with self.subTest(override=override),tempfile.TemporaryDirectory() as d:
                root=Path(d);cell=self.fixture(root,trace=self.trace(**override))
                with self.assertRaises(AssertionError):q.collect(root,self.plan(root,[cell]))

    def test_blind_packet_contains_full_answer_but_no_condition_metadata(self):
        with tempfile.TemporaryDirectory() as d:
            root=Path(d);answer='5. '+'evidence '*900
            cells=[self.fixture(root,'D',self.trace(answer=answer)),self.fixture(root,'B',self.trace())]
            cases=root/'cases.json';cases.write_text(json.dumps({'cases':[{'id':'case','user':'What is 2+3?','oracle':{'accuracy':'5'}}]}))
            dest=root/'grade';q.packets(root,[self.plan(root,cells)],cases,dest,'p/judge')
            request=json.loads(next((dest/'requests').glob('*.json')).read_text())
            packet=json.loads(request['messages'][1]['content'])
            self.assertEqual(set(packet),{'question','oracle','answers'})
            self.assertIn(answer,[a['answer'] for a in packet['answers']])
            self.assertTrue(all(set(a)=={'id','answer'} for a in packet['answers']))
            self.assertEqual(len(json.loads((dest/'records.json').read_text())['rows']),2)

    def test_crossed_cluster_interval_preserves_trial_and_scenario_uncertainty(self):
        constant=[{'case_id':s,'trial':t,'delta':1} for s in ['x','y'] for t in range(3)]
        self.assertEqual(q.bootstrap_pairs(constant),[1,1])
        opposite=[{'case_id':s,'trial':t,'delta':1 if s=='x' else -1} for s in ['x','y'] for t in range(3)]
        low,high=q.bootstrap_pairs(opposite)
        self.assertLess(low,0);self.assertGreater(high,0)
        self.assertIsNone(q.bootstrap_pairs([]))

    def test_independent_training_draws_do_not_gain_false_pairing_precision(self):
        before={(s,t):t for s in ['x','y'] for t in range(3)}
        after={(s,t):t+1 for s in ['x','y'] for t in range(3)}
        paired=[{'case_id':s,'trial':t,'delta':1} for s,t in before]
        self.assertEqual(q.bootstrap_pairs(paired),[1,1])
        low,high=q.bootstrap_independent_versions(before,after)
        self.assertLess(low,1);self.assertGreater(high,1)
        self.assertEqual(q.bootstrap_independent_versions({('x',0):2},{('x',0):2}),[0,0])
        self.assertIsNone(q.bootstrap_independent_versions({},after))

    def test_identical_packets_reuse_judgment_without_double_counting_usage(self):
        with tempfile.TemporaryDirectory() as d:
            root=Path(d);cell=self.fixture(root,'D',self.trace())
            cases=root/'cases.json';cases.write_text(json.dumps({'cases':[{'id':'case','user':'What is 2+3?','oracle':{'accuracy':'5'}}]}))
            dest=root/'grade';q.packets(root,[self.plan(root,[cell,{**cell,'trial':1}])],cases,dest,'p/judge')
            metas=q.read_json(dest/'packets.json');self.assertEqual(len(metas),2)
            self.assertEqual(len(list((dest/'requests').glob('*.json'))),1)
            score={'id':'answer-1','dimensions':{k:{'score':2,'reason':'Correct'} for k in q.DIMENSIONS},'numeric_errors':[]}
            q.dump(dest/'judgments'/metas[0]['key']/'result.json',{'disposition':'completed','body':{'scores':[score],'oracle_issue':None},'usage':{'input':7}})
            report=root/'report.json';q.summarize(dest,report);body=q.read_json(report)
            self.assertEqual(body['summaries'][0]['graded'],2)
            self.assertEqual(body['judge_usage'],[{'input':7}])

    def test_difference_in_differences_removes_general_ability_change(self):
        with tempfile.TemporaryDirectory() as d:
            root=Path(d);dest=root/'grade';rows=[];metas=[]
            for version in ['before','after']:
                for condition in ['B','D']:
                    identity=version+condition;score=1 if version=='before' else 2
                    row={'version':version,'condition':condition,'trial':0,'case_id':'case','identity':identity,'trace':self.trace()}
                    rows.append(row);metas.append({'key':identity,'labels':{'answer-1':identity},'case_id':'case','trial':0})
                    q.dump(dest/'judgments'/identity/'result.json',{'disposition':'completed','body':{'scores':[{'id':'answer-1','dimensions':{k:{'score':score,'reason':'fixture'} for k in q.DIMENSIONS},'numeric_errors':[]}],'oracle_issue':None},'usage':None})
            q.dump(dest/'records.json',{'rows':rows,'attempts':[]});q.dump(dest/'packets.json',metas)
            report=root/'report.json';q.summarize(dest,report);body=q.read_json(report)
            effect=next(e for e in body['version_effects'] if e['contrast']=='D-B' and e['dimension']=='accuracy')
            self.assertEqual(effect['mean_delta'],0)
            direct=next(e for e in body['version_effects'] if e['contrast']=='D' and e['dimension']=='accuracy')
            self.assertEqual(direct['mean_delta'],1)


if __name__=='__main__':unittest.main()
