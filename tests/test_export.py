import csv
import json
import os
from pathlib import Path
import sys
import tempfile
import time
import types
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'lib'))
import swiftbar_preferences as preferences


def day(date, tokens, cost, requests=1, **extra):
    return dict({'date': date, 'totalTokens': tokens, 'inputTokens': tokens - 1, 'outputTokens': 1, 'estimatedCostUsd': cost,
                 'requests': requests, 'unpricedRequests': 0}, **extra)


class ExportTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(); self.addCleanup(self.temp.cleanup)
        env = patch.dict(os.environ, {'OCX_PLUGIN_PATH': str(ROOT / 'starmeter.30s.sh'), 'OCX_CACHE_DIR': self.temp.name,
                                      'OCX_STATE_DIR': self.temp.name + '/state'})
        env.start(); self.addCleanup(env.stop)
        self.m = types.ModuleType('export_test'); exec(compile((ROOT / 'lib/swiftbar_plugin.py').read_text(), 'plugin', 'exec'), self.m.__dict__)
        for key in ('claude', 'magpie'):preferences.update(self.m.STATE_DIR, 'module', key, 'true')
        # Claude Code sent one of its two requests on 11-03 through Magpie: Summary counts that one from Magpie.
        claude = {'summary': {'totalTokens': 30, 'estimatedCostUsd': 3, 'requests': 3}, 'models': [{'model': 'claude-opus-5-5', 'totalTokens': 30, 'requests': 3}],
                  'days': [day('2026-11-03', 20, 2, 2, cacheCreationInputTokens=4, cacheReadInputTokens=6), day('2026-11-04', 10, 1)],
                  'carried': {'Magpie': 1}, 'counted': {'summary': {'totalTokens': 20, 'estimatedCostUsd': 2, 'requests': 2},
                                                        'days': [day('2026-11-03', 10, 1), day('2026-11-04', 10, 1)]}}
        magpie = {'summary': {'totalTokens': 10, 'estimatedCostUsd': 1, 'requests': 1}, 'models': [], 'days': [day('2026-11-03', 10, 1)]}
        os.makedirs(self.m.CACHE_DIR, exist_ok=True)
        record = {'schema': 1, 'fetchedAt': time.time(), 'day': time.strftime('%Y-%m-%d'),
                  'payload': {'range': 'today', 'start': 1762100000, 'end': 1762200000, 'claude': claude, 'magpie': magpie}}
        self.m.write_json_cache(self.m.cache_path(self.m.range_cache_name('today')), record)

    def export(self, kind):
        target = Path(self.temp.name) / ('out.' + kind)
        with patch.object(self.m, 'save_path', return_value=str(target)), patch.object(self.m, 'notify') as told:
            self.m.run_action('export-usage', [kind])
        told.assert_called_once()
        return target

    def test_csv_has_a_row_per_day_and_tool_and_adds_up_to_summary(self):
        target = self.export('csv')
        self.assertTrue(target.read_bytes().startswith(b'\xef\xbb\xbf'), 'Excel reads it as UTF-8')
        rows = list(csv.DictReader(target.read_text(encoding='utf-8-sig').splitlines()))
        self.assertEqual([(r['date'], r['tool']) for r in rows], [('2026-11-03', 'Magpie'), ('2026-11-03', 'Claude'), ('2026-11-04', 'Claude')])
        claude = rows[1]
        self.assertEqual((claude['total_tokens'], claude['cache_read_tokens'], claude['cache_write_tokens'], claude['summary_tokens']), ('20', '6', '4', '10'))
        self.assertEqual(rows[0]['cache_write_tokens'], '', 'Tools that don\'t log cache writes leave it empty')
        self.assertEqual(sum(int(r['summary_tokens']) for r in rows), 30, 'Summary columns count the Magpie request once')

    def test_json_holds_totals_models_and_days(self):
        body = json.loads(self.export('json').read_text())
        self.assertEqual((body['product'], body['range']), ('Starmeter', 'today'))
        claude = next(t for t in body['tools'] if t['id'] == 'claude')
        self.assertEqual((claude['totals']['total_tokens'], claude['summary']['total_tokens']), (30, 20))
        self.assertEqual(claude['models'][0]['model'], 'claude-opus-5-5'); self.assertEqual(len(claude['days']), 2)

    def test_nothing_to_export_before_the_range_is_read_or_when_cancelled(self):
        os.unlink(self.m.cache_path(self.m.range_cache_name('today')))
        with patch.object(self.m, 'dialog_error') as error, patch.object(self.m, 'save_path') as ask:
            self.m.run_action('export-usage', ['csv'])
        error.assert_called_once(); ask.assert_not_called()
        with self.assertRaises(ValueError):self.m.run_action('export-usage', ['xlsx'])


if __name__ == '__main__':
    unittest.main(verbosity=2)
