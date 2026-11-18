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


class MenuBarTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(); self.addCleanup(self.temp.cleanup)
        env = patch.dict(os.environ, {'OCX_PLUGIN_PATH': str(ROOT / 'starmeter.30s.sh'), 'OCX_CACHE_DIR': self.temp.name,
                                      'OCX_STATE_DIR': self.temp.name + '/state'})
        env.start(); self.addCleanup(env.stop)
        self.m = types.ModuleType('menu_bar_test'); exec(compile((ROOT / 'lib/swiftbar_plugin.py').read_text(), 'plugin', 'exec'), self.m.__dict__)
        for key in ('claude', 'codex'):preferences.update(self.m.STATE_DIR, 'module', key, 'true')

    def today(self, claude, codex):
        os.makedirs(self.m.CACHE_DIR, exist_ok=True)
        payload = {'range': 'today', 'claude': {'summary': {'estimatedCostUsd': claude, 'requests': 2}}, 'codex': {'summary': {'estimatedCostUsd': codex, 'requests': 1}}}
        record = {'schema': 1, 'fetchedAt': time.time(), 'day': time.strftime('%Y-%m-%d'), 'payload': payload}
        self.m.write_json_cache(self.m.cache_path(self.m.range_cache_name('today')), record)

    def test_only_the_star_by_default(self):
        self.today(1, 2)
        self.assertEqual(self.m.menu_bar_value({}), '')
        line = self.m.title({})
        self.assertTrue(line.startswith(' | ')); self.assertNotIn('size=', line)

    def test_today_estimate_beside_the_star(self):
        self.m.run_action('set-menu-bar', ['cost'])
        self.assertEqual(self.m.menu_bar_value({}), '', 'Nothing until today has been read')
        self.today(12.345, 3.2)
        self.assertEqual(self.m.menu_bar_value({}), '$15.55')
        self.assertTrue(self.m.title({}).startswith('$15.55 | '))
        self.today(1200.4, 34)
        self.assertEqual(self.m.menu_bar_value({}), '$1,234')
        os.unlink(self.m.cache_path(self.m.range_cache_name('today')))
        with patch.object(self.m, 'selected_range', return_value='7d'):
            self.assertIn('today', self.m.usage_ranges_due(), 'Today stays fresh while another range is shown')

    def test_tightest_current_subscription_window(self):
        self.m.run_action('set-menu-bar', ['quota'])
        windows = [{'used': 92.4, 'current': False}, {'used': 76.6, 'current': True}, {'used': 12, 'current': True}]
        with patch.object(self.m, 'alert_windows', return_value=windows):
            self.assertEqual(self.m.menu_bar_value({}), '77%')
        with patch.object(self.m, 'alert_windows', return_value=[{'used': 92.4, 'current': False}]):
            self.assertEqual(self.m.menu_bar_value({}), '', 'A stale reading is not kept in sight')

    def test_the_choice_lives_in_appearance_and_settings(self):
        rows = next(o for o in self.m.module_options() if o['label'] == '外观与设置')['children']
        row = next(r for r in rows if r['label'] == '菜单栏显示')
        self.assertEqual(row['value'], '只显示星星')
        self.assertEqual([c['action'] for c in row['children']], [['set-menu-bar', 'none'], ['set-menu-bar', 'cost'], ['set-menu-bar', 'quota']])
        with self.assertRaises(ValueError):self.m.run_action('set-menu-bar', ['loud'])

    def test_settings_are_grouped_under_titles(self):
        rows = next(o for o in self.m.module_options() if o['label'] == '外观与设置')['children']
        groups, current = {}, None
        for r in rows:
            if r.get('section'):current = r['label'];groups[current] = []
            elif current and not r.get('separator'):groups[current].append(r['label'])
        self.assertEqual(groups, {'显示': ['金额显示', '菜单栏显示'], '钉住与提醒': ['钉住时刷新', '用量提醒', '服务状态'],
                                  '数据': ['保留已清理记录的用量', '导出用量'], '模块与配置': ['自动检测', '高级配置文件…', '恢复默认模块']})


if __name__ == '__main__':
    unittest.main(verbosity=2)
