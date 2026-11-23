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


class PaybackTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(); self.addCleanup(self.temp.cleanup)
        env = patch.dict(os.environ, {'OCX_PLUGIN_PATH': str(ROOT / 'starmeter.30s.sh'), 'OCX_CACHE_DIR': self.temp.name,
                                      'OCX_STATE_DIR': self.temp.name + '/state'})
        env.start(); self.addCleanup(env.stop)
        self.m = types.ModuleType('payback_test'); exec(compile((ROOT / 'lib/swiftbar_plugin.py').read_text(), 'plugin', 'exec'), self.m.__dict__)
        for key in ('claude-subscription', 'claude'):preferences.update(self.m.STATE_DIR, 'module', key, 'true')

    def rows(self, key='claude-subscription'):
        return {row.split(' | ')[0].lstrip('-'): row for row in self.m.payback_lines(key)}

    def month(self, cost, unpriced=0):
        os.makedirs(self.m.CACHE_DIR, exist_ok=True)
        record = {'schema': 1, 'fetchedAt': time.time(), 'day': time.strftime('%Y-%m-%d'), 'payload': {'range': 'month', 'claude': {'summary': {'estimatedCostUsd': cost, 'unpricedRequests': unpriced, 'requests': 9}}}}
        self.m.write_json_cache(self.m.cache_path(self.m.range_cache_name('month')), record)

    def test_without_a_price_only_the_price_row_shows(self):
        rows = self.rows()
        self.assertEqual(list(rows), ['订阅月费…']); self.assertIn('badge=未填写', rows['订阅月费…'])
        self.assertEqual(self.m.payback_lines('minimax'), [], 'Subscriptions without a tool of their own have nothing to weigh')

    def test_the_month_is_weighed_against_the_price(self):
        preferences.update(self.m.STATE_DIR, 'subscription-price', 'claude-subscription', '200')
        self.assertIn('badge=待同步', self.rows()['本月回本'])
        self.month(1640.5, unpriced=2)
        row = self.rows()['本月回本']
        self.assertIn('badge="8.2 倍"', row); self.assertIn('$1640.50', row); self.assertIn('2 次未定价', row)
        self.assertIn('badge=$200.00/月', self.rows()['订阅月费…'])
        self.month(60); self.assertIn('badge="0.3 倍"', self.rows()['本月回本'])

    def alone(self, usage):
        os.makedirs(self.m.CACHE_DIR, exist_ok=True)
        record = {'schema': 1, 'fetchedAt': time.time(), 'payload': {'range': 'month', 'claude': usage}}
        self.m.write_json_cache(self.m.cache_path(self.m.PAYBACK_MONTH + 'claude'), record)

    def test_a_tool_off_the_homepage_is_weighed_from_its_own_month(self):
        # 本月回本 doesn't ask for the tool on the homepage: its month is read on its own, in the background.
        preferences.update(self.m.STATE_DIR, 'subscription-price', 'claude-subscription', '200')
        preferences.update(self.m.STATE_DIR, 'module', 'claude', 'false')
        self.assertIn('payback-month-claude', self.m.external_loaders())
        self.assertNotIn('month', self.m.usage_ranges_due())
        self.assertIn('badge=待同步', self.rows()['本月回本'])
        self.assertNotIn('Harness', ' '.join(self.rows().values()))
        self.alone({'summary': {'estimatedCostUsd': 500, 'unpricedRequests': 0, 'requests': 9}})
        self.assertIn('badge="2.5 倍"', self.rows()['本月回本'])
        # No records of the tool this month: why, not 待同步 for ever.
        self.alone({'error': '未找到 Claude 用量记录'})
        self.assertIn('未找到 Claude 用量记录', self.rows()['本月回本'])
        # Back on the homepage, the month's own figures again, and no read of its own.
        preferences.update(self.m.STATE_DIR, 'module', 'claude', 'true')
        self.assertNotIn('payback-month-claude', self.m.external_loaders())
        self.month(100); self.assertIn('badge="0.5 倍"', self.rows()['本月回本'])
        # Without the subscription on the homepage, or without a price, nothing is read for it.
        preferences.update(self.m.STATE_DIR, 'module', 'claude', 'false')
        preferences.update(self.m.STATE_DIR, 'module', 'claude-subscription', 'false')
        self.assertNotIn('payback-month-claude', self.m.external_loaders())

    def test_a_price_keeps_this_month_fresh_in_the_background(self):
        self.assertNotIn('month', self.m.usage_ranges_due())
        preferences.update(self.m.STATE_DIR, 'subscription-price', 'claude-subscription', '200')
        self.assertIn('month', self.m.usage_ranges_due())

    def test_the_dialog_takes_dollars_and_an_empty_answer_clears(self):
        with patch.object(self.m, 'text_dialog', return_value=' $1,200/月 '), patch.object(self.m, 'schedule_background_refresh'):
            self.m.run_action('set-subscription-price', ['claude-subscription'])
        self.assertEqual(preferences.read(self.m.STATE_DIR)['subscriptionPrices'], {'claude-subscription': 1200.0})
        with patch.object(self.m, 'text_dialog', return_value='abc'), patch.object(self.m, 'dialog_error') as error, patch.object(self.m, 'schedule_background_refresh'):
            self.m.run_action('set-subscription-price', ['claude-subscription'])
        error.assert_called_once()
        with patch.object(self.m, 'text_dialog', return_value=''), patch.object(self.m, 'schedule_background_refresh'):
            self.m.run_action('set-subscription-price', ['claude-subscription'])
        self.assertEqual(preferences.read(self.m.STATE_DIR)['subscriptionPrices'], {})
        with self.assertRaises(ValueError):self.m.run_action('set-subscription-price', ['minimax'])


if __name__ == '__main__':
    unittest.main(verbosity=2)
