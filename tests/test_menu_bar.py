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


class MenuBarTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(); self.addCleanup(self.temp.cleanup)
        env = patch.dict(os.environ, {'OCX_PLUGIN_PATH': str(ROOT / 'starmeter.30s.sh'), 'OCX_CACHE_DIR': self.temp.name,
                                      'OCX_STATE_DIR': self.temp.name + '/state'})
        env.start(); self.addCleanup(env.stop)
        self.m = types.ModuleType('menu_bar_test'); exec(compile((ROOT / 'lib/swiftbar_plugin.py').read_text(), 'plugin', 'exec'), self.m.__dict__)
        for key in ('claude', 'codex'):preferences.update(self.m.STATE_DIR, 'module', key, 'true')
        # Choosing the estimate starts a background read: none here, it would write into the folder
        # being cleaned up.
        background = patch.object(self.m, 'schedule_background_refresh'); background.start(); self.addCleanup(background.stop)

    def today(self, claude, codex, output=(0, 0), at=None):
        os.makedirs(self.m.CACHE_DIR, exist_ok=True)
        payload = {'range': 'today', 'claude': {'summary': {'estimatedCostUsd': claude, 'requests': 2, 'outputTokens': output[0]}},
                   'codex': {'summary': {'estimatedCostUsd': codex, 'requests': 1, 'outputTokens': output[1]}}}
        record = {'schema': 1, 'fetchedAt': time.time() if at is None else at, 'day': time.strftime('%Y-%m-%d'), 'payload': payload}
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
        rows = next(o for o in self.m.module_options() if o['label'] == '通用')['children']
        row = next(r for r in rows if r['label'] == '菜单栏显示')
        self.assertEqual(row['value'], '只显示星星')
        self.assertEqual([c['action'] for c in row['children']], [['set-menu-bar', 'none'], ['set-menu-bar', 'cost'], ['set-menu-bar', 'quota']])

    def test_the_output_speed_is_gone(self):
        """输出速度 beside the star was taken out: a stored choice reads as the star alone, and it cannot be chosen."""
        preferences.update(self.m.STATE_DIR, 'menu-bar', 'cost')
        place = Path(self.m.STATE_DIR, 'preferences.json')
        place.write_text(json.dumps(dict(json.loads(place.read_text()), menuBar='speed')))
        self.assertEqual(self.m.view_settings()['menuBar'], 'none')
        self.today(1, 1, (3000, 1000))
        self.assertEqual(self.m.menu_bar_value({}), '')
        with self.assertRaises(ValueError):self.m.run_action('set-menu-bar', ['speed'])
        with patch.object(self.m, 'selected_range', return_value='7d'):
            self.assertNotIn('today', self.m.usage_ranges_due(), 'Nothing reads today every minute for it')

    def test_settings_are_pages_by_what_they_are_for(self):
        options = self.m.module_options()
        self.assertEqual((options[0]['label'], options[0].get('section')), ('设置', True))
        top = [o['label'] for o in options[1:next(i for i, o in enumerate(options) if i and o.get('section'))]]
        self.assertEqual(top, ['外观', '通用', '提醒', '数据'])
        pages = {o['label']: [r['label'] for r in o['children'] if not r.get('separator') and not r.get('radio')] for o in options[1:5]}
        self.assertEqual(pages, {'外观': ['允许折叠', '金额显示'], '通用': ['菜单栏显示', '钉住时刷新', '模块与配置', '自动检测', '高级配置文件…', '恢复默认模块'],
                                 '提醒': ['用量提醒', '服务状态'], '数据': ['保留已清理记录的用量', '导出用量', '多台电脑合并']})
        self.assertTrue(next(r for r in options[2]['children'] if r['label'] == '模块与配置').get('section'))


if __name__ == '__main__':
    unittest.main(verbosity=2)
