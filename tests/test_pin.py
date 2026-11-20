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


class PinTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(); self.addCleanup(self.temp.cleanup)
        env = patch.dict(os.environ, {'OCX_PLUGIN_PATH': str(ROOT / 'starmeter.30s.sh'), 'OCX_CACHE_DIR': self.temp.name,
                                      'OCX_STATE_DIR': self.temp.name + '/state'})
        env.start(); self.addCleanup(env.stop)
        self.m = types.ModuleType('pin_test'); exec(compile((ROOT / 'lib/swiftbar_plugin.py').read_text(), 'plugin', 'exec'), self.m.__dict__)

    def test_a_tick_rereads_the_range_on_screen_and_the_sessions_at_once(self):
        with patch.object(self.m, 'range_usage_cached') as figures, patch.object(self.m, 'sessions_cached') as sessions, \
             patch.object(self.m, 'selected_range', return_value='7d'):
            self.m.run_action('refresh-live', [])
        figures.assert_called_once_with('7d', ttl=0); sessions.assert_called_once_with(ttl=0)

    def test_a_tick_while_the_last_is_running_is_skipped(self):
        import fcntl
        os.makedirs(self.m.CACHE_DIR, exist_ok=True)
        with open(os.path.join(self.m.CACHE_DIR, 'range-refresh.lock'), 'a') as held:
            fcntl.flock(held, fcntl.LOCK_EX)
            with patch.object(self.m, 'range_usage_cached') as figures:
                self.assertFalse(self.m.refresh_live())
            figures.assert_not_called()

    def test_the_interval_is_a_setting_and_reaches_the_panel(self):
        self.assertEqual(preferences.read(self.m.STATE_DIR)['pinInterval'], 2)
        self.m.run_action('set-pin-interval', ['1'])
        self.assertEqual(preferences.read(self.m.STATE_DIR)['pinInterval'], 1)
        with self.assertRaises(ValueError):self.m.run_action('set-pin-interval', ['0.1'])
        rows = next(o for o in self.m.module_options() if o['label'] == '通用')['children']
        row = next(r for r in rows if r['label'] == '钉住时刷新')
        self.assertEqual(row['value'], '每 1 秒'); self.assertEqual([c['action'][1] for c in row['children']], ['1', '2', '5'])
        self.assertEqual(self.m.panel_snapshot({}, {'rangeKey': 'today'}, [])['pinInterval'], 1)


if __name__ == '__main__':
    unittest.main(verbosity=2)
