import os
from pathlib import Path
import sys
import tempfile
import types
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'lib'))
import swiftbar_preferences as preferences


def usage(models, **extra):
    """A source's figures with these models: (name, tokens, cost, requests)."""
    rows = [{'model': name, 'totalTokens': tokens, 'inputTokens': tokens - 10, 'outputTokens': 10, 'cachedInputTokens': 5,
             'estimatedCostUsd': cost, 'requests': requests, 'unpricedRequests': 0} for name, tokens, cost, requests in models]
    summary = {key: sum(row[key] for row in rows) for key in ('totalTokens', 'estimatedCostUsd', 'requests', 'unpricedRequests')}
    return dict({'summary': summary, 'models': rows, 'days': [], 'hours': []}, **extra)


class GroupingTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(); self.addCleanup(self.temp.cleanup)
        env = patch.dict(os.environ, {'OCX_PLUGIN_PATH': str(ROOT / 'starmeter.30s.sh'), 'OCX_CACHE_DIR': self.temp.name,
                                      'OCX_STATE_DIR': self.temp.name + '/state'})
        env.start(); self.addCleanup(env.stop)
        self.m = types.ModuleType('grouping_test'); exec(compile((ROOT / 'lib/swiftbar_plugin.py').read_text(), 'plugin', 'exec'), self.m.__dict__)

    def test_one_card_per_model_across_tools_counted_as_summary_counts(self):
        # Claude Code sent one Opus request through Magpie; Summary counts it from Magpie, so it counts once here too.
        claude = usage([('claude-opus-5-5-20261101', 100, 1.0, 2)], carried={'Magpie': 1},
                       counted=usage([('claude-opus-5-5-20261101', 50, 0.5, 1)]))
        magpie = usage([('anthropic/claude-opus-5-5', 50, 0.5, 1), ('gpt-6.2-sol', 30, 2.0, 1)])
        selected = [('magpie', 'Magpie', magpie), ('claude', 'Claude', claude)]
        cards = self.m.model_cards(selected, self.m.summary_usage(selected))
        self.assertEqual([c['name'] for c in cards], ['gpt-6.2-sol', 'Claude Opus 5.5'], 'Cards go by cost')
        opus = cards[1]
        self.assertEqual((opus['tokens'], opus['requests'], opus['cost']), (100, 2, 1.0))
        self.assertEqual(opus['id'], 'model:claude-opus-5-5'); self.assertEqual(opus['accent'], 'claude')
        self.assertEqual([(t['name'], t['share']) for t in opus['models']], [('Magpie', 0.5), ('Claude', 0.5)])
        self.assertEqual(cards[0]['accent'], 'codex'); self.assertEqual(cards[0]['sessions'], 0)

    def test_unpriced_models_say_so(self):
        selected = [('codex', 'ChatGPT', usage([('new-model', 10, 0, 2)]))]
        selected[0][2]['models'][0]['unpricedRequests'] = 2
        card = self.m.model_cards(selected, self.m.summary_usage(selected))[0]
        self.assertIsNone(card['cost']); self.assertEqual(card['status'], '费用待定价'); self.assertEqual(card['symbol'], 'cpu')

    def test_one_card_per_project_from_the_clients_own_records(self):
        claude = usage([('claude-opus-5-5', 100, 1.0, 2)], projects=[{'project': 'starmeter', 'totalTokens': 60, 'estimatedCostUsd': 0.6, 'requests': 1},
                                                                     {'project': 'notes', 'totalTokens': 40, 'estimatedCostUsd': 0.4, 'requests': 1}])
        codex = usage([('gpt-6.2-sol', 30, 2.0, 1)], projects=[{'project': 'starmeter', 'totalTokens': 30, 'estimatedCostUsd': 2.0, 'requests': 1}])
        gemini = usage([('gemini-3-pro', 5, 0.1, 1)])
        magpie = usage([('claude-opus-5-5', 50, 0.5, 1)])
        cards = self.m.project_cards([('magpie', 'Magpie', magpie), ('claude', 'Claude', claude), ('codex', 'ChatGPT', codex), ('gemini', 'Gemini', gemini)])
        self.assertEqual([c['name'] for c in cards], ['starmeter', 'notes', '未标注项目'], 'Gateways are left out; tools without projects go last')
        star = cards[0]
        self.assertEqual((star['tokens'], star['requests'], star['cost']), (90, 2, 2.6))
        self.assertEqual([t['name'] for t in star['models']], ['Claude', 'ChatGPT']); self.assertEqual(star['accent'], 'claude')
        self.assertEqual(star['symbol'], 'folder'); self.assertEqual(cards[2]['models'][0]['name'], 'Gemini')

    def test_every_project_has_its_card(self):
        projects=[{'project':'p%02d'%i,'totalTokens':100-i,'estimatedCostUsd':(100-i)/100,'requests':1} for i in range(12)]
        claude=usage([('claude-opus-5-5',1,1.0,12)],projects=projects)
        cards=self.m.project_cards([('claude','Claude',claude)])
        self.assertEqual([c['name'] for c in cards],['p%02d'%i for i in range(12)],'None folded into "其他": the panel lists five and the rest a tap away')

    def test_readers_keep_only_the_project_folder_name(self):
        import json
        import swiftbar_sources as sources
        from datetime import timezone
        root = Path(self.temp.name) / 'codex'; root.mkdir()
        usage_counts = {'total_tokens': 50, 'input_tokens': 40, 'cached_input_tokens': 0, 'output_tokens': 10}
        rows = [{'type': 'session_meta', 'payload': {'id': 's', 'cwd': '/Users/someone/Code/starmeter/'}}, {'type': 'turn_context', 'payload': {'model': 'gpt-6.2-sol'}},
                {'type': 'event_msg', 'timestamp': 100, 'payload': {'type': 'token_count', 'info': {'total_token_usage': usage_counts, 'last_token_usage': usage_counts}}}]
        (root / 'a.jsonl').write_text(''.join(json.dumps(row) + '\n' for row in rows))
        result = sources.jsonl_usage('codex', root, Path(self.temp.name) / 'cache', 0, 1e9, timezone.utc)
        self.assertEqual([(r['project'], r['requests']) for r in result['projects']], [('starmeter', 1)])
        for db in (Path(self.temp.name) / 'cache').glob('*.sqlite3'):self.assertNotIn(b'/Users/someone', db.read_bytes())
        self.assertEqual(sources.project_name('C:\\Users\\me\\proj'), 'proj'); self.assertIsNone(sources.project_name(''))

    def test_claude_records_keep_their_project(self):
        import json
        from datetime import datetime
        from zoneinfo import ZoneInfo
        root = Path(self.temp.name) / 'projects'; root.mkdir()
        def row(id, cwd):
            return {'type': 'assistant', 'timestamp': '2026-11-04T01:00:00Z', 'sessionId': 's', 'requestId': 'r' + id, 'cwd': cwd,
                    'message': {'id': id, 'model': 'claude-opus-5-5', 'usage': {'input_tokens': 10, 'output_tokens': 2}}}
        (root / 'a.jsonl').write_text(''.join(json.dumps(r) + '\n' for r in (row('m1', '/Users/someone/Code/starmeter'), row('m2', None))))
        result = self.m.claude_usage_records(root, str(Path(self.temp.name) / 'c.db'), datetime(2026, 11, 4, 12, tzinfo=ZoneInfo('Asia/Taipei')), 'today')[0]
        self.assertEqual(sorted((r['project'], r['requests']) for r in result['projects']), [('', 1), ('starmeter', 1)])
        self.assertNotIn(b'/Users/someone', (Path(self.temp.name) / 'c.db').read_bytes())

    def test_the_choice_is_saved_and_always_allowed(self):
        self.assertEqual(preferences.read(self.m.STATE_DIR)['groupBy'], 'tool')
        self.m.run_action('set-group', ['model'])
        self.assertEqual(preferences.read(self.m.STATE_DIR)['groupBy'], 'model')
        with self.assertRaises(ValueError):self.m.run_action('set-group', ['nothing'])

    def test_counted_parts_keep_their_models(self):
        import swiftbar_sources as sources
        from datetime import timezone
        events = [{'id': str(i), 'timestamp': 100 + i, 'model': 'm', 'input': 10, 'output': 1, 'cached': 0, 'writes': 0} for i in range(2)]
        result = sources.aggregate(events, 0, 1e9, timezone.utc, carriers=[('Magpie', {(10, 0, 0, 1): [100]})])
        self.assertEqual(result['counted']['models'][0]['requests'], 1)
        self.assertEqual(self.m.add_usage([result['counted'], result['carriedParts']['Magpie']])['models'][0]['requests'], 2)


if __name__ == '__main__':
    unittest.main(verbosity=2)
