import io
import json
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
import swiftbar_status as status


class Response(io.BytesIO):
    def __enter__(self):return self
    def __exit__(self, *_):self.close()


def opener(body):
    seen = []
    def open_(request, timeout):
        seen.append(request)
        if isinstance(body, Exception):raise body
        return Response(json.dumps(body).encode() if not isinstance(body, bytes) else body)
    return open_, seen


class StatusPageTests(unittest.TestCase):
    def test_a_page_reads_as_its_indicator_and_sends_nothing_about_the_user(self):
        read, seen = opener({'page': {'name': 'Claude'}, 'status': {'indicator': 'minor', 'description': 'Partially Degraded Service'}})
        self.assertEqual(status.read('claude', '9.9.9', read), {'indicator': 'minor', 'description': 'Partially Degraded Service'})
        request = seen[0]
        self.assertEqual(request.full_url, 'https://status.claude.com/api/v2/status.json')
        self.assertEqual(sorted(k.lower() for k in request.headers), ['accept', 'user-agent']); self.assertEqual(request.get_header('User-agent'), 'Starmeter/9.9.9')

    def test_unreadable_or_odd_pages_say_so_for_themselves(self):
        self.assertEqual(status.read('openai', '1', opener(OSError('offline'))[0]), {'error': 'OSError'})
        self.assertIn('error', status.read('openai', '1', opener({'status': {'indicator': 'meltdown', 'description': 'x'}})[0]))
        self.assertIn('error', status.read('openai', '1', opener(b'not json')[0]))
        both = status.read_all(['openai', 'cursor', 'unknown'], '1', opener({'status': {'indicator': 'none', 'description': 'All Systems Operational'}})[0])
        self.assertEqual(sorted(both), ['cursor', 'openai'])

    def test_cards_hear_only_of_trouble(self):
        pages = {'claude': {'indicator': 'major', 'description': 'Major Outage'}, 'openai': {'indicator': 'none', 'description': 'ok'}, 'cursor': {'error': 'OSError'}}
        self.assertEqual(status.note(pages, 'claude'), '服务大范围故障')
        self.assertIsNone(status.note(pages, 'codex')); self.assertIsNone(status.note(pages, 'cursor')); self.assertIsNone(status.note(pages, 'magpie'))


class StatusPanelTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(); self.addCleanup(self.temp.cleanup)
        env = patch.dict(os.environ, {'OCX_PLUGIN_PATH': str(ROOT / 'starmeter.30s.sh'), 'OCX_CACHE_DIR': self.temp.name,
                                      'OCX_STATE_DIR': self.temp.name + '/state'})
        env.start(); self.addCleanup(env.stop)
        self.m = types.ModuleType('status_test'); exec(compile((ROOT / 'lib/swiftbar_plugin.py').read_text(), 'plugin', 'exec'), self.m.__dict__)
        preferences.write(self.m.STATE_DIR, dict(preferences.read(self.m.STATE_DIR), visible=['claude', 'magpie', 'claude-subscription']))

    def test_off_by_default_so_nothing_is_read(self):
        self.assertEqual(self.m.status_pages(), []); self.assertNotIn('service_status', self.m.external_loaders())
        self.assertEqual(self.m.status_lines('claude-subscription', {}), [])

    def test_on_it_reads_the_pages_of_modules_shown_and_marks_cards(self):
        with patch.object(self.m, 'schedule_background_refresh'):self.m.run_action('set-service-status', ['true'])
        self.assertEqual(self.m.status_pages(), ['claude']); self.assertIn('service_status', self.m.external_loaders())
        data = {'service_status': {'claude': {'indicator': 'minor', 'description': 'Partially Degraded Service'}},
                'range_usage': {'range': 'today', 'claude': {'summary': {'requests': 1, 'totalTokens': 10}}, 'magpie': {'summary': {'requests': 1, 'totalTokens': 5}}},
                'displayed_range': 'today'}
        cards = {p['id']: p for p in self.m.dashboard_payload(data)['providers']}
        self.assertEqual(cards['claude']['serviceNote'], '服务部分降级'); self.assertIsNone(cards['magpie']['serviceNote'])
        row = self.m.status_lines('claude-subscription', data)[0]
        self.assertIn('Claude 服务状态', row); self.assertIn('badge=服务部分降级', row); self.assertIn('panelstatus=issue', row); self.assertIn('href=https://status.claude.com', row)
        self.assertIn('badge=待同步', self.m.status_lines('claude-subscription', {})[0])


if __name__ == '__main__':
    unittest.main(verbosity=2)
