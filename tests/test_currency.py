import io
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


class Response(io.BytesIO):
    def __enter__(self):return self
    def __exit__(self, *_):self.close()


class CurrencyTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(); self.addCleanup(self.temp.cleanup)
        env = patch.dict(os.environ, {'OCX_PLUGIN_PATH': str(ROOT / 'starmeter.30s.sh'), 'OCX_CACHE_DIR': self.temp.name,
                                      'OCX_STATE_DIR': self.temp.name + '/state'})
        env.start(); self.addCleanup(env.stop)
        self.m = types.ModuleType('currency_test'); exec(compile((ROOT / 'lib/swiftbar_plugin.py').read_text(), 'plugin', 'exec'), self.m.__dict__)

    def test_dollars_by_default_and_nothing_is_read(self):
        self.assertIsNone(self.m.currency_unit({'exchange_rate': {'rate': 7.1, 'date': '2026-11-17'}}))
        self.assertNotIn('exchange_rate', self.m.external_loaders())

    def test_yuan_waits_for_a_rate_then_follows_the_day_or_the_users(self):
        with patch.object(self.m, 'schedule_background_refresh'):self.m.run_action('set-currency', ['CNY'])
        self.assertIn('exchange_rate', self.m.external_loaders())
        self.assertIsNone(self.m.currency_unit({}), 'No rate yet: estimates stay in dollars rather than guess')
        unit = self.m.currency_unit({'exchange_rate': {'rate': 6.6661, 'date': '2026-11-17'}})
        self.assertEqual((unit['symbol'], unit['rate']), ('¥', 6.6661)); self.assertIn('2026-11-17', unit['note'])
        with patch.object(self.m, 'text_dialog', return_value='7.2'), patch.object(self.m, 'schedule_background_refresh'):
            self.m.run_action('set-cny-rate', [])
        self.assertEqual(self.m.currency_unit({})['rate'], 7.2); self.assertNotIn('exchange_rate', self.m.external_loaders())
        with patch.object(self.m, 'text_dialog', return_value='70'), patch.object(self.m, 'dialog_error') as error:
            self.m.run_action('set-cny-rate', [])
        error.assert_called_once(); self.assertEqual(preferences.read(self.m.STATE_DIR)['cnyRate'], 7.2)

    def test_the_day_rate_comes_from_frankfurter_with_nothing_about_the_user(self):
        seen = []
        def opener(request, timeout):
            seen.append(request);return Response(json.dumps({'base': 'USD', 'date': '2026-11-17', 'rates': {'CNY': 6.6661}}).encode())
        with patch.object(self.m.urllib.request, 'urlopen', opener):
            self.assertEqual(self.m.exchange_rate_load(), ({'rate': 6.6661, 'date': '2026-11-17'}, None))
        self.assertTrue(seen[0].full_url.startswith('https://api.frankfurter.dev/'))
        with patch.object(self.m.urllib.request, 'urlopen', lambda *a, **k: Response(b'{"rates":{"CNY":900}}')):
            self.assertIsNone(self.m.exchange_rate_load()[0])

    def test_the_menu_bar_figure_and_the_dashboard_follow_the_choice(self):
        preferences.update(self.m.STATE_DIR, 'currency', 'CNY'); preferences.update(self.m.STATE_DIR, 'cny-rate', '7')
        preferences.update(self.m.STATE_DIR, 'menu-bar', 'cost'); preferences.update(self.m.STATE_DIR, 'module', 'claude', 'true')
        os.makedirs(self.m.CACHE_DIR, exist_ok=True)
        record = {'schema': 1, 'fetchedAt': time.time(), 'day': time.strftime('%Y-%m-%d'),
                  'payload': {'range': 'today', 'claude': {'summary': {'estimatedCostUsd': 2.5, 'requests': 1}}}}
        self.m.write_json_cache(self.m.cache_path(self.m.range_cache_name('today')), record)
        self.assertEqual(self.m.menu_bar_value({}), '¥17.50')
        self.assertEqual(self.m.dashboard_payload({'range_usage': record['payload'], 'displayed_range': 'today'})['currency']['symbol'], '¥')
        rows = next(o for o in self.m.module_options() if o['label'] == '外观')['children']
        row = next(r for r in rows if r['label'] == '金额显示')
        self.assertEqual(row['value'], '人民币'); self.assertEqual(row['children'][3]['value'], '7.0000 · 手动')


if __name__ == '__main__':
    unittest.main(verbosity=2)
