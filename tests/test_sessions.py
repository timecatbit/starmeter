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
        # The running one first, though the finished one changed more recently.
        self.assertEqual([(s['state'], s['project']) for s in found], [('running', 'starmeter'), ('waiting', 'starmeter')])
        self.assertAlmostEqual(found[1]['context'], 0.25); self.assertAlmostEqual(found[0]['context'], 0.3, msg='Past 200K the window is 1M')
        self.assertNotIn('Users', json.dumps(found), 'Only the folder name of a project is kept')

    def test_a_session_goes_by_the_title_its_client_gave_it(self):
        named = self.write(self.claude / 'p' / 'named.jsonl', [{'type': 'ai-title', 'aiTitle': 'Made up'}, {'type': 'custom-title', 'customTitle': '维护'},
                                                              assistant('end_turn'), {'type': 'custom-title', 'customTitle': ' token monitor维护 '}])
        made = self.write(self.claude / 'p' / 'made.jsonl', [{'type': 'ai-title', 'aiTitle': 'Fix the panel'}, assistant('end_turn')], age=1)
        app = self.write(self.claude / 'p' / '7a5d1c2e-0000-4000-8000-000000000001.jsonl', [assistant('end_turn')], age=2)
        bare = self.write(self.claude / 'p' / 'bare.jsonl', [assistant('end_turn')], age=3)
        found = sessions.active([self.claude], None, time.time(), claude_app={app.stem: {'title': 'App 起的名字', 'effort': 'xhigh'}})
        self.assertEqual([(s['title'], s['effort']) for s in found],
                         [('token monitor维护', None), ('Fix the panel', None), ('App 起的名字', 'xhigh'), (None, None)])
        self.assertTrue(named and made and bare)

    def test_codex_threads_from_earlier_days_are_named_and_subagents_left_out(self):
        thread = '019a0000-1111-7222-8333-444455556666'
        old = self.codex / 'sessions' / '2026' / '11' / '03'
        meta = {'type': 'session_meta', 'payload': {'id': thread, 'cwd': '/w/ops', 'source': 'vscode'}}
        started = {'type': 'event_msg', 'payload': {'type': 'task_started'}}
        self.write(old / ('rollout-2026-11-03T10-00-00-%s.jsonl' % thread), [meta, {'type': 'turn_context', 'payload': {'model': 'gpt-6.2-sol', 'effort': 'ultra'}}, started])
        guardian = dict(meta, payload={'id': 'g', 'cwd': '/w/ops', 'source': {'subagent': {'other': 'guardian'}}})
        self.write(self.codex / 'sessions' / time.strftime('%Y/%m/%d') / 'rollout-guardian.jsonl', [guardian, started])
        (self.codex / 'session_index.jsonl').write_text(json.dumps({'id': thread, 'thread_name': '旧名字'}) + '\n' + 'not json\n'
                                                         + json.dumps({'id': thread, 'thread_name': '服务器管理'}) + '\n')
        found = sessions.active([], self.codex, time.time())
        self.assertEqual([(s['title'], s['project'], s['effort'], s['state']) for s in found], [('服务器管理', 'ops', 'ultra', 'running')])

    def test_a_day_of_sessions_and_a_quiet_running_one_is_idle(self):
        self.write(self.claude / 'p' / 'stalled.jsonl', [assistant('tool_use')], age=sessions.STALLED + 5)
        self.write(self.claude / 'p' / 'old.jsonl', [assistant('end_turn')], age=sessions.ACTIVE + 5)
        self.write(self.claude / 'p' / 'idle.jsonl', [assistant('end_turn')], age=3 * 3600)
        self.write(self.claude / 'p' / 'subagents' / 'x.jsonl', [assistant('tool_use')])
        found = sessions.active([self.claude], None, time.time())
        self.assertEqual([s['state'] for s in found], ['waiting', 'waiting'], 'Older than a day: left out; silent while running: idle')

    def test_codex_runs_between_task_started_and_complete(self):
        day = self.codex / 'sessions' / time.strftime('%Y/%m/%d')
        meta = {'type': 'session_meta', 'payload': {'id': 's', 'cwd': '/w/notes'}}
        count = {'type': 'event_msg', 'payload': {'type': 'token_count', 'info': {'model_context_window': 1000, 'last_token_usage': {'input_tokens': 300, 'output_tokens': 100}}}}
        self.write(day / 'rollout-a.jsonl', [meta, {'type': 'turn_context', 'payload': {'model': 'gpt-6.2-sol'}}, {'type': 'event_msg', 'payload': {'type': 'task_started'}}, count])
        self.write(day / 'rollout-b.jsonl', [meta, {'type': 'event_msg', 'payload': {'type': 'task_started'}}, {'type': 'event_msg', 'payload': {'type': 'task_complete'}}], age=1)
        found = sessions.active([], self.codex, time.time())
        self.assertEqual([(s['tool'], s['state'], s['project']) for s in found], [('ChatGPT', 'running', 'notes'), ('ChatGPT', 'waiting', 'notes')])
        self.assertEqual((found[0]['model'], found[0]['context']), ('gpt-6.2-sol', 0.4))

    def test_claude_turns_run_from_prompt_to_prompt(self):
        def reply(id, tools=(), out=10):
            return {'type': 'assistant', 'timestamp': '2026-11-04T01:00:00Z', 'message': {'id': id, 'model': 'claude-opus-5-5',
                    'content': [{'type': 'tool_use', 'name': name} for name in tools], 'usage': {'input_tokens': 100, 'output_tokens': out}}}
        prompt = lambda at: {'type': 'user', 'timestamp': at, 'message': {'content': 'private question'}}
        rows = [reply('early'), prompt('2026-11-04T01:00:00Z'), reply('a', ['Bash']), dict(reply('a', ['Edit']), uuid='x'),
                {'type': 'user', 'message': {'content': [{'type': 'tool_result'}]}}, {'type': 'user', 'isMeta': True, 'message': {'content': 'meta'}},
                dict(reply('side', ['Read']), isSidechain=True), prompt('2026-11-04T02:00:00Z'), reply('b')]
        path = self.write(self.claude / 'p' / 's.jsonl', rows)
        turns = sessions.claude_turns(path, lambda model, usage: None if usage['output_tokens'] == 7 else 0.5)
        self.assertEqual([(t['requests'], t['tokens'], t['tools']) for t in turns],
                         [(1, 110, {}), (2, 220, {'Bash': 1, 'Edit': 1, 'Read': 1}), (1, 110, {})], 'Newest first; a reply streamed over lines counts once')
        self.assertIsNone(turns[-1]['at'], 'Work before the first prompt read belongs to a turn begun earlier')
        self.assertEqual(turns[0]['cost'], 0.5); self.assertNotIn('private question', str(turns))

    def test_codex_turns_run_from_task_started_to_complete(self):
        def count(total, last):return {'type': 'event_msg', 'payload': {'type': 'token_count', 'info': {'total_token_usage': {'t': total}, 'last_token_usage': {'input_tokens': last, 'output_tokens': 1}}}}
        rows = [{'type': 'turn_context', 'payload': {'model': 'gpt-6.2-sol'}}, {'type': 'event_msg', 'timestamp': '2026-11-04T01:00:00Z', 'payload': {'type': 'task_started'}},
                {'type': 'response_item', 'payload': {'type': 'function_call', 'name': 'shell'}}, count(10, 9), count(10, 9), count(20, 9),
                {'type': 'event_msg', 'payload': {'type': 'task_complete'}}]
        path = self.write(self.codex / 's.jsonl', rows)
        turns = sessions.codex_turns(path, lambda model, usage: 0.25 if model == 'gpt-6.2-sol' else None)
        self.assertEqual([(t['requests'], t['tokens'], t['cost'], t['tools']) for t in turns], [(2, 20, 0.5, {'shell': 1})])

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
        self.assertIn('badge="1 运行中 · 1 空闲"', lines[0])
        self.assertTrue(lines[1].startswith('--starmeter |'), 'Without a title, the project folder names it')
        # Running or not: no "waiting for input"; a running one with no request yet says so where its tokens go.
        self.assertIn('paneldetail="运行中 · Opus 5.5 · 上下文 38%"', lines[1]); self.assertIn('panelstatus=new', lines[1])
        self.assertIn('badge=运行中', lines[1]); self.assertIn('image=', lines[1], 'Each session wears its tool\'s logo')
        self.assertTrue(lines[2].startswith('--未知项目 |')); self.assertIn('paneldetail="空闲 3 分钟"', lines[2])
        data['sessions']['sessions'][1]['at'] = now - 3 * 3600 - 200
        self.assertIn('paneldetail="空闲 3 小时"', self.m.sessions_lines(data)[2], 'From an hour on, in hours')
        self.assertIn('panelstatus=unknown', lines[2])
        titled = [{'tool': 'ChatGPT', 'state': 'running', 'title': '服务器管理', 'project': 'ops', 'model': 'gpt-6.2-sol', 'effort': 'xhigh',
                   'context': 0.54, 'at': now, 'id': 'c', 'turns': [{'at': None, 'requests': 1, 'tokens': 10, 'cost': 0.0, 'unpriced': 0, 'tools': {}}]}]
        lines = self.m.sessions_lines({'sessions': {'sessions': titled}})
        self.assertTrue(lines[1].startswith('--服务器管理 |'), 'A session goes by the title its client gave it')
        self.assertIn('paneldetail="运行中 · gpt-6.2-sol · xhigh · 上下文 54%"', lines[1])
        self.assertIn('badge="10 Token"', lines[1], 'The live figure is this turn\'s tokens')
        self.assertTrue(lines[2].startswith('----ops · 最近 1 轮'), 'The project folder heads its turns')
        self.assertIn('badge=没有会话', self.m.sessions_lines({'sessions': {'sessions': []}})[0])
        self.assertIn('badge=待同步', self.m.sessions_lines({})[0])

    def test_a_live_auto_review_request_is_priced_by_today(self):
        # Codex's reviewer is priced by the day of the request; a session read live has no day of its own.
        self.assertIsNotNone(self.m.session_prices()['ChatGPT']('codex-auto-review', {'input_tokens': 10, 'output_tokens': 1}))

    def test_each_session_lists_its_last_turns(self):
        turns = [{'at': None, 'requests': 3, 'tokens': 1200, 'cost': 0.4, 'unpriced': 1, 'tools': {'Bash': 2, 'Read': 5}}]
        lines = self.m.turn_lines(turns)
        self.assertIn('最近 1 轮', lines[0]); self.assertTrue(lines[1].startswith('----更早开始 · 1.2k Token · 已知 $0.40'))
        self.assertIn('3 次请求 · Read ×5 · Bash ×2 · 1 次未定价', lines[1])
        self.assertIn('费用待定价', self.m.turn_lines([dict(turns[0], unpriced=3)])[1])
        self.assertEqual(self.m.turn_lines([]), [])
        stopped = {'at': None, 'requests': 0, 'tokens': 0, 'cost': 0, 'unpriced': 0, 'tools': {}}
        self.assertEqual(self.m.turn_lines([stopped]), [], 'A turn that sent no request is left out')
        mcp = self.m.turn_lines([dict(turns[0], tools={'mcp__ccd_turn__update_plan': 2, 'mcp__x__update_plan': 1, 'Read': 1}), stopped])
        self.assertIn('最近 1 轮', mcp[0]); self.assertIn('3 次请求 · update_plan ×3 · Read ×1', mcp[1], 'MCP tools go by their own name')

    def test_the_alert_is_off_by_default_and_tells_of_a_finished_turn(self):
        self.assertTrue('sessions' in preferences.read(self.m.STATE_DIR)['visible'], 'New installs list active sessions')
        self.assertFalse(preferences.read(self.m.STATE_DIR)['sessionAlerts'])
        running = [{'tool': 'Claude', 'state': 'running', 'project': 'starmeter', 'id': 'a', 'at': time.time()}]
        with patch.object(self.m.live_sessions, 'active', return_value=running):self.m.sessions_cached()
        self.m.run_action('set-session-alerts', ['true'])
        self.m.invalidate_caches(['sessions'])
        with patch.object(self.m.live_sessions, 'active', return_value=[dict(running[0], state='waiting')]), patch.object(self.m, 'post_notice', return_value=True) as told:
            self.m.sessions_cached()
        told.assert_called_once_with('会话完成', 'Claude · starmeter 这一轮已完成')


if __name__ == '__main__':
    unittest.main(verbosity=2)
