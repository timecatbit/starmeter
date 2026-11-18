"""Holds the Swift engine (engine/) against the Python one: the same inputs must give the same answers, down to
int-or-float and key order, because the two share state files while the port is under way."""
import json
import os
from pathlib import Path
import re
import shutil
import sqlite3
import subprocess
import sys
import tempfile
import unittest
from datetime import timezone

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'lib'))
import swiftbar_catalog as catalog
import swiftbar_claude_usage as claude_usage
import swiftbar_files as files
import swiftbar_preferences as preferences
from swiftbar_time import parse_iso
import hashlib

try:
    from zoneinfo import ZoneInfo
except ImportError:  # Python 3.8 and older
    ZoneInfo = None


def probe_binary():
    """The probe check.py built, or one built here when this file runs on its own."""
    given = os.environ.get('STARMETER_PROBE')
    if given:
        return given if os.path.isfile(given) else None
    swift = shutil.which('swift')
    if not swift:
        return None
    scratch = ROOT / 'build' / 'engine'
    built = subprocess.run([swift, 'build', '--package-path', str(ROOT / 'engine'), '--scratch-path', str(scratch),
                            '--product', 'starmeter-probe'], capture_output=True, text=True)
    if built.returncode:
        raise RuntimeError('swift build failed:\n' + built.stdout + built.stderr)
    path = scratch / 'debug' / 'starmeter-probe'
    return str(path) if path.is_file() else None


PROBE = probe_binary()


class SwiftError(Exception):
    pass


def swift(call, *args):
    done = subprocess.run([PROBE], input=json.dumps({'call': call, 'args': list(args)}).encode(), capture_output=True, check=True)
    answer = json.loads(done.stdout)
    if 'error' in answer:
        raise SwiftError(answer['error'])
    return answer['result']


def same(test, python_value, swift_value):
    """Equal down to int-or-float and key order, which a plain == would let slide."""
    test.assertEqual(json.dumps(python_value, ensure_ascii=False), json.dumps(swift_value, ensure_ascii=False))


@unittest.skipUnless(PROBE, 'Swift toolchain not installed')
class EngineParityTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)

    def test_times_read_like_parse_iso(self):
        cases = ['2026-11-18T10:00:00Z', '2026-11-18T10:00:00.123Z', '2026-11-18T10:00:00.1234567Z',
                 '2026-11-18T10:00:00.1+08:00', '2026-11-18T10:00:00,5+0800', '2026-11-18 10:00:00-0530',
                 '2026-11-18T10:00', '2026-11-18T10', '2026-11-18', '  2026-11-18T10:00:00z  ', '2024-02-29T00:00:00Z',
                 '2026-11-18T23:59:59.999999-12:00', '2026-11-18T10:00:00.000001+05:45', '2026-03-08T02:30:00', '2026-11-01T01:30:00',
                 # Every Python from 3.9 up rejects these.
                 '2026-02-29T00:00:00Z', '2026-13-01', '2026-11-18T25:00', 'garbage', '2026-11-18T10:00:00.',
                 '2026-11-18T10:00:00+24:00', '1700000000', '', '2026-11-18T10:61', '2026-1-8']
        zones = ['UTC', 'Asia/Shanghai', 'America/New_York'] if ZoneInfo else ['UTC']
        for text in cases:
            with self.subTest(text=text):
                try:
                    expected = parse_iso(text)
                except ValueError:
                    with self.assertRaises(SwiftError):
                        swift('parse_iso', text)
                    continue
                for zone in zones:
                    got = swift('parse_iso', text, zone)
                    self.assertEqual(got['wall'], expected.replace(tzinfo=None).isoformat(timespec='microseconds'))
                    offset = expected.utcoffset()
                    self.assertEqual(got['offset'], None if offset is None else int(offset.total_seconds()))
                    self.assertEqual(got['isoformat'], expected.isoformat())
                    aware = expected if expected.tzinfo else expected.replace(tzinfo=ZoneInfo(zone) if ZoneInfo else timezone.utc)
                    self.assertAlmostEqual(got['timestamp'], aware.timestamp(), places=5)

    def test_json_reads_and_writes_like_the_json_module(self):
        cases = ['{"b": 1, "a": [true, false, null], "b": 2.5}', '[1, 1.0, -0, -0.0, 1e2, 1E-2, 123456789012345678]',
                 '"caf\\u00e9 \\ud83d\\ude00 \\u2028 \\n\\t\\"\\\\/\\b\\f\\u0001\\u007f"', '"直接写的中文 😀"',
                 '[NaN, Infinity, -Infinity]', '{"nested": {"deep": [[[{}]]], "empty": []}}', '  [ 1 , 2 ]  ',
                 '[0.1, 1e-5, 1e16, 1234567890123456.0, 100.0, 5e-324, 1.7976931348623157e308, 2.5e-7, 123456789.123, 0.0001, 1e15]',
                 '12345678901234567890123', '"\\ud800 lone"']
        for text in cases:
            for ensure_ascii in (False, True):
                for separators in ((', ', ': '), (',', ':')):
                    with self.subTest(text=text, ensure_ascii=ensure_ascii, separators=separators):
                        expected = json.dumps(json.loads(text), ensure_ascii=ensure_ascii, separators=separators)
                        if '\\ud800' in text or '123456789012345678901' in text:
                            # A lone surrogate and an integer past 64 bits are the two things kept differently.
                            continue
                        self.assertEqual(swift('json.roundtrip', text, ensure_ascii, *separators), expected)
        for broken in ['﻿{}', '01', '[1,]', '{"a" 1}', '{"a":1,}', '[1 2]', '"tab\there"', 'nul', '"\\x"', '[', '', '1 2']:
            with self.subTest(broken=broken):
                with self.assertRaises(ValueError):
                    json.loads(broken)
                with self.assertRaises(SwiftError):
                    swift('json.roundtrip', broken)
        floats = [0.1, 0.5, 1e-5, 1e16, 1e15, 9999999999999998.0, 1234567890123456.0, 5e-324, 2.5e-7, -0.0, 3.0, -123.456, 1e300, 1e-300, 7.2]
        self.assertEqual(swift('python_repr', *floats), [repr(f) for f in floats])

    def write(self, directory, content):
        directory.mkdir(parents=True, exist_ok=True)
        (directory / 'preferences.json').write_bytes(content if isinstance(content, bytes) else json.dumps(content).encode())

    def test_preferences_read_the_same_file_the_same_way(self):
        base = {'schema': 1}
        cases = [None, b'not json', b'\xef\xbb\xbf{"schema": 1}', b'{"schema": 1, "theme": "dark\xff"}', {'schema': 2}, {'schema': 1.0, 'theme': 'dark'},
                 {'schema': True, 'theme': 'light'}, [1, 2],
                 dict(base, visible=['claude', 'codex-quota', 'chatgpt', 'bogus', 'claude']),
                 dict(base, visible=['claude', 7]),
                 dict(base, visible=['summary', 'claude'], modules=['summary']),
                 dict(base, visible=['summary', 'gemini'], modules=['summary']),
                 dict(base, visible=['summary', 'codex'], modules=['summary', 'sessions']),
                 dict(base, visible=['summary', 'claude']),
                 dict(base, visible=['claude'], modules=[]),
                 dict(base, appearance='solid', theme='bogus', allowCollapse='yes', claudeLiveUsage=True, cursorUsage=False),
                 dict(base, usageAlert=70), dict(base, usageAlert=90.0), dict(base, usageAlert=True), dict(base, usageAlert='90'),
                 dict(base, usageAlert=50), dict(base, usageAlert=0), dict(base, usageAlert=False),
                 dict(base, pinInterval=5), dict(base, pinInterval=1.0), dict(base, pinInterval=True), dict(base, pinInterval=3),
                 dict(base, currency='CNY', cnyRate='7.1'), dict(base, cnyRate=7), dict(base, cnyRate=25), dict(base, cnyRate=True),
                 dict(base, cnyRate=' 6.5 '), dict(base, cnyRate='inf'), dict(base, cnyRate=[7]),
                 dict(base, subscriptionPrices={'chatgpt': '20', 'claude-subscription': 200, 'bogus': 5, 'cursor-subscription': 0,
                                                'gemini-subscription': True}),
                 dict(base, balanceAlerts={'kimi-api': 50, 'deepseek|USD': '3', 'grok|EUR': 1, 'qoder-api': 10, 'kimi-api|CNY': 20,
                                           'grok': '1e3', 'chatgpt-api|USD': 4}),
                 dict(base, expandedProviders=['claude', 'x', 1, 'codex']), dict(base, expandedProvider='codex'),
                 dict(base, expandedProviders='claude', expandedProvider='codex'),
                 dict(base, revision=-1), dict(base, revision=True), dict(base, revision=1.5), dict(base, revision=123456789012345678),
                 dict(base, paths={'claude-app': '/x', 'codex': '', 'bogus': '/y', 'gemini': 5, 'claude': '/z'}),
                 dict(base, groupBy='project', menuBar='quota', sessionAlerts=True, serviceStatus=True, keepCleanedUsage=False,
                      resetReminder=1, configuredSubscriptions=['grok-subscription', 'chatgpt', 'gemini-subscription', 3])]
        for number, case in enumerate(cases):
            with self.subTest(case=case):
                directory = self.root / str(number)
                if case is not None:
                    self.write(directory, case)
                same(self, preferences.read(directory), swift('preferences.read', str(directory)))

    def test_preference_changes_write_the_same_file(self):
        here, there = self.root / 'python', self.root / 'swift'
        data = self.root / 'data'
        (data / 'real').mkdir(parents=True)
        (data / 'link').symlink_to(data / 'real')
        steps = [('module', 'claude', 'false'), ('module', 'codex-quota', 'true'), ('module', 'bogus', 'true'), ('module', 'summary', 'maybe'),
                 ('usage-alert', '70'), ('usage-alert', '75'), ('pin-interval', '5'), ('pin-interval', '2.0'),
                 ('cny-rate', '7.2'), ('cny-rate', ''), ('cny-rate', '30'), ('cny-rate', None), ('cny-rate', ' 6.9 '),
                 ('subscription-price', 'chatgpt', '20'), ('subscription-price', 'claude-subscription', '200.5'),
                 ('subscription-price', 'chatgpt', ''), ('subscription-price', 'chatgpt', 'abc'), ('subscription-price', 'grok-subscription', '8'),
                 ('balance-alert', 'kimi-api|USD', '5'), ('balance-alert', 'deepseek|CNY', '12.5'), ('balance-alert', 'kimi-api|USD', '0'),
                 ('balance-alert', None), ('balance-alert', 'kimi-api|EUR', '5'),
                 ('subscription', 'gemini-subscription', 'true'), ('subscription', 'kimi-subscription', 'true'),
                 ('subscription', 'gemini-subscription', 'false'), ('subscription', 'chatgpt', 'true'),
                 ('collapse', 'true'), ('expanded', 'claude', 'true'), ('expanded', 'codex', 'true'), ('expanded', 'claude', 'false'),
                 ('collapse', 'false'), ('expanded', 'bogus', 'true'), ('theme', 'dark'), ('theme', 'sepia'), ('appearance', 'solid'),
                 ('path', 'codex', str(data / 'link')), ('path', 'claude', str(data / 'real')), ('path', 'gemini', str(data / 'missing')),
                 ('auto-path', 'codex'), ('auto-path', 'claude'), ('auto-path', 'bogus'),
                 ('group', 'model'), ('group', 'folder'), ('menu-bar', 'cost'), ('menu-bar', 'everything'), ('currency', 'CNY'),
                 ('currency', 'EUR'), ('service-status', 'true'), ('session-alerts', 'true'), ('keep-usage', 'false'),
                 ('reset-reminder', 'true'), ('claude-live', 'true'), ('claude-oauth', 'true'), ('cursor-usage', 'true'),
                 ('cursor-usage', 'yes'), ('reset',), ('bogus-kind', 'x')]
        revision = 0
        for step in steps:
            with self.subTest(step=step):
                try:
                    expected = preferences.update(here, *step)
                except ValueError as error:
                    with self.assertRaises(SwiftError) as raised:
                        swift('preferences.update', str(there), *step)
                    self.assertIn(str(error), str(raised.exception))
                    continue
                got = swift('preferences.update', str(there), *step)
                self.assertGreater(got['revision'], revision)
                revision = got['revision']
                expected.pop('revision'); got.pop('revision')
                same(self, expected, got)
                written = [json.loads((d / 'preferences.json').read_text()) for d in (here, there)]
                for value in written:
                    value.pop('revision')
                same(self, *written)
                self.assertEqual((there / 'preferences.json').stat().st_mode & 0o777, 0o600)
        self.assertEqual(sorted(p.name for p in there.iterdir()), ['preferences.json'])

    def test_an_unreadable_file_is_kept_aside_before_it_is_replaced(self):
        for directory in (self.root / 'python', self.root / 'swift'):
            self.write(directory, b'{"schema": 1, "theme": "dark"')
        preferences.update(self.root / 'python', 'theme', 'light')
        swift('preferences.update', str(self.root / 'swift'), 'theme', 'light')
        for directory in (self.root / 'python', self.root / 'swift'):
            names = sorted(p.name for p in directory.iterdir())
            self.assertEqual(len(names), 2, names)
            self.assertRegex(names[1], r'^preferences\.unreadable-\d{8}-\d{6}\.json$')
            self.assertEqual((directory / names[1]).read_bytes(), b'{"schema": 1, "theme": "dark"')
            self.assertEqual((directory / names[1]).stat().st_mode & 0o777, 0o600)
        readable = self.root / 'fine.json'
        readable.write_text('{"schema": 1}')
        self.assertIsNone(files.keep_unreadable(readable))
        self.assertIsNone(swift('keep_unreadable', str(readable)))
        self.assertIsNone(swift('keep_unreadable', str(self.root / 'missing.json')))
        broken = self.root / 'broken.json'
        broken.write_text('[]')
        kept = swift('keep_unreadable', str(broken))
        self.assertTrue(re.search(r'/broken\.unreadable-\d{8}-\d{6}\.json$', kept), kept)
        again = swift('keep_unreadable', str(broken))
        self.assertTrue(again.endswith('-1.json'), again)

    def test_the_catalog_and_module_list_match(self):
        got = swift('catalog')
        self.assertEqual(got['manual'], list(catalog.MANUAL_SUBSCRIPTIONS))
        self.assertEqual(got['portal'], list(catalog.PORTAL_APIS))
        self.assertEqual(got['balance'], list(catalog.BALANCE_APIS))
        self.assertEqual(got['keyOnly'], list(catalog.KEY_ONLY_APIS))
        self.assertEqual(got['labels'], [s.label for s in catalog.SERVICES])
        self.assertEqual(got['modules'], [list(m) for m in preferences.MODULES])
        self.assertEqual(got['credentials'], [list(catalog.credential_names(s.key)) for s in catalog.SERVICES])


def reply(message='msg_1', request='req_1', session='s1', ts='2026-11-18T10:00:00.000Z', model='claude-opus-5-5', usage=None,
          cost=None, sidechain=None, cwd='/Users/someone/Code/starmeter', **extra):
    """One assistant line of a Claude transcript."""
    row = {'type': 'assistant', 'sessionId': session, 'timestamp': ts, 'cwd': cwd,
           'message': {'model': model, 'usage': usage or {'input_tokens': 10, 'output_tokens': 5, 'cache_read_input_tokens': 100,
                                                         'cache_creation_input_tokens': 20}}}
    if message is not None:row['message']['id'] = message
    if request is not None:row['requestId'] = request
    if cost is not None:row['costUSD'] = cost
    if sidechain is not None:row['isSidechain'] = sidechain
    row.update(extra)
    return row


def dump(database, archive=None):
    """Every table the reader writes, blobs as hex, in a fixed order."""
    conn = sqlite3.connect(database)
    try:
        if archive:conn.execute('ATTACH DATABASE ? AS kept', (archive,))
        def rows(sql):
            return sorted([[v.hex() if isinstance(v, bytes) else v for v in row] for row in conn.execute(sql)], key=repr)
        found = {'files': rows('SELECT * FROM files'), 'records': rows('SELECT * FROM records'),
                 'canonical': rows('SELECT * FROM canonical ORDER BY id'), 'metadata': rows('SELECT * FROM metadata')}
        if archive:found['kept'] = rows('SELECT * FROM kept.records')
        return found
    finally:
        conn.close()


@unittest.skipUnless(PROBE, 'Swift toolchain not installed')
class ClaudeReaderParityTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.logs = self.root / 'projects' / 'p'
        self.logs.mkdir(parents=True)

    def test_hashes_and_prices_match(self):
        for text in ['', 'abc', 'a' * 55, 'a' * 56, 'a' * 63, 'a' * 64, 'a' * 65, 'x' * 1000, '中文 😀']:
            self.assertEqual(swift('sha256', text), hashlib.sha256(text.encode()).hexdigest())
        for values in (['msg_1', 'req'], ['msg_1', 's', None], ['msg_1:advisor:1', 's', 1794967200.123], ['location', ['ab12', 77], 0],
                       ['/Users/someone/项目/x.jsonl'], [['b', 'a'], True], [None, False]):
            self.assertEqual(swift('claude.digest', *values), claude_usage.digest(*values).hex())
        price = __import__('types').ModuleType('price')
        exec(compile((ROOT / 'lib/swiftbar_plugin.py').read_text(), 'plugin', 'exec'), price.__dict__)
        for model in ['claude-opus-5-5', 'claude-opus-5-5-20261101', 'claude-fable-5', 'claude-fable-5-2', 'claude-haiku-5-5', 'claude-sonnet-4-5',
                      'claude-opus-5-50', 'gpt-5', 'claude-3-opus']:
            for metrics in ([10, 5, 20, 0, 100], [60000, 10, 30000, 20000, 1], [1, 2, 3, 4, 5]):
                for speed, geo, tier in (('standard', 'global', 'standard'), ('fast', 'us', 'batch'), ('standard', 'us-only', 'standard')):
                    with self.subTest(model=model, metrics=metrics, speed=speed):
                        self.assertEqual(swift('claude.price', model, metrics, speed, geo, tier), price.claude_price(model, metrics, speed, geo, tier))

    def write(self, name, rows, tail=b''):
        path = self.logs / name
        path.write_bytes(b''.join((json.dumps(r, ensure_ascii=False).encode() if isinstance(r, dict) else r) + b'\n' for r in rows) + tail)
        return path

    def compare(self, allowed=None, start=0, end=float('inf'), archive=False, note=''):
        files = sorted(str(p) for p in self.logs.rglob('*.jsonl'))
        here, there = str(self.root / 'python.sqlite3'), str(self.root / 'swift.sqlite3')
        kept = (str(self.root / 'kept-python.sqlite3'), str(self.root / 'kept-swift.sqlite3')) if archive else (None, None)
        events, failures = claude_usage.read_events(files, here, allowed, start, end, kept[0])
        got = swift('claude.read_events', files, there, sorted(allowed) if allowed is not None else None, start,
                    end if end != float('inf') else None, kept[1])
        expected = [dict(e, session=e['session'].hex() if e['session'] else None) for e in events]
        key = lambda e: (e['ts'], e['model'], e['metrics'], repr(e['cost']), e['session'] or '')
        with self.subTest(step=note):
            self.assertEqual(got['failures'], failures)
            same(self, sorted(expected, key=key), sorted(got['events'], key=key))
            self.assertEqual(dump(here, kept[0]), dump(there, kept[1]))
        return events

    def test_the_index_and_the_events_are_the_same(self):
        advisor = {'input_tokens': 7, 'output_tokens': 3, 'iterations': [
            {'type': 'advisor_message', 'model': 'claude-fable-5-2', 'input_tokens': 40, 'output_tokens': 9, 'speed': 'fast'},
            {'type': 'advisor_message', 'model': '', 'input_tokens': 1}, {'type': 'other', 'model': 'x', 'input_tokens': 1}]}
        self.write('a.jsonl', [
            reply(), reply(ts='2026-11-18T10:00:01Z'),                          # the same response streamed twice
            reply('msg_2', None, ts='2026-11-18T11:00:00+08:00', cost=0.25),       # no request ID
            reply('msg_2', None, ts='2026-11-18T11:00:00+08:00', sidechain=True),  # a sidechain replay of it
            reply('gw-7', None, ts='2026-11-18T12:00:00.5Z'), reply('gw-7', None, ts='2026-11-18T12:00:01.5Z'),  # gateway IDs by time
            reply(None, None, ts='2026-11-18T12:30:00Z'), reply(None, None, ts='2026-11-18T12:30:00Z'),  # no IDs: by place
            reply('msg_3', 'req_3', usage=advisor, cwd='C:\\\\Users\\\\x\\\\仓库\\\\'),
            reply('msg_4', 'req_4', usage={'input_tokens': 1, 'cache_creation': {'ephemeral_5m_input_tokens': 5, 'ephemeral_1h_input_tokens': 6},
                                           'service_tier': 'batch', 'inference_geo': 'us', 'speed': 'fast'}),
            reply('msg_5', 'req_5', usage={'input_tokens': 0, 'output_tokens': 0}),            # nothing counted
            reply('msg_6', 'req_6', usage={'input_tokens': 0}, cost=0.0),                      # but a recorded cost counts
            reply('msg_7', 'req_7', usage={'input_tokens': 1.5}), reply('msg_8', 'req_8', usage={'input_tokens': True}),
            reply('msg_9', 'req_9', usage={'input_tokens': -1}), reply('msg_10', 'req_10', model='<synthetic>'),
            reply('msg_11', 'req_11', ts='2026-11-18T10:00:00'), reply('msg_12', 'req_12', ts=1794967200),
            reply('msg_13', 'req_13', cost=float('nan')), reply('msg_14', 'req_14', cost=True), reply('msg_15', ''), reply('', 'req_16'),
            reply('msg_17', 'req_17', session=''), reply('msg_18', 'req_18', session=7), reply('msg_19', 'req_19', model=None),
            {'type': 'user', 'message': {'content': 'usage assistant'}}, b'{"type": "assistant", "usage": broken',
            reply('msg_20', 'req_20', usage={'input_tokens': 2 ** 64}),
        ], tail=json.dumps(reply('msg_21', 'req_21')).encode())  # still being written: no newline yet
        self.write('b.jsonl', [reply(), reply('msg_2', None, ts='2026-11-18T11:00:00+08:00'), reply('msg_30', 'req_30', session='s2')])
        nested = self.logs / 'session-x' / 'subagents'
        nested.mkdir(parents=True)
        (nested / 'agent.jsonl').write_text(json.dumps(reply('msg_40', 'req_40', session=None, sidechain=True)) + '\n')
        self.compare(note='first read')
        self.compare(note='nothing changed')
        with open(self.logs / 'a.jsonl', 'ab') as handle:
            handle.write(b'\n' + json.dumps(reply('msg_22', 'req_22', ts='2026-11-19T01:00:00Z')).encode() + b'\n')
        self.compare(note='appended')
        with open(self.logs / 'b.jsonl', 'ab') as handle:
            handle.write(json.dumps(reply()).encode() + b'\n')
        self.compare(note='appended a duplicate')
        self.write('b.jsonl', [reply('msg_31', 'req_31')])
        self.compare(note='rewritten')
        self.compare(start=1794967200.5, end=1794970000, note='a range')
        self.compare(allowed={'s1', 'session-x'}, note='only some sessions')
        self.compare(archive=True, note='archive on')
        (self.logs / 'b.jsonl').unlink()
        self.compare(archive=True, note='a file vanished into the archive')
        self.write('b.jsonl', [reply('msg_31', 'req_31')])
        self.compare(archive=True, note='it came back')
        self.compare(note='archive off')

    def test_real_transcripts_agree(self):
        """On this machine's own Claude transcripts, when there are any; nothing from them is printed."""
        roots = [Path(p) for p in os.environ.get('STARMETER_REAL_CLAUDE', '').split(os.pathsep) if p]
        files = sorted(str(p) for root in roots if root.is_dir() for p in root.rglob('*.jsonl'))
        if not files:
            self.skipTest('set STARMETER_REAL_CLAUDE to a projects folder to compare on real transcripts')
        here, there = str(self.root / 'python.sqlite3'), str(self.root / 'swift.sqlite3')
        events, failures = claude_usage.read_events(files, here)
        got = swift('claude.read_events', files, there, None, 0, None, None)
        self.assertEqual((len(got['events']), got['failures']), (len(events), failures))
        self.assertEqual(dump(here)['canonical'], dump(there)['canonical'])


if __name__ == '__main__':
    unittest.main(verbosity=2)
