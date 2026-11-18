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
import swiftbar_sessions as sessions


def assistant(stop, used=1000, model='claude-opus-5-5', cwd='/Users/someone/Code/starmeter'):
    return {'type': 'assistant', 'cwd': cwd, 'message': {'model': model, 'stop_reason': stop,
            'usage': {'input_tokens': used, 'cache_read_input_tokens': 0, 'output_tokens': 0}}}


class SessionReaderTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(); self.addCleanup(self.temp.cleanup)
        self.claude = Path(self.temp.name) / 'projects'; self.codex = Path(self.temp.name) / 'codex'

    def write(self, path, rows, age=0):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(''.join(json.dumps(row) + '\n' for row in rows))
        stamp = time.time() - age; os.utime(path, (stamp, stamp))
        return path

    def test_claude_waits_once_the_turn_ends_and_runs_otherwise(self):
        self.write(self.claude / 'p' / 'done.jsonl', [assistant('tool_use'), {'type': 'user', 'message': {'content': []}}, assistant('end_turn', 50_000),
                                                     {'type': 'last-prompt'}, {'type': 'mode'}])
        self.write(self.claude / 'p' / 'busy.jsonl', [assistant('tool_use', 300_000), {'type': 'user', 'message': {'content': []}}, {'type': 'api-request'}], age=5)
        self.write(self.claude / 'p' / 'side.jsonl', [dict(assistant('tool_use'), isSidechain=True)])
        found = sessions.active([self.claude], None, time.time())
        self.assertEqual([(s['state'], s['project']) for s in found], [('waiting', 'starmeter'), ('running', 'starmeter')])
        self.assertAlmostEqual(found[0]['context'], 0.25); self.assertAlmostEqual(found[1]['context'], 0.3, msg='Past 200K the window is 1M')
        self.assertNotIn('Users', json.dumps(found), 'Only the folder name of a project is kept')

    def test_quiet_or_old_sessions_drop_out(self):
        self.write(self.claude / 'p' / 'stalled.jsonl', [assistant('tool_use')], age=sessions.STALLED + 5)
        self.write(self.claude / 'p' / 'old.jsonl', [assistant('end_turn')], age=sessions.ACTIVE + 5)
        self.write(self.claude / 'p' / 'idle.jsonl', [assistant('end_turn')], age=sessions.STALLED + 5)
        self.write(self.claude / 'p' / 'subagents' / 'x.jsonl', [assistant('tool_use')])
        self.assertEqual([s['state'] for s in sessions.active([self.claude], None, time.time())], ['waiting'])

    def test_codex_runs_between_task_started_and_complete(self):
        day = self.codex / 'sessions' / time.strftime('%Y/%m/%d')
        meta = {'type': 'session_meta', 'payload': {'id': 's', 'cwd': '/w/notes'}}
        count = {'type': 'event_msg', 'payload': {'type': 'token_count', 'info': {'model_context_window': 1000, 'last_token_usage': {'input_tokens': 300, 'output_tokens': 100}}}}
        self.write(day / 'rollout-a.jsonl', [meta, {'type': 'turn_context', 'payload': {'model': 'gpt-6.2-sol'}}, {'type': 'event_msg', 'payload': {'type': 'task_started'}}, count])
        self.write(day / 'rollout-b.jsonl', [meta, {'type': 'event_msg', 'payload': {'type': 'task_started'}}, {'type': 'event_msg', 'payload': {'type': 'task_complete'}}], age=1)
        found = sessions.active([], self.codex, time.time())
        self.assertEqual([(s['tool'], s['state'], s['project']) for s in found], [('ChatGPT', 'running', 'notes'), ('ChatGPT', 'waiting', 'notes')])
        self.assertEqual((found[0]['model'], found[0]['context']), ('gpt-6.2-sol', 0.4))

    def test_only_a_turn_that_just_finished_is_news(self):
        now = time.time()
        before = [{'id': 'a', 'state': 'running'}, {'id': 'b', 'state': 'waiting'}]
        after = [{'id': 'a', 'state': 'waiting', 'at': now}, {'id': 'b', 'state': 'waiting', 'at': now}, {'id': 'c', 'state': 'waiting', 'at': now}]
        self.assertEqual([s['id'] for s in sessions.newly_waiting(before, after, now)], ['a'])
        self.assertEqual(sessions.newly_waiting(before, [dict(after[0], at=now - 600)], now), [])


class SessionPanelTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(); self.addCleanup(self.temp.cleanup)
        env = patch.dict(os.environ, {'OCX_PLUGIN_PATH': str(ROOT / 'starmeter.30s.sh'), 'OCX_CACHE_DIR': self.temp.name,
                                      'OCX_STATE_DIR': self.temp.name + '/state'})
        env.start(); self.addCleanup(env.stop)
        self.m = types.ModuleType('sessions_test'); exec(compile((ROOT / 'lib/swiftbar_plugin.py').read_text(), 'plugin', 'exec'), self.m.__dict__)

    def test_homepage_row_counts_and_lists_each_session(self):
        now = time.time()
        data = {'sessions': {'sessions': [{'tool': 'Claude', 'state': 'running', 'project': 'starmeter', 'model': 'claude-opus-5-5-20261101', 'context': 0.38, 'at': now, 'id': 'a'},
                                          {'tool': 'ChatGPT', 'state': 'waiting', 'project': None, 'model': None, 'context': None, 'at': now - 200, 'id': 'b'}]}}
        lines = self.m.sessions_lines(data)
        self.assertIn('badge="1 运行中 · 1 等待输入"', lines[0])
        self.assertIn('starmeter · Claude', lines[1]); self.assertIn('Claude Opus 5.5 · 上下文已用 38% · 刚刚', lines[1]); self.assertIn('panelstatus=new', lines[1])
        self.assertIn('未知项目 · ChatGPT', lines[2]); self.assertIn('3 分钟前', lines[2])
        self.assertIn('badge=没有活跃会话', self.m.sessions_lines({'sessions': {'sessions': []}})[0])
        self.assertIn('badge=待同步', self.m.sessions_lines({})[0])

    def test_the_alert_is_off_by_default_and_tells_of_a_finished_turn(self):
        self.assertTrue('sessions' in preferences.read(self.m.STATE_DIR)['visible'], 'New installs list active sessions')
        self.assertFalse(preferences.read(self.m.STATE_DIR)['sessionAlerts'])
        running = [{'tool': 'Claude', 'state': 'running', 'project': 'starmeter', 'id': 'a', 'at': time.time()}]
        with patch.object(self.m.live_sessions, 'active', return_value=running):self.m.sessions_cached()
        self.m.run_action('set-session-alerts', ['true'])
        self.m.invalidate_caches(['sessions'])
        with patch.object(self.m.live_sessions, 'active', return_value=[dict(running[0], state='waiting')]), patch.object(self.m, 'post_notice', return_value=True) as told:
            self.m.sessions_cached()
        told.assert_called_once_with('会话等待输入', 'Claude · starmeter 这一轮已完成')


if __name__ == '__main__':
    unittest.main(verbosity=2)
