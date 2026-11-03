import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

spec=importlib.util.spec_from_file_location('metrics',Path(__file__).with_name('context-metrics.py'))
m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)


class ContextMeasurements(unittest.TestCase):
    def test_dynamic_schemas_and_history_are_accounted_separately(self):
        def user(text):return {'role':'user','content':[{'text':f'<turn_envelope>\n{{"memory_protocol":{{"active":true}}}}\n</turn_envelope>\n<current_user_message>\n{text}\n</current_user_message>'}]}
        old_call={'type':'function_call','call_id':'old','name':'lens_read','arguments':'{}'}
        old_result={'type':'function_call_output','call_id':'old','output':'old method'}
        new_call={'type':'function_call','call_id':'new','name':'read','arguments':'{"path":"/skills/lens/SKILL.md"}'}
        new_result={'type':'function_call_output','call_id':'new','output':'current skill'}
        tool={'type':'function','name':'lens_update','parameters':{'type':'object'}}
        base={'pid':123,'at':'now','model':'test','instructions':'<cwd>\n/tmp/example/workspace\n</cwd>','tools':[],'input':[user('old'),old_call,old_result,user('new')]}
        continued={**base,'input':base['input']+[new_call,new_result,{'type':'additional_tools','tools':[tool]}]}
        with tempfile.TemporaryDirectory() as d:
            p=Path(d);(p/'request-123.jsonl').write_text('\n'.join(json.dumps(x) for x in [base,continued])+'\n')
            rows=m.measure(p)
            self.assertEqual(len(rows),1);row=rows[0]
            self.assertEqual(row['request_count'],2)
            self.assertEqual(row['selected_current_result_bytes'],len('current skill'))
            self.assertEqual(row['selected_unique_result_bytes'],len('current skillold method'))
            self.assertEqual(row['additional_tool_schema_unique_bytes'],m.size(tool))
            self.assertEqual(row['transmitted_tool_schema_bytes'],4+m.size([tool]))
            self.assertEqual(row['workspace'],str(Path('/tmp/example/workspace').resolve()))


if __name__=='__main__':unittest.main()
