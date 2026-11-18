import json
import os
from pathlib import Path
import sqlite3
import sys
import tempfile
import types
import unittest
from datetime import datetime, timezone
from unittest.mock import patch
from zoneinfo import ZoneInfo

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'lib'))
import swiftbar_archive as archive
import swiftbar_preferences as preferences
import swiftbar_sources as sources


def codex_rows(session, total, when):
    usage = {'total_tokens': total, 'input_tokens': total - 10, 'cached_input_tokens': 40, 'output_tokens': 10, 'reasoning_output_tokens': 5}
    return [{'type': 'session_meta', 'payload': {'id': session, 'model_provider': 'openai'}}, {'type': 'turn_context', 'payload': {'model': 'gpt-5'}},
            {'type': 'event_msg', 'timestamp': when, 'payload': {'type': 'token_count', 'info': {'total_token_usage': usage, 'last_token_usage': usage}}}]


class GenericArchiveTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(); self.addCleanup(self.temp.cleanup)
        base = Path(self.temp.name); self.root = base / 'sessions'; self.root.mkdir(); self.cache = base / 'cache'
        self.kept = archive.location(base / 'state', 'codex', 'x')

    def write(self, name, rows):
        (self.root / name).write_text(''.join(json.dumps(row) + '\n' for row in rows))

    def read(self, kept='default', start=0, end=9e9):
        return sources.jsonl_usage('codex', self.root, self.cache, start, end, timezone.utc, archive=self.kept if kept == 'default' else kept)

    def test_deleted_logs_keep_counting_while_the_archive_is_used(self):
        self.write('a.jsonl', codex_rows('a', 150, 100)); self.write('b.jsonl', codex_rows('b', 200, 300))
        self.assertEqual(self.read()['summary']['totalTokens'], 350)
        (self.root / 'a.jsonl').unlink()
        result = self.read()
        self.assertEqual(result['summary']['totalTokens'], 350); self.assertEqual(result['summary']['requests'], 2)
        self.assertEqual(self.read(None)['summary']['totalTokens'], 200, 'Without the archive only the logs that are left count')
        self.assertEqual(self.read(start=0, end=200)['summary']['totalTokens'], 150, 'Archived requests keep their own time')
        (self.root / 'b.jsonl').unlink()
        self.assertEqual(self.read()['summary']['totalTokens'], 350)
        self.assertIn('error', self.read(None))

    def test_a_moved_log_is_counted_once(self):
        self.write('a.jsonl', codex_rows('a', 150, 100)); self.read()
        (self.root / 'a.jsonl').rename(self.root / 'moved.jsonl')
        self.assertEqual(self.read()['summary']['totalTokens'], 150)

    def test_a_rewritten_log_is_not_archived(self):
        self.write('a.jsonl', codex_rows('a', 150, 100)); self.read()
        self.write('a.jsonl', codex_rows('a', 151, 100))
        self.assertEqual(self.read()['summary']['totalTokens'], 151)
        self.assertFalse(self.kept.exists(), 'Nothing disappeared, so nothing was archived')

    def test_the_archive_keeps_counters_only(self):
        self.write('a.jsonl', codex_rows('a', 150, 100)); self.read()
        (self.root / 'a.jsonl').unlink(); self.read()
        self.assertEqual(oct(self.kept.stat().st_mode & 0o777), '0o600')
        self.assertEqual(oct(self.kept.parent.stat().st_mode & 0o777), '0o700')
        raw = self.kept.read_bytes()
        self.assertNotIn(str(self.root).encode(), raw)
        archive.clear(self.kept.parent.parent); self.assertFalse(self.kept.parent.exists())


class ClaudeArchiveTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(); self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name) / 'projects'; self.root.mkdir()
        env = patch.dict(os.environ, {'OCX_PLUGIN_PATH': str(ROOT / 'starmeter.30s.sh'), 'OCX_CACHE_DIR': self.temp.name,
                                      'OCX_STATE_DIR': self.temp.name + '/state'})
        env.start(); self.addCleanup(env.stop)
        self.m = types.ModuleType('archive_test'); exec(compile((ROOT / 'lib/swiftbar_plugin.py').read_text(), 'plugin', 'exec'), self.m.__dict__)
        self.now = datetime(2026, 11, 4, 12, tzinfo=ZoneInfo('Asia/Taipei')); self.db = str(Path(self.temp.name) / 'usage.db')
        self.kept = str(Path(self.temp.name) / 'state/archive/claude-v1-x.sqlite3')

    def row(self, id, timestamp='2026-11-04T01:00:00Z'):
        return {'type': 'assistant', 'timestamp': timestamp, 'sessionId': 'session', 'requestId': 'req-' + id,
                'message': {'id': id, 'model': 'claude-opus-5-5', 'usage': {'input_tokens': 100, 'output_tokens': 20}}}

    def write(self, name, rows):
        (self.root / name).write_text(''.join(json.dumps(row) + '\n' for row in rows))

    def stats(self, kept='default', key='all'):
        return self.m.claude_usage_records(self.root, self.db, self.now, key, archive=self.kept if kept == 'default' else kept)[0]['summary']

    def test_cleaned_transcripts_keep_counting_and_copies_count_once(self):
        self.write('old.jsonl', [self.row('m1', '2026-10-01T01:00:00Z'), self.row('m2')]); self.write('new.jsonl', [self.row('m2'), self.row('m3')])
        self.assertEqual(self.stats()['requests'], 3)
        (self.root / 'old.jsonl').unlink()
        self.assertEqual(self.stats()['requests'], 3, 'm1 lives on in the archive; m2 survives in new.jsonl and counts once')
        self.assertEqual(self.stats(None)['requests'], 2, 'Turning the archive off goes back to the transcripts that are left')
        self.assertEqual(self.stats()['requests'], 3)
        self.assertEqual(self.stats(key='today')['requests'], 2, 'The archived response keeps its own day')

    def test_a_transcript_that_comes_back_counts_as_itself(self):
        self.write('a.jsonl', [self.row('m1')]); self.stats()
        moved = (self.root / 'a.jsonl').read_text(); (self.root / 'a.jsonl').unlink(); self.stats()
        (self.root / 'a.jsonl').write_text(moved)
        self.assertEqual(self.stats()['requests'], 1)
        conn = sqlite3.connect(self.kept)
        try:self.assertEqual(conn.execute('SELECT COUNT(*) FROM records').fetchone()[0], 1)
        finally:conn.close()


class SettingTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(); self.addCleanup(self.temp.cleanup)
        env = patch.dict(os.environ, {'OCX_PLUGIN_PATH': str(ROOT / 'starmeter.30s.sh'), 'OCX_CACHE_DIR': self.temp.name,
                                      'OCX_STATE_DIR': self.temp.name + '/state'})
        env.start(); self.addCleanup(env.stop)
        self.m = types.ModuleType('archive_setting_test'); exec(compile((ROOT / 'lib/swiftbar_plugin.py').read_text(), 'plugin', 'exec'), self.m.__dict__)

    def test_on_by_default_and_turning_it_off_asks_then_deletes_the_archive(self):
        self.assertTrue(preferences.read(self.m.STATE_DIR)['keepCleanedUsage'])
        self.assertTrue(self.m.usage_archive('codex', '/tmp/somewhere').startswith(os.path.join(self.m.STATE_DIR, 'archive')))
        kept = Path(self.m.usage_archive('codex', '/tmp/somewhere')); archive.keep(kept, [('id', 1.0, '{}')])
        with patch.object(self.m, 'confirm', return_value=False):self.m.run_action('set-keep-usage', ['false'])
        self.assertTrue(kept.exists()); self.assertTrue(preferences.read(self.m.STATE_DIR)['keepCleanedUsage'])
        with patch.object(self.m, 'confirm', return_value=True) as ask, patch.object(self.m, 'schedule_background_refresh'):
            self.m.run_action('set-keep-usage', ['false'])
        self.assertIn('无法恢复', ask.call_args.args[1])
        self.assertFalse(kept.exists()); self.assertFalse(preferences.read(self.m.STATE_DIR)['keepCleanedUsage'])
        self.assertIsNone(self.m.usage_archive('codex', '/tmp/somewhere'))
        with patch.object(self.m, 'schedule_background_refresh'):self.m.run_action('set-keep-usage', ['true'])
        self.assertTrue(preferences.read(self.m.STATE_DIR)['keepCleanedUsage'])

    def test_the_switch_sits_in_appearance_and_settings(self):
        rows = next(o for o in self.m.module_options() if o['label'] == '外观与设置')['children']
        row = next(r for r in rows if r['label'] == '保留已清理记录的用量')
        self.assertTrue(row['checked']); self.assertEqual(row['action'], ['set-keep-usage', 'false'])


if __name__ == '__main__':
    unittest.main(verbosity=2)
