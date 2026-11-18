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
from datetime import datetime, timezone
import time
import unittest.mock

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'lib'))
import swiftbar_accounts as accounts
import swiftbar_catalog as catalog
import swiftbar_claude as claude_desktop
import swiftbar_claude_live as claude_live
import swiftbar_claude_local as claude_local
import swiftbar_claude_oauth as claude_oauth
import swiftbar_claude_usage as claude_usage
import swiftbar_cursor as cursor
import swiftbar_sources as sources
import swiftbar_files as files
import swiftbar_preferences as preferences
import swiftbar_qoder as qoder
import swiftbar_sessions as live_sessions
import swiftbar_status as service_status
import swiftbar_update_channels as update_channels
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
    built = subprocess.run([swift, 'build', '--package-path', str(ROOT / 'engine'), '--scratch-path', str(scratch)], capture_output=True, text=True)
    if built.returncode:
        raise RuntimeError('swift build failed:\n' + built.stdout + built.stderr)
    path = scratch / 'debug' / 'starmeter-probe'
    return str(path) if path.is_file() else None


PROBE = probe_binary()
# The program the hosts run, built beside the probe.
ENGINE = str(Path(PROBE).parent / 'starmeter-engine') if PROBE else None


class SwiftError(Exception):
    pass


def swift(call, *args):
    environment = dict(os.environ, STARMETER_PRICING=str(ROOT / 'lib'))
    done = subprocess.run([PROBE], input=json.dumps({'call': call, 'args': list(args)}).encode(), capture_output=True, check=True, env=environment)
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


def codex_rows(session='0199-a', cwd='/w/starmeter', provider='openai', start=1794967200, model='gpt-6.2-sol'):
    """A Codex rollout: metadata, a tier change, and token counts by delta and by running total."""
    def at(n):return '2026-11-18T%02d:%02d:%02d.%03dZ' % (10 + n // 3600, n // 60 % 60, n % 60, n % 1000)
    count = lambda n, total, last=None: {'timestamp': at(n), 'type': 'event_msg', 'payload': {'type': 'token_count', 'info': {
        'total_token_usage': total, 'last_token_usage': last, 'model_context_window': 400000}}}
    return [
        {'timestamp': at(0), 'type': 'session_meta', 'payload': {'id': session, 'cwd': cwd, 'model_provider': provider}},
        {'timestamp': at(1), 'type': 'turn_context', 'payload': {'model': model}},
        {'timestamp': at(2), 'type': 'event_msg', 'payload': {'type': 'thread_settings_applied', 'thread_settings': {'service_tier': 'priority'}}},
        count(3, {'input_tokens': 1000, 'cached_input_tokens': 200, 'output_tokens': 50, 'total_tokens': 1050},
              {'input_tokens': 1000, 'cached_input_tokens': 200, 'output_tokens': 50, 'total_tokens': 1050}),
        count(4, {'input_tokens': 1000, 'cached_input_tokens': 200, 'output_tokens': 50, 'total_tokens': 1050}),  # the same total again
        count(5, {'input_tokens': 3000, 'cached_input_tokens': 2500, 'output_tokens': 90, 'total_tokens': 3090}),  # a delta from totals
        {'timestamp': at(6), 'type': 'event_msg', 'payload': {'type': 'thread_settings_applied', 'thread_settings': {'service_tier': 'odd'}}},
        count(7, None, {'input_tokens': 400, 'cached_input_tokens': 900, 'output_tokens': 7}),  # cached above input
        {'timestamp': at(8), 'type': 'turn_context', 'payload': {'model': 'codex-auto-review'}},
        count(9, {'input_tokens': 5000, 'output_tokens': 100}, {'input_tokens': 120, 'output_tokens': 3}),
        {'timestamp': at(10), 'type': 'response_item', 'payload': {'type': 'message', 'content': 'token_count is said here but not counted'}},
        {'type': 'event_msg', 'payload': {'type': 'token_count', 'info': {'last_token_usage': {'input_tokens': 9}}}},  # no time: skipped
        {'timestamp': at(11), 'type': 'event_msg', 'payload': ['not', 'a', 'dict', 'token_count']},
    ]


def jsonl(path, rows, tail=b''):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(b''.join((r if isinstance(r, bytes) else json.dumps(r, ensure_ascii=False).encode()) + b'\n' for r in rows) + tail)
    return path


@unittest.skipUnless(PROBE, 'Swift toolchain not installed')
class HarnessReaderParityTests(unittest.TestCase):
    """Every reader other than Claude's: results, and the index files they leave, must match."""
    zone = 'Asia/Shanghai'
    start, end = 1794931200.0, 1795017599.999  # 2026-11-18 in Shanghai

    @classmethod
    def setUpClass(cls):
        import types
        cls.plugin = types.ModuleType('plugin_for_parity')
        exec(compile((ROOT / 'lib/swiftbar_plugin.py').read_text(), 'plugin', 'exec'), cls.plugin.__dict__)

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        os.environ['STARMETER_PRICING'] = str(ROOT / 'lib')

    def carriers(self, specs, cache):
        found = []
        for name, path, kind, agents in specs:
            if path is None:
                found.append((name, None))
            else:
                found.append((name, sources.requests_from(sources.gateway_requests(path, cache, self.start, self.end, kind), agents)))
        return found

    def compare(self, source, root, note, exclude=False, specs=(), archive=False, start=None, end=None):
        start = self.start if start is None else start
        end = self.end if end is None else end
        here, there = self.root / 'cache-python', self.root / 'cache-swift'
        kept = (str(self.root / 'kept-python.sqlite3'), str(self.root / 'kept-swift.sqlite3')) if archive else (None, None)
        zone = ZoneInfo(self.zone)
        carriers = self.carriers(specs, here) if specs else None
        if source == 'gemini':
            expected = sources.gemini_usage(root, here, start, end, zone, exclude, carriers, kept[0])
        elif source == 'opencode':
            expected = sources.opencode_usage(root, start, end, zone, exclude, carriers)
        else:
            expected = sources.jsonl_usage(source, root, here, start, end, zone, exclude, carriers,
                                           price=self.plugin.magpie_price if source == 'magpie' else None, archive=kept[0])
        got = swift('harness.read', source, str(root), str(there), start, end, self.zone, exclude, kept[1],
                    [[name, str(path) if path else None, kind, sorted(agents) if agents else None] for name, path, kind, agents in specs])
        with self.subTest(step=note):
            same(self, expected, got)
            for python_db in sorted(here.glob('*.sqlite3')):
                swift_db = there / python_db.name
                self.assertTrue(swift_db.exists(), python_db.name)
                self.assertEqual(table_dump(python_db), table_dump(swift_db), python_db.name)
            if archive and Path(kept[0]).exists():
                self.assertEqual(table_dump(kept[0]), table_dump(kept[1]))
        return expected

    def test_codex(self):
        codex = self.root / 'codex'
        day = codex / 'sessions' / '2026' / '11' / '18'
        jsonl(day / 'rollout-2026-11-18T10-00-00-0199-a.jsonl', codex_rows(), tail=b'{"timestamp": "2026-11-18T11:00:00Z"')
        jsonl(day / 'rollout-2026-11-18T10-30-00-0199-b.jsonl', codex_rows('0199-b', '/w/other', 'opencodex', model='gpt-5'))
        jsonl(codex / 'archived_sessions' / '2026' / '11' / '18' / 'rollout-2026-11-18T10-00-00-0199-a.jsonl', codex_rows())
        jsonl(day / 'rollout-bad.jsonl', codex_rows('0199-c')[:3] + [{'timestamp': 'yesterday', 'type': 'event_msg', 'payload': {
            'type': 'token_count', 'info': {'last_token_usage': {'input_tokens': 5}}}}])
        self.compare('codex', codex, 'first read')
        self.compare('codex', codex, 'again, nothing changed')
        self.compare('codex', codex, 'OpenCodex sessions left out', exclude=True)
        with open(day / 'rollout-2026-11-18T10-00-00-0199-a.jsonl', 'ab') as handle:
            handle.write(b', "type": "event_msg", "payload": {"type": "token_count", "info": {"last_token_usage": {"input_tokens": 77, "output_tokens": 1}}}}\n')
        self.compare('codex', codex, 'the half-written line finished')
        self.compare('codex', codex, 'archive on', archive=True)
        (day / 'rollout-2026-11-18T10-30-00-0199-b.jsonl').unlink()
        self.compare('codex', codex, 'a session cleaned up', archive=True)
        self.compare('codex', codex, 'a narrower range', archive=True, start=1794996003, end=1794996006)

    def test_codex_chats_without_a_project_go_by_their_title(self):
        # The ChatGPT app runs a chat with no project in Documents/Codex/<day>/<slug>; a Guardian review in it has a parent.
        codex = self.root / 'codex'
        day = codex / 'sessions' / '2026' / '11' / '18'
        chat = '/Users/u/Documents/Codex/2026-11-18/https-github-com-x'
        jsonl(day / 'rollout-2026-11-18T10-00-00-chat.jsonl', codex_rows('chat-1', chat))
        guardian = codex_rows('guard-1', 'file://' + chat + '/', model='codex-auto-review')
        guardian[0]['payload']['parent_thread_id'] = 'chat-1'
        jsonl(day / 'rollout-2026-11-18T10-10-00-guard.jsonl', guardian)
        jsonl(day / 'rollout-2026-11-18T10-20-00-new.jsonl', codex_rows('chat-2', 'C:\\Users\\u\\Documents\\Codex\\2026-11-18\\wo-d'))
        jsonl(day / 'rollout-2026-11-18T10-30-00-repo.jsonl', codex_rows('chat-3', chat + '/work/dsh-web-plugins'))
        jsonl(day / 'rollout-2026-11-18T10-40-00-proj.jsonl', codex_rows('chat-4', 'file:///Users/u/.codex/.chatgpt-projects/g-p-6aab52'))
        jsonl(day / 'rollout-2026-11-18T10-50-00-gone.jsonl', codex_rows('chat-5', '/Users/u/.codex/.chatgpt-projects/g-p-0bad/'))
        jsonl(day / 'rollout-2026-11-18T11-00-00-space.jsonl', codex_rows('chat-6', 'file:///Users/u/Documents/SwiftBar%20plugin'))
        (codex / 'session_index.jsonl').write_text(json.dumps({'id': 'chat-1', 'thread_name': '为 Token Monitor 设计 Logo'}) + '\n'
                                                   + json.dumps({'id': 'guard-1', 'thread_name': 'Guardian review'}) + '\n')
        (codex / '.codex-global-state.json').write_text(json.dumps({'local-projects': {'g-p-6aab52': {'name': ' charles 1 ', 'rootPaths': []}, 'x': {'name': ''}}}))
        result = self.compare('codex', codex, 'chats by title')
        self.assertEqual({row['project'] for row in result['projects']},
                         {'为 Token Monitor 设计 Logo', 'ChatGPT 对话', 'dsh-web-plugins', 'charles 1', 'ChatGPT 项目', 'SwiftBar plugin'})

    def test_gateways_carry_requests(self):
        codex = self.root / 'codex'
        jsonl(codex / 'sessions' / '2026' / '11' / '18' / 'rollout-x.jsonl', codex_rows())
        log = jsonl(self.root / 'opencodex-usage.jsonl', [
            {'requestId': 'r1', 'timestamp': 1794996003000, 'durationMs': 1500, 'usage': {'inputTokens': 1000, 'cachedInputTokens': 200, 'outputTokens': 50}},
            {'requestId': 'r2', 'timestamp': 1794996005000, 'usage': {'inputTokens': 2000, 'cachedInputTokens': 2300, 'outputTokens': 40}},
            {'timestamp': 1794996005000, 'usage': 'none'}, b'not json'])
        magpie = self.root / 'magpie'
        jsonl(magpie / 'usage.jsonl', [
            {'t': '2026-11-18T10:00:05Z', 'ms': 900, 'provider': 'openai', 'model': 'gpt-6.2-sol', 'agent': 'codex', 'in': 1700, 'cache_read': 1300, 'out': 40},
            {'t': '2026-11-18T10:00:07Z', 'provider': 'openai', 'model': 'gpt-6.2-sol', 'agent': 'claude', 'in': 400, 'cache_read': 900, 'out': 7},
            {'t': '2026-11-18T10:00:08Z', 'provider': 'anthropic', 'model': 'claude-opus-5-5', 'agent': 'claude-code', 'in': 10, 'out': 5,
             'cache_write': 30, 'cache_write_1h': 20, 'tier_sent': 'fast', 'session': 's1'},
            {'t': '2026-11-18T10:00:09Z', 'provider': 'x', 'model': 'deepseek-chat', 'source': 'reported', 'agent': 'cursor', 'in': 5, 'out': 5, 'cost': 0.01},
            {'t': '2026-11-18T10:00:10Z', 'provider': 'x', 'model': 'm', 'source': 'probe', 'in': 5},
            {'t': '2026-11-18T10:00:11Z', 'provider': 'x', 'model': 'm', 'rejected': True, 'in': 5},
            {'t': '2026-11-18T10:00:12Z', 'provider': 'x', 'model': 'm', 'status': 500, 'in': 5},
            {'t': '2026-11-18T10:00:13Z', 'provider': 'x', 'model': 'routing', 'served': 'kimi-k2', 'cache_unknown': True, 'in': 50, 'out': 5},
            {'t': 1794996014, 'provider': 'x', 'model': 'gpt-5', 'tier_served': 'priority', 'in': 3, 'out': 3},
        ])
        specs = [('OpenCodex', log, 'opencodex', None), ('Magpie', magpie / 'usage.jsonl', 'magpie', {'codex'})]
        self.compare('codex', codex, 'carried by both gateways', specs=specs)
        self.compare('codex', codex, 'OpenCodex unreadable', specs=[('OpenCodex', None, 'opencodex', None)])
        self.compare('magpie', magpie, 'Magpie on its own')
        for kind, path in (('opencodex', log), ('magpie', magpie / 'usage.jsonl')):
            expected = sources.gateway_requests(path, self.root / 'g-python', self.start, self.end, kind)
            got = swift('harness.gateway', str(path), str(self.root / 'g-swift'), self.start, self.end, kind)
            self.assertEqual([[agent, sorted([*split, times] for split, times in requests.items())] for agent, requests in expected.items()],
                             [[agent, [[*row[:4], row[4]] for row in requests]] for agent, requests in got])

    def test_deepseek_workbuddy_and_qoder(self):
        dsh = self.root / 'dsh'
        session = [{'type': 'session', 'version': 3, 'id': 'd1'},
                   {'type': 'request/header', 'data': {'header': {'config': {'model': 'deepseek-v4', 'provider': 'deepseek'}}}},
                   {'type': 'assistant/message', 'seq': 3, 'time': '2026-11-18T10:00:00Z', 'data': {'turn': 1, 'step': 1,
                    'usage': {'inputTokens': 100, 'outputTokens': 20, 'cacheReadTokens': 50, 'cacheWriteTokens': 5}}},
                   {'type': 'llm/retry-started'},
                   {'type': 'assistant/attempt', 'seq': 4, 'time': 1794996100000, 'data': {'turn': 1, 'step': 2, 'message': {'model': 'deepseek-v4-pro'},
                    'stream': [{'chunk': {'type': 'text'}}, {'type': 'usage', 'usage': {'inputTokens': 9, 'outputTokens': 1}}]}}]
        jsonl(dsh / 'a' / 'session.jsonl', session)
        jsonl(dsh / 'a' / 'session.v2.jsonl', session[:3])
        jsonl(dsh / 'b' / 'session.jsonl', [{'type': 'session', 'version': 9, 'id': 'old'}] + session[1:])  # unsupported: the file fails
        jsonl(dsh / 'c' / 'session.jsonl', [{'type': 'session', 'version': 4, 'id': 'seeded', 'isSeeded': True, 'seedLength': 2}] + session[1:]
              + [{'type': 'session/end-seed', 'data': {'inherited': True}}, dict(session[2], seq=9)])
        self.compare('deepseek-harness', dsh, 'DeepSeek Harness')
        work = self.root / 'home'
        jsonl(work / '.workbuddy' / 'projects' / 'p' / 's.jsonl', [
            {'type': 'message', 'sessionId': 'w1', 'timestamp': '2026-11-18T10:00:00Z', 'message': {'role': 'assistant', 'id': 'm1', 'model': 'glm-5',
             'usage': {'input_tokens': 100, 'cache_read_input_tokens': 30, 'output_tokens': 10}}},
            {'type': 'function_call', 'timestamp': '2026-11-18T10:00:01Z', 'providerData': {'messageId': 'm1', 'model': 'glm-5', 'provider': 'zhipu',
             'usage': {'input_tokens': 100, 'output_tokens': 10}}},
            {'type': 'message', 'timestamp': '2026-11-18T10:00:02Z', 'message': {'role': 'user', 'usage': {'input_tokens': 1}}},
            {'type': 'assistant', 'timestamp': '2026-11-18T10:00:03Z', 'id': 'r3', 'message': {'usage': {'input_tokens': 5}}}])
        self.compare('workbuddy', work, 'WorkBuddy')
        qoder = self.root / 'qoder'
        jsonl(qoder / 'projects' / 'p' / 'q.jsonl', [
            {'type': 'assistant', 'sessionId': 'q1', 'cwd': '/w/app', 'timestamp': '2026-11-18T10:00:00Z',
             'message': {'id': 'm1', 'model': 'qfmodel', 'usage': {'input_tokens': 0, 'output_tokens': 0, 'credits': 1.5}}},
            {'type': 'assistant', 'sessionId': 'q1', 'timestamp': '2026-11-18T10:00:01Z',
             'message': {'id': 'm1', 'model': 'qfmodel', 'usage': {'input_tokens': 0, 'output_tokens': 0, 'credits': 2.5}}},
            {'type': 'assistant', 'uuid': 'u2', 'timestamp': '2026-11-18T10:00:02Z',
             'message': {'model': 'qoder-custom-12/claude-opus-5-5', 'usage': {'input_tokens': 10, 'output_tokens': 4, 'cached_tokens': 3}}},
            {'type': 'assistant', 'timestamp': 'later', 'message': {'id': 'm9', 'usage': {'input_tokens': 10}}}])
        self.compare('qoder', qoder, 'Qoder CN')

    def test_gemini_and_opencode(self):
        gemini = self.root / 'gemini'
        chats = gemini / 'tmp' / 'abc' / 'chats'
        chats.mkdir(parents=True)
        message = lambda i, **usage: {'type': 'gemini', 'id': 'g%d' % i, 'model': 'gemini-3-pro', 'timestamp': '2026-11-18T10:00:%02dZ' % i,
                                      'tokens': dict({'input': 100, 'output': 10, 'cached': 20, 'thoughts': 5, 'tool': 2, 'total': 117}, **usage)}
        (chats / 'session-1.json').write_text(json.dumps({'sessionId': 'gs1', 'messages': [message(1), message(2, total=0), {'type': 'user'}]}))
        jsonl(chats / 'session-2.jsonl', [{'sessionId': 'gs2'}, message(3), {'$set': {'messages': [message(3), message(4)]}}, b'not json'])
        jsonl(chats / 'sub' / '7f3c.jsonl', [message(5)])
        (chats / 'session-3.json').write_text(json.dumps({'messages': [{'type': 'gemini', 'id': 'g9', 'tokens': {'input': 1}}]}))  # no time
        self.compare('gemini', gemini, 'Gemini CLI')
        opencode = self.root / 'opencode'
        opencode.mkdir()
        conn = sqlite3.connect(opencode / 'opencode.db')
        conn.execute('CREATE TABLE message(id TEXT, session_id TEXT, time_created INTEGER, data TEXT)')
        rows = [('m1', 's1', 1794996000123, {'role': 'assistant', 'modelID': 'deepseek-chat', 'providerID': 'deepseek', 'path': {'cwd': '/w/x/'},
                                              'tokens': {'input': 100, 'output': 9, 'cache': {'read': 50, 'write': 3}}}),
                ('m2', 's1', 1794996001000, {'role': 'assistant', 'modelID': 'kimi-k2', 'cost': 0.02, 'tokens': {'input': 1, 'output': 1}}),
                ('m3', 's2', 1794996002000, {'role': 'user', 'tokens': {'input': 5}}),
                ('m4', 's2', 1794996003000, {'role': 'assistant', 'tokens': 'none'})]
        conn.executemany('INSERT INTO message VALUES (?,?,?,?)', [(a, b, c, json.dumps(d)) for a, b, c, d in rows])
        conn.commit(); conn.close()
        self.compare('opencode', opencode, 'OpenCode')

    def test_compressed_codex_archives(self):
        try:
            from compression import zstd
        except ImportError:
            self.skipTest('this Python has no zstd')
        codex = self.root / 'codex'
        archived = codex / 'archived_sessions' / '2026' / '11' / '18'
        archived.mkdir(parents=True)
        body = b''.join(json.dumps(r).encode() + b'\n' for r in codex_rows('0199-z'))
        (archived / 'rollout-z.jsonl.zst').write_bytes(zstd.compress(body, level=3))
        (archived / 'rollout-broken.jsonl.zst').write_bytes(b'\x28\xb5\x2f\xfd' + b'\x00' * 12)
        self.compare('codex', codex, 'compressed archives')


def table_dump(path):
    conn = sqlite3.connect(str(path))
    try:
        found = {}
        for (name,) in conn.execute("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name"):
            found[name] = sorted(([v.hex() if isinstance(v, bytes) else v for v in row] for row in conn.execute('SELECT * FROM "%s"' % name)), key=repr)
        return found
    finally:
        conn.close()


@unittest.skipUnless(PROBE and ZoneInfo, 'Swift toolchain not installed')
class RangeParityTests(unittest.TestCase):
    """The whole range bundle, from preferences to every selected source, read by both engines."""

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        root = Path(self.temp.name)
        self.home = root / 'home'
        now = time.time()
        self.at = lambda seconds: datetime.fromtimestamp(now - seconds, timezone.utc).isoformat().replace('+00:00', 'Z')
        jsonl(self.home / '.claude' / 'projects' / 'p' / 's1.jsonl', [
            reply('msg_a', 'req_a', ts=self.at(100)), reply('msg_b', 'req_b', ts=self.at(90), model='claude-fable-5-2', cost=0.5),
            reply('msg_c', None, ts=self.at(80 * 3600), cwd='/w/older')])
        codex_day = self.home / '.codex' / 'sessions' / '2026' / '11' / '18'
        rows = codex_rows()
        for index, row in enumerate(rows):
            if 'timestamp' in row:row['timestamp'] = self.at(200 - index)
        jsonl(codex_day / 'rollout-x.jsonl', rows)
        magpie = self.home / '.config' / 'magpie'
        jsonl(magpie / 'usage.jsonl', [
            {'t': self.at(197), 'provider': 'openai', 'model': 'gpt-6.2-sol', 'agent': 'codex', 'in': 1000, 'out': 50, 'cache_read': 200},
            {'t': self.at(60), 'provider': 'anthropic', 'model': 'claude-opus-5-5', 'agent': 'claude-code', 'in': 10, 'out': 5, 'cache_read': 100,
             'cache_write': 20}])
        jsonl(self.home / '.workbuddy' / 'projects' / 'p' / 'w.jsonl', [
            {'type': 'message', 'timestamp': self.at(50), 'message': {'role': 'assistant', 'id': 'w1', 'model': 'glm-5', 'usage': {'input_tokens': 9, 'output_tokens': 1}}}])
        self.states = {}
        for engine in ('python', 'swift'):
            state = root / engine / 'state'
            state.mkdir(parents=True)
            (state / 'preferences.json').write_text(json.dumps({'schema': 1, 'visible': ['summary', 'claude', 'codex', 'magpie', 'workbuddy'], 'modules': [m[0] for m in preferences.MODULES]}))
            self.states[engine] = {'OCX_STATE_DIR': str(state), 'OCX_CACHE_DIR': str(root / engine / 'cache')}
        self.common = {'HOME': str(self.home), 'OCX_TIMEZONE': 'Asia/Shanghai', 'OCX_HOME': str(self.home / '.opencodex'),
                       'OCX_PLUGIN_PATH': str(ROOT / 'starmeter.30s.sh'), 'STARMETER_PRICING': str(ROOT / 'lib')}

    def python(self, key):
        import types
        with unittest.mock.patch.dict(os.environ, dict(self.common, **self.states['python'])):
            for name in ('CLAUDE_CONFIG_DIR', 'XDG_CONFIG_HOME', 'CODEX_HOME', 'OCX_BASE_URL', 'TZ'):os.environ.pop(name, None)
            plugin = types.ModuleType('plugin_range')
            exec(compile((ROOT / 'lib/swiftbar_plugin.py').read_text(), 'plugin', 'exec'), plugin.__dict__)
            payload, error = plugin.range_usage_cached(key, ttl=0)
            return payload, error, plugin.range_cache_name(key)

    def swift(self, key):
        environment = {k: v for k, v in os.environ.items() if k not in ('CLAUDE_CONFIG_DIR', 'XDG_CONFIG_HOME', 'CODEX_HOME', 'OCX_BASE_URL', 'TZ')}
        environment.update(self.common, **self.states['swift'])
        done = subprocess.run([PROBE], input=json.dumps({'call': 'range.usage', 'args': [key, 0]}).encode(), capture_output=True, check=True, env=environment)
        answer = json.loads(done.stdout)
        if 'error' in answer:raise SwiftError(answer['error'])
        return answer['result']['payload'], answer['result']['error'], answer['result']['cache']

    @staticmethod
    def steady(value):
        """Without what depends on the moment of reading: read times and the range's end, which is now."""
        if isinstance(value, dict):
            return {k: RangeParityTests.steady(v) for k, v in value.items() if k not in ('readAt', '_fetchedAt', 'end', '_stale')}
        if isinstance(value, list):
            return [RangeParityTests.steady(v) for v in value]
        return value

    def test_every_range_bundle_matches(self):
        for key in ('today', 'all', '7d', 'yesterday'):
            with self.subTest(range=key):
                expected = self.python(key)
                got = self.swift(key)
                self.assertEqual(expected[2], got[2], 'the cache name')
                self.assertEqual(expected[1], got[1])
                same(self, self.steady(expected[0]), self.steady(got[0]))
                self.assertTrue(expected[0]['claude']['summary']['requests'] or key == 'yesterday')

    def test_the_dashboard_matches(self):
        import types
        payload, _, _ = self.python('today')
        week, _, _ = self.python('7d')
        everything, _, _ = self.python('all')
        frozen = time.time()
        variants = [dict(groupBy='tool'), dict(groupBy='model'), dict(groupBy='project'),
                    dict(currency='CNY', cnyRate=7.1234, serviceStatus=True, allowCollapse=True, expandedProviders=['claude']),
                    dict(currency='CNY', visible=['claude', 'magpie', 'heatmap']), dict(visible=['summary', 'codex', 'heatmap', 'workbuddy'])]
        datas = [('today', {'range_usage': payload, 'displayed_range': 'today', 'healthz': {'ok': True},
                            'exchange_rate': {'rate': 7.2, 'date': '2026-11-17'},
                            'service_status': {'claude': {'indicator': 'minor', 'description': 'x'}, 'openai': {'error': 'URLError'}}}),
                 ('7d', {'range_usage': week, 'displayed_range': '7d'}), ('all', {'range_usage': everything, 'displayed_range': 'all'}),
                 ('today', {'displayed_range': 'today'}), ('30d', {'range_usage': week, 'displayed_range': '30d'})]
        for variant in variants:
            for engine in ('python', 'swift'):
                state = Path(self.states[engine]['OCX_STATE_DIR'])
                base = {'schema': 1, 'visible': ['summary', 'heatmap', 'claude', 'codex', 'magpie', 'workbuddy'], 'modules': [m[0] for m in preferences.MODULES]}
                (state / 'preferences.json').write_text(json.dumps(dict(base, **variant)))
            for key, data in datas:
                with self.subTest(variant=variant, range=key, data=sorted(data)):
                    with unittest.mock.patch.dict(os.environ, dict(self.common, **self.states['python'])):
                        plugin = types.ModuleType('plugin_dash')
                        exec(compile((ROOT / 'lib/swiftbar_plugin.py').read_text(), 'plugin', 'exec'), plugin.__dict__)
                        plugin.usage_now = lambda: datetime.fromtimestamp(frozen, plugin.local_zone())
                        expected = plugin.dashboard_payload(data)
                    environment = {k: v for k, v in os.environ.items() if k not in ('CLAUDE_CONFIG_DIR', 'XDG_CONFIG_HOME', 'CODEX_HOME', 'OCX_BASE_URL', 'TZ')}
                    environment.update(self.common, **self.states['swift'])
                    done = subprocess.run([PROBE], input=json.dumps({'call': 'dashboard', 'args': [data, frozen, None]}).encode(),
                                          capture_output=True, check=True, env=environment)
                    answer = json.loads(done.stdout)
                    self.assertNotIn('error', answer)
                    same(self, expected, answer['result'])

    def test_range_bounds_follow_the_wall_clock(self):
        import types
        plugin = types.ModuleType('plugin_bounds')
        with unittest.mock.patch.dict(os.environ, dict(self.common, **self.states['python'])):
            exec(compile((ROOT / 'lib/swiftbar_plugin.py').read_text(), 'plugin', 'exec'), plugin.__dict__)
        for zone in ('Asia/Shanghai', 'America/New_York', 'Europe/London', 'UTC'):
            for moment in (1794996000.25, 1772953200.0, 1772960400.5, 1793512800.0, 1790000000.0, 1796083199.999):
                now = datetime.fromtimestamp(moment, ZoneInfo(zone))
                for key in ('today', 'yesterday', '24h', '7d', '30d', 'week', 'month', 'all'):
                    with self.subTest(zone=zone, moment=moment, key=key):
                        self.assertEqual(swift('range.bounds', key, moment, zone), list(plugin.range_bounds(key, now)))


class FakeServices:
    """A local HTTP server standing in for every provider: both engines are pointed at it, and answer the same."""

    def __init__(self, handler):
        import http.server, threading, urllib.request
        outer = self
        self.handler = handler
        class Handler(http.server.BaseHTTPRequestHandler):
            def do_POST(self):
                self.headers['X-Body'] = self.rfile.read(int(self.headers.get('Content-Length') or 0)).decode()
                self.do_GET()
            def do_GET(self):
                status, body, headers = outer.handler(self.path, dict(self.headers))
                self.send_response(status)
                for name, value in headers.items():self.send_header(name, value)
                self.send_header('Content-Length', str(len(body)))
                self.end_headers()
                self.wfile.write(body)
            def log_message(self, *args):pass
        self.server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Handler)
        threading.Thread(target=self.server.serve_forever, daemon=True).start()
        self.base = 'http://127.0.0.1:%d' % self.server.server_address[1]
        hosts = ['https://api.deepseek.com', 'https://management-api.x.ai', 'https://api.qoder.com.cn', 'https://www.minimax.io', 'https://api.minimaxi.com',
                 'https://api.moonshot.cn', 'https://api.moonshot.ai', 'https://api.anthropic.com', 'https://api.openai.com', 'https://generativelanguage.googleapis.com',
                 'https://cursor.com', 'https://status.claude.com', 'https://status.openai.com', 'https://status.cursor.com', 'https://api.frankfurter.dev']
        self.routes = {host: self.base + '/' + host.split('//')[1] for host in hosts}
        original = urllib.request.OpenerDirector.open
        routes = self.routes
        def rerouted(director, request, *args, **kwargs):
            if isinstance(request, urllib.request.Request):
                for host, target in routes.items():
                    if request.full_url.startswith(host):request.full_url = target + request.full_url[len(host):];break
            return original(director, request, *args, **kwargs)
        self.patch = unittest.mock.patch.object(urllib.request.OpenerDirector, 'open', rerouted)
        self.patch.start()

    def close(self):
        self.patch.stop()
        self.server.shutdown()
        self.server.server_close()


@unittest.skipUnless(PROBE, 'Swift toolchain not installed')
class AccountParityTests(unittest.TestCase):
    def setUp(self):
        self.answers = {}
        def handler(path, headers):
            for prefix, answer in self.answers.items():
                if path.startswith(prefix):
                    return answer(path, headers) if callable(answer) else answer
            return 404, b'{}', {}
        self.services = FakeServices(handler)
        self.addCleanup(self.services.close)

    def both(self, source, key=None, options=None, environment=None):
        with unittest.mock.patch.dict(os.environ, environment or {}):
            expected = accounts.account(source, key, options)
        got = swift('accounts.account', source, key, options or {}, environment or {}, self.services.routes)
        same(self, expected, got)
        return expected

    def ok(self, value):
        return 200, json.dumps(value).encode(), {'Content-Type': 'application/json'}

    def test_balances_plans_and_spend(self):
        cases = {
            'deepseek': ('/api.deepseek.com/user/balance', [
                {'balance_infos': [{'currency': 'USD', 'total_balance': '3.20'}, {'currency': 'CNY', 'total_balance': '123456789.123456789'}]},
                {'balance_infos': [{'currency': 'USD', 'total_balance': -0.5}]}, {'balance_infos': []}, {'balance_infos': [{'currency': 'CNY'}]},
                {'balance_infos': [{'total_balance': 'NaN'}]}, {'balance_infos': [{'total_balance': True}]}]),
            'grok': ('/management-api.x.ai/v1/billing/teams/team%201/prepaid/balance', [
                {'total': {'val': -1000}}, {'total': {'val': '-1234.5'}}, {'total': {'val': 0.1}}, {'total': {}}, {'total': {'val': 'x'}}]),
            'kimi-api': ('/api.moonshot.cn/v1/users/me/balance', [
                {'code': 0, 'data': {'available_balance': 12.5, 'voucher_balance': '2.5', 'cash_balance': -3}}, {'code': 1, 'data': {}},
                {'status': False, 'data': {'available_balance': 1}}, {'data': {'voucher_balance': 1}}, {'code': False, 'data': {'available_balance': '1e2'}},
                {'data': {'available_balance': None}}]),
            'minimax': ('/www.minimax.io/v1/token_plan/remains', [
                {'base_resp': {'status_code': 0}, 'model_remains': [
                    {'model_name': 'MiniMax-M3', 'current_interval_remaining_percent': 72.5, 'start_time': 1794996000000, 'end_time': 1795014000000,
                     'current_weekly_status': 3},
                    {'model_name': 'speech', 'current_interval_total_count': 100, 'current_interval_usage_count': 40, 'end_time': 1795014000,
                     'current_weekly_total_count': 10, 'current_weekly_usage_count': 11},
                    {'current_interval_total_count': 0, 'current_interval_status': 3, 'current_weekly_total_count': 0, 'current_weekly_status': 3},
                    'bad', {'model_name': 'x', 'current_interval_remaining_percent': True}]},
                {'base_resp': {'status_code': 1004}}, {'base_resp': {'status_code': 0}, 'model_remains': []}]),
        }
        environment = {'XAI_TEAM_ID': 'team 1', 'MOONSHOT_API_KEY': 'k', 'MINIMAX_API_KEY': 'k', 'XAI_MANAGEMENT_API_KEY': 'k'}
        for source, (path, bodies) in cases.items():
            for body in bodies:
                with self.subTest(source=source, body=body):
                    self.answers = {path: self.ok(body)}
                    self.both(source, 'sk-test' if source == 'deepseek' else None, environment=environment)
            for status in (401, 403, 429, 500, 302):
                with self.subTest(source=source, status=status):
                    self.answers = {path: (status, b'{"error": {"message": "no"}}', {'Location': 'https://example.com/'} if status == 302 else {})}
                    self.both(source, 'sk-test' if source == 'deepseek' else None, environment=environment)
            with self.subTest(source=source, body='not json'):
                self.answers = {path: (200, b'<html>', {})}
                self.both(source, 'sk-test' if source == 'deepseek' else None, environment=environment)
        self.both('grok', environment={'XAI_MANAGEMENT_API_KEY': 'k'})
        self.both('kimi-api', options={'region': 'mars'}, environment={'MOONSHOT_API_KEY': 'k'})
        self.both('minimax', environment={'MINIMAX_API_KEY': 'k', 'MINIMAX_REGION': 'moon'})
        self.both('deepseek', '')
        self.both('claude-api', environment={'ANTHROPIC_API_KEY': ''})

    def test_qoder_packages_page_through(self):
        def pages(path, headers):
            if 'nextToken=p2' in path:return self.ok({'resourcePackages': [{'remainingValue': '0.3'}, {'remainingValue': 7, 'status': 'expired'}]})
            return self.ok({'resourcePackages': [{'remainingValue': '100.1', 'status': 'ACTIVE'}, 'x', {'remainingValue': 0.2}], 'nextToken': 'p2'})
        self.answers = {'/api.qoder.com.cn/v1/organizations/org%2F1/resource-packages': pages}
        self.both('qoder-api', 'qk', {'org': 'org/1'})
        self.answers = {'/api.qoder.com.cn/': self.ok({'resourcePackages': [{'remainingValue': -1}]})}
        self.both('qoder-api', 'qk', {'org': 'o'})
        self.answers = {'/api.qoder.com.cn/': self.ok({'nothing': []})}
        self.both('qoder-api', 'qk', {'org': 'o'})

    def test_claude_spend(self):
        today = datetime.now(timezone.utc).strftime('%Y-%m-%d')
        def report(path, headers):
            if 'page=two' in path:return self.ok({'data': [{'starting_at': today + 'T00:00:00Z', 'results': [{'amount': '250.5', 'currency': 'USD'}]}], 'has_more': False})
            return self.ok({'data': [{'starting_at': '2026-11-01T00:00:00Z', 'results': [{'amount': '1000'}, {'amount': 33.3}]}, 'odd'],
                            'has_more': True, 'next_page': 'two'})
        self.answers = {'/api.anthropic.com/v1/organizations/cost_report': report}
        self.both('claude-api', 'sk-ant-admin-x')
        for status, body in ((403, b'{}'), (401, b'{"error": {"message": "Admin API key required"}}'), (401, b'{}'), (500, b'')):
            with self.subTest(status=status, body=body):
                self.answers = {'/api.anthropic.com/': (status, body, {})}
                self.both('claude-api', 'sk-ant-api')
        self.answers = {'/api.anthropic.com/': self.ok({'data': [{'results': [{'amount': '1', 'currency': 'EUR'}]}]})}
        self.both('claude-api', 'k')

    def test_key_checks(self):
        for source, key in (('chatgpt-api', 'sk'), ('claude-api', 'sk-ant-api'), ('claude-api', 'sk-ant-admin-1'), ('gemini-api', 'g')):
            for status, body in ((200, b'{}'), (400, b'{"error": {"message": "This key is not scoped to a workspace"}}'), (400, b'{}'),
                                 (401, b''), (403, b''), (429, b''), (503, b''), (302, b'')):
                with self.subTest(source=source, key=key, status=status):
                    self.answers = {'/': (status, body, {'Location': 'https://example.com/'} if status == 302 else {})}
                    expected = accounts.check_key(source, key)
                    self.assertEqual(swift('accounts.check', source, key, self.services.routes), expected)


@unittest.skipUnless(PROBE, 'Swift toolchain not installed')
class OpenCodexParityTests(unittest.TestCase):
    """OpenCodex's management API, Komari and DeepSeek's balance, from a local stand-in, read by both engines."""

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        root = Path(self.temp.name)
        self.answers = {}
        self.seen = []
        def handler(path, headers):
            self.seen.append((path, {k.lower(): v for k, v in headers.items()}.get('x-opencodex-api-key')))
            for prefix, answer in self.answers.items():
                if path.startswith(prefix):return answer(path, headers) if callable(answer) else answer
            return 404, b'{"error": "missing"}', {}
        self.services = FakeServices(handler)
        self.addCleanup(self.services.close)
        home = root / 'home'; (home / '.opencodex').mkdir(parents=True)
        (home / '.opencodex' / 'admin-api-token').write_text('secret-token\n')
        self.states = {e: {'OCX_STATE_DIR': str(root / e / 'state'), 'OCX_CACHE_DIR': str(root / e / 'cache')} for e in ('python', 'swift')}
        self.common = {'HOME': str(home), 'OCX_HOME': str(home / '.opencodex'), 'OCX_BASE_URL': self.services.base, 'OCX_TIMEZONE': 'Asia/Shanghai',
                       'KOMARI_BASE_URL': self.services.base + '/komari', 'OCX_PLUGIN_PATH': str(ROOT / 'starmeter.30s.sh'), 'STARMETER_PRICING': str(ROOT / 'lib')}

    def ok(self, value, status=200):
        return status, json.dumps(value).encode(), {'Content-Type': 'application/json'}

    def python(self, call, *args, extra=None):
        import types
        environment = dict(self.common, **self.states['python'], **(extra or {}))
        with unittest.mock.patch.dict(os.environ, environment):
            for name in ('DEEPSEEK_API_KEY', 'DEEPSEEK_API_KEY_FILE', 'TZ'):
                if name not in environment:os.environ.pop(name, None)
            plugin = types.ModuleType('plugin_ocx')
            exec(compile((ROOT / 'lib/swiftbar_plugin.py').read_text(), 'plugin', 'exec'), plugin.__dict__)
            if call == 'provider_quotas':return list(plugin.provider_quotas_cached())
            if call == 'komari':return list(plugin.komari_snapshot())
            if call == 'opencodex_range':
                # As range_usage_cached's collect() does: a failed read is an error, not an exception.
                try:return plugin.ocx_range_usage(args[0], datetime.fromtimestamp(args[1], plugin.local_zone()))
                except Exception:return {'error': '记录暂不可用'}
            if call == 'opencodex_refresh':
                plugin.opencodex_refresh()
                return {name: plugin.read_cache(name) for name in plugin.OPENCODEX_ENDPOINTS}

    def swift(self, call, *args, extra=None):
        environment = {k: v for k, v in os.environ.items() if k not in ('DEEPSEEK_API_KEY', 'DEEPSEEK_API_KEY_FILE', 'TZ', 'OCX_BASE_URL')}
        environment.update(self.common, **self.states['swift'], **(extra or {}))
        done = subprocess.run([PROBE], input=json.dumps({'call': 'runtime', 'args': [call, self.services.routes, *args]}).encode(),
                              capture_output=True, check=True, env=environment)
        answer = json.loads(done.stdout)
        if 'error' in answer:raise SwiftError(answer['error'])
        result = answer['result']
        return [result['payload'], result['error']] if call in ('provider_quotas', 'komari') else result

    @staticmethod
    def steady(value):
        if isinstance(value, dict):
            return {k: OpenCodexParityTests.steady(v) for k, v in value.items() if k not in ('generatedAt', 'updatedAt', '_fetchedAt', 'fetchedAt', 'retryAt', '_stale')}
        if isinstance(value, list):return [OpenCodexParityTests.steady(v) for v in value]
        return value

    def compare(self, call, *args, extra=None):
        for engine in self.states.values():shutil.rmtree(engine['OCX_CACHE_DIR'], ignore_errors=True)
        expected = self.python(call, *args, extra=extra)
        if isinstance(expected, list) and expected[1]:expected[1] = expected[1][1]
        got = self.swift(call, *args, extra=extra)
        same(self, self.steady(expected), self.steady(got))
        return expected

    def test_provider_quotas(self):
        balance = lambda total: self.ok({'balance_infos': [{'currency': 'USD', 'total_balance': '0'}, {'currency': 'CNY', 'total_balance': total}]})
        self.answers = {'/api/provider-quotas': self.ok({'reports': [{'provider': 'DeepSeek', 'label': 'DeepSeek', 'source': 'proxy', 'secret': 'x',
            'quota': {'customWindows': [{'label': 'API balance (¥0.00)'}]}}, {'provider': 'grok', 'quota': {}}, 'odd']}),
            '/api.deepseek.com/user/balance': balance('-12.5')}
        self.compare('provider_quotas')
        self.answers['/api/provider-quotas'] = self.ok({'reports': [{'provider': 'deepseek', 'quota': {'customWindows': [{'label': 'API balance (€ 1,234.50)'}]}}]})
        self.compare('provider_quotas')
        self.answers['/api/provider-quotas'] = (503, b'{"error": {"message": "warming up"}}', {})
        self.compare('provider_quotas')
        self.answers['/api.deepseek.com/user/balance'] = (500, b'', {})
        self.compare('provider_quotas')
        self.answers['/api.deepseek.com/user/balance'] = balance('88.8')
        self.compare('provider_quotas', extra={'DEEPSEEK_API_KEY': 'sk-direct'})
        self.answers['/api.deepseek.com/user/balance'] = self.ok({'balance_infos': [{'currency': 'USD', 'granted_balance': '2', 'topped_up_balance': '5'}]})
        self.compare('provider_quotas', extra={'DEEPSEEK_API_KEY': 'sk-direct'})

    def test_komari(self):
        def rpc(path, headers):
            assert json.loads(headers['X-Body'])['method'] == 'common:getNodesLatestStatus'
            return self.ok({'result': {'n1': {'online': True, 'cpu': {'usage': 3}, 'ram': 1, 'time': 'now'}, 'n2': {'online': 'yes'}}})
        self.answers = {'/komari/api/nodes': self.ok({'data': [{'uuid': 'n1', 'name': 'Tokyo', 'weight': 1}, {'uuid': 'n2', 'name': 'HK'},
                                                              {'uuid': 'n3', 'hidden': True}, {'name': 'no id'}, 'odd']}),
                        '/komari/api/rpc2': rpc}
        self.compare('komari')
        self.answers['/komari/api/rpc2'] = (500, b'', {})
        self.compare('komari')
        self.answers['/komari/api/nodes'] = self.ok({'status': 'error'})
        self.compare('komari')
        self.compare('komari', extra={'KOMARI_BASE_URL': 'https://your-komari.example'})

    def test_opencodex(self):
        frozen = 1794996000 + 9 * 3600 + 1234.5  # 2026-11-18 03:20:34 in Shanghai
        def usage(path, headers):
            since = int(path.split('since=')[1].split('&')[0])
            return self.ok({'summary': {'totalTokens': since % 1000 + 1, 'requests': 2, 'estimatedCostUsd': 0.5}, 'models': [], 'days': []})
        def timeline(path, headers):
            hours = int(path.split('hours=')[1].split('&')[0])
            begin = frozen - hours * 3600
            return self.ok({'start': begin, 'bucketSeconds': 3600 if 'bucketMinutes=60' in path else 60,
                            'series': [{'points': [i % 3 for i in range(hours if 'bucketMinutes=60' in path else 60)]}], 'missingMeasurements': hours == 24})
        self.answers = {'/api/usage/timeline': timeline, '/api/usage': usage, '/healthz': self.ok({'ok': True}),
                        '/readyz': self.ok({'ready': False, 'state': 'starting'}, 503), '/api/codex-auth/quota': self.ok({'windows': []}),
                        '/api/codex-auth/accounts': self.ok([1, 2]), '/api/codex-auth/active': (401, b'{"message": "bad token"}', {})}
        for key in ('today', 'yesterday', '24h', '7d', '30d', 'all'):
            with self.subTest(range=key):
                self.compare('opencodex_range', key, frozen)
        self.answers['/api/usage/timeline'] = self.ok({'truncated': True})
        self.compare('opencodex_range', 'today', frozen)
        self.answers['/api/usage'] = (500, b'', {})
        self.compare('opencodex_range', 'today', frozen)
        self.compare('opencodex_refresh')
        self.assertTrue(all(token == 'secret-token' for path, token in self.seen if path.startswith('/api/codex-auth')))
        self.answers['/healthz'] = (503, b'', {})
        self.compare('opencodex_refresh')


@unittest.skipUnless(PROBE, 'Swift toolchain not installed')
class ClaudeSubscriptionParityTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)

    def test_usage_responses(self):
        cases = [
            {'five_hour': {'utilization': 42.5, 'resets_at': '2026-11-18T15:00:00Z'}, 'seven_day': {'utilization': 7, 'resets_at': None},
             'seven_day_opus': {'utilization': 101}, 'seven_day_sonnet': 'x'},
            {'limits': [{'group': 'weekly', 'kind': 'all', 'percent': 61, 'resets_at': '2026-11-20T00:00:00Z'},
                        {'kind': 'five_hour', 'percent': 12.25, 'resets_at': '2026-11-18T15:00:00Z', 'scope': {'model': {'display_name': 'Opus 5.5'}}},
                        {'kind': 'mystery', 'percent': 3, 'scope': {'surface': {'display_name': 'Cowork'}}},
                        {'kind': 'five_hour', 'percent': 12.25, 'resets_at': '2026-11-18T15:00:00Z', 'scope': {'model': {'display_name': 'Opus 5.5'}}},
                        {'kind': 'week', 'percent': True}, {'kind': 'session', 'percent': 5, 'resets_at': 7}, 'odd'],
             'spend': {'enabled': True, 'used': {'amount_minor': 1234, 'exponent': 2, 'currency': 'USD'},
                       'limit': {'amount_minor': 5000, 'exponent': 2}, 'balance': {'amount_minor': 99, 'exponent': 0, 'currency': 'eur'}}},
            {'five_hour': {'utilization': 1}, 'spend': {'enabled': False}},
            {'five_hour': {'utilization': 1}, 'spend': {'enabled': True, 'used': {'amount_minor': -1, 'exponent': 2}},
             'extra_usage': {'is_enabled': True, 'used_credits': 250, 'monthly_limit': 0, 'currency': 'JPY', 'decimal_places': 0}},
            {'five_hour': {'utilization': 1}, 'extra_usage': {'is_enabled': True, 'used_credits': 1234.5, 'monthly_limit': 10000}},
            {'five_hour': {'utilization': 1}, 'extra_usage': {'is_enabled': True, 'used_credits': -1}},
            {'nothing': True}, [1, 2], {'limits': []},
        ]
        for data in cases:
            with self.subTest(data=data):
                try:
                    expected = claude_live.parse_usage(data, now=1794996000.5)
                except claude_live.UsageError as error:
                    with self.assertRaises(SwiftError) as raised:
                        swift('claude.parse', data, 1794996000.5)
                    self.assertIn(str(error), str(raised.exception))
                    continue
                same(self, expected, swift('claude.parse', data, 1794996000.5))

    def test_desktop_and_statusline_snapshots(self):
        now = 1794996000.0
        app = self.root / 'Claude'
        account = '11111111-2222-4333-8444-555555555555'
        org = '66666666-7777-4888-9999-000000000000'
        variants = [
            {'version': 2, 'samples': [{'t': (now - 60) * 1000, 'org': org, 'u': {'fh': 40, 'sd': 12.5, 'so': 'x', 'oa': 100}},
                                       {'t': (now - 3600) * 1000, 'org': org, 'u': {'fh': 90}}]},
            {'version': 1, 'samples': [{'t': (now - 4000) * 1000, 'fh': 5, 'sd': 6, 'cw': 7}]},
            {'version': 2, 'samples': [{'t': now * 1000, 'org': 'a', 'u': {'fh': 1}}, {'t': now * 1000, 'org': 'b', 'u': {'fh': 2}}]},
            {'version': 3, 'samples': []}, {'version': 2, 'samples': [{'t': (now + 3600) * 1000, 'u': {'fh': 1}}]},
            {'version': 2, 'samples': [{'t': now * 1000, 'u': 'none'}]},
        ]
        for index, data in enumerate(variants):
            for owned in (False, True):
                with self.subTest(variant=index, owned=owned):
                    shutil.rmtree(app, ignore_errors=True)
                    app.mkdir()
                    (app / 'plan-usage-history.json').write_text(json.dumps(data))
                    if owned:
                        (app / 'config.json').write_text(json.dumps({'lastKnownAccountUuid': account}))
                        (app / 'claude-code-sessions' / account / org).mkdir(parents=True)
                    same(self, claude_desktop.subscription(app, now), swift('claude.desktop', str(app), now))
        state = self.root / 'state'
        state.mkdir()
        for data in [{'schema': 1, 'source': 'claude-code', 'updatedAt': now - 30, 'windows': [
                          {'label': '5 小时', 'used': 33, 'resetsAt': '2026-11-18T15:00:00+00:00'},
                          {'label': '本周', 'used': 101, 'resetsAt': '2026-11-20T00:00:00+00:00'}, {'label': '其他', 'used': 1, 'resetsAt': 'x'}]},
                     {'schema': 1, 'source': 'claude-code', 'updatedAt': now - 900, 'windows': [{'label': '本周', 'used': 3, 'resetsAt': '2026-11-01T00:00:00+00:00'}]},
                     {'schema': 1, 'source': 'claude-code', 'updatedAt': now + 120, 'windows': []},
                     {'schema': 1, 'source': 'elsewhere', 'updatedAt': now, 'windows': []}, {'schema': 1, 'source': 'claude-code', 'updatedAt': True, 'windows': []}]:
            with self.subTest(snapshot=data):
                (state / 'claude-code-usage.json').write_text(json.dumps(data, ensure_ascii=False))
                same(self, claude_local.snapshot(state, now), swift('claude.local', str(state), now))

    def test_oauth_reads_from_the_login_file(self):
        answers = {}
        def handler(path, headers):
            return answers.get('usage', (404, b'', {}))
        services = FakeServices(handler)
        self.addCleanup(services.close)
        config = self.root / 'claude-config'
        config.mkdir()
        bin_dir = self.root / 'bin'
        bin_dir.mkdir()
        environment = {'CLAUDE_CONFIG_DIR': str(config), 'PATH': str(bin_dir), 'HOME': str(self.root / 'home')}
        def login(**changes):
            oauth = dict({'accessToken': 'tok_' + 'a' * 30, 'expiresAt': (time.time() + 7200) * 1000, 'scopes': ['user:inference', 'user:profile']}, **changes)
            (config / '.credentials.json').write_text(json.dumps({'claudeAiOauth': oauth}))
        good = (200, json.dumps({'five_hour': {'utilization': 20, 'resets_at': '2026-11-18T15:00:00Z'}}).encode(), {'Content-Type': 'application/json'})
        def run(note, force=False, requested=0):
            results = []
            for engine in ('python', 'swift'):
                state = self.root / ('state-' + engine)
                if engine == 'python':
                    with unittest.mock.patch.dict(os.environ, environment):
                        payload, error = claude_oauth.fetch(state, consent=True, force=force, requested_at=requested)
                    results.append({'payload': payload, 'error': error[1] if error else None})
                else:
                    env = dict(os.environ, **environment)
                    done = subprocess.run([PROBE], input=json.dumps({'call': 'claude.oauth', 'args': [str(state), force, requested, services.routes]}).encode(),
                                          capture_output=True, check=True, env=env)
                    results.append(json.loads(done.stdout)['result'])
            files = [json.loads((self.root / ('state-' + e) / 'claude-oauth-usage.json').read_text()) for e in ('python', 'swift')]
            def steady(value):
                if isinstance(value, dict):return {k: steady(v) for k, v in value.items() if k not in ('checkedAt', 'lastRenewAt', 'updatedAt', 'retryAt')}
                return value
            with self.subTest(step=note):
                same(self, steady(results[0]), steady(results[1]))
                same(self, steady(files[0]), steady(files[1]))
                self.assertEqual(bool(files[0].get('lastRenewAt')), bool(files[1].get('lastRenewAt')))
                self.assertEqual(bool(files[0].get('retryAt')), bool(files[1].get('retryAt')))
        login(); answers['usage'] = good
        run('first read')
        answers['usage'] = (500, b'', {})
        run('cached within five minutes')
        run('a forced retry after thirty seconds is still cached', force=True)
        answers['usage'] = (429, b'', {'Retry-After': '120'})
        for state in ('python', 'swift'):(self.root / ('state-' + state) / 'claude-oauth-usage.json').unlink()
        run('rate limited')
        run('still rate limited', force=True)
        for state in ('python', 'swift'):(self.root / ('state-' + state) / 'claude-oauth-usage.json').unlink()
        for name, changes, answer in (('expired, no CLI to renew it', {'expiresAt': (time.time() - 60) * 1000}, good),
                                      ('signed out', {'accessToken': ''}, good), ('odd token', {'accessToken': 'short'}, good),
                                      ('no usage scope', {'scopes': ['user:inference']}, good), ('forbidden', {}, (403, b'', {})),
                                      ('not JSON', {}, (200, b'<html>', {'Content-Type': 'text/html'})), ('server error', {}, (503, b'', {}))):
            login(**changes); answers['usage'] = answer
            for state in ('python', 'swift'):(self.root / ('state-' + state) / 'claude-oauth-usage.json').unlink(missing_ok=True)
            run(name)
        (config / '.credentials.json').write_text('{"mcpOAuth": {}}')
        for state in ('python', 'swift'):(self.root / ('state-' + state) / 'claude-oauth-usage.json').unlink(missing_ok=True)
        run('an API-key login has no subscription')


@unittest.skipUnless(PROBE, 'Swift toolchain not installed')
class ClientAccountParityTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)

    def test_qoder_reads_the_newest_summary(self):
        home = self.root / 'home'
        logs = home / 'Library' / 'Application Support' / 'com.qodercn.app.stable' / 'logs'
        now = 1794996000.0
        def line(stamp, usage, tail=''):
            return '[%s] [info] account.getQuotaUsage %s%s' % (stamp, json.dumps({'qoderUsage': usage}), tail)
        (logs / 's1').mkdir(parents=True)
        (logs / 's1' / 'main.log').write_text('\n'.join([
            line('2026-11-18T08:00:00.000Z', {'userQuota': {'used': 10, 'total': 100}, 'expiresAt': 1795996000000}),
            'noise "qoderUsage" without json', line('2026-11-18 09:30:00', {'userQuota': {'used': 30.5, 'total': 100, 'remaining': 70},
                                                                          'addOnQuota': {'used': 1, 'total': 5}, 'expiresAt': 253402214400, 'isQuotaExceeded': True}),
        ]) + '\n')
        (logs / 's2').mkdir()
        (logs / 's2' / 'main.log').write_text(line('2026-11-17T08:00:00Z', {'userQuota': {'used': 1, 'total': 2}}) + '\r\n' +
                                              line('bad stamp', {'userQuota': {'used': 1, 'total': 2}}) + '\n' + line('2026-11-18T09:00:00Z', {'nothing': 1}) + '\n')
        old = home / 'Library' / 'Application Support' / 'QoderCN' / 'logs' / 'x'
        old.mkdir(parents=True)
        (old / 'main.log').write_text(line('2026-11-01T00:00:00Z', {'userQuota': {'used': 1, 'total': 2}}))
        os.utime(old / 'main.log', (now - 30 * 86400, now - 30 * 86400))
        for zone in ('UTC', 'Asia/Shanghai'):
            with self.subTest(zone=zone), unittest.mock.patch.dict(os.environ, {'TZ': zone}):
                time.tzset()
                expected = qoder.snapshot(home, now)
                done = subprocess.run([PROBE], input=json.dumps({'call': 'qoder.snapshot', 'args': [str(home), now]}).encode(),
                                      capture_output=True, check=True, env=dict(os.environ, TZ=zone))
                same(self, expected, json.loads(done.stdout)['result'])
        time.tzset()

    def fake_codex(self, account, limits):
        script = self.root / 'codex'
        script.write_text('#!%s\n' % sys.executable + r"""
import json, sys
answers = %s
for line in sys.stdin:
    message = json.loads(line)
    if 'id' not in message: continue
    result = answers.get(message['method'])
    if result == 'hang': continue
    print(json.dumps({'id': message['id'], 'result': result}), flush=True)
""" % repr({'initialize': {'ok': True}, 'account/read': account, 'account/rateLimits/read': limits}))
        script.chmod(0o755)
        return str(script)

    def test_chatgpt_reads_the_app_server(self):
        limits = {'rateLimitsByLimitId': {'codex': {'limitName': 'Codex', 'primary': {'usedPercent': 37.5, 'windowDurationMins': 300, 'resetsAt': 1795000000},
                                                    'secondary': {'usedPercent': 120, 'windowDurationMins': 10080}},
                                          'sol': {'primary': {'usedPercent': -5}, 'secondary': {'usedPercent': '9'}}, 'odd': 3}}
        cases = [({'account': {'type': 'chatgpt', 'planType': 'pro'}}, limits),
                 ({'account': {'type': 'chatgpt'}}, {'rateLimits': {'primary': {'usedPercent': 0, 'resetsAt': None}}}),
                 ({'account': {'type': 'apiKey'}}, limits), ({'account': None}, limits), ('nope', limits),
                 ({'account': {'type': 'chatgptAuthTokens'}}, 'nope'), ({'account': {'type': 'chatgpt'}}, {'rateLimitsByLimitId': {}})]
        for account, result in cases:
            with self.subTest(account=account, limits=result):
                binary = self.fake_codex(account, result)
                expected = sources.chatgpt_account(binary, self.root / 'home')
                same(self, expected, swift('chatgpt.account', binary, str(self.root / 'home')))
        self.assertEqual(swift('chatgpt.account', str(self.root / 'missing'), None), {'error': '无法启动 ChatGPT 编程客户端'})


@unittest.skipUnless(PROBE, 'Swift toolchain not installed')
class CursorParityTests(unittest.TestCase):
    now = 1794996000.0

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.home = Path(self.temp.name) / 'home'
        self.events = []
        self.summary = {}
        self.requests = []
        def handler(path, headers):
            if path.startswith('/cursor.com/api/usage-summary'):return 200, json.dumps(self.summary).encode(), {'Content-Type': 'application/json'}
            if path.startswith('/cursor.com/api/dashboard/get-filtered-usage-events'):
                body = json.loads(headers.get('X-Body') or '{}')
                self.requests.append((body.get('page'), body.get('startDate'), body.get('endDate'), headers.get('Cookie')))
                start, end = int(body['startDate']), int(body['endDate'])
                def stamp(event):
                    try:return int(str(event.get('timestamp')))
                    except ValueError:return start  # an odd one is still sent, as cursor.com might
                rows = [e for e in self.events if start <= stamp(e) <= end]
                page, size = body['page'], body['pageSize']
                chunk = rows[(page - 1) * size: page * size]
                if chunk and page > 1:chunk = [rows[(page - 1) * size - 1]] + chunk  # a repeated row at the seam
                return 200, json.dumps({'usageEventsDisplay': chunk, 'totalUsageEventsCount': len(rows)}).encode(), {}
            return 404, b'', {}
        self.services = FakeServices(handler)
        self.addCleanup(self.services.close)

    def login(self, token, where='app'):
        if where == 'app':
            root = self.home / 'Library' / 'Application Support' / 'Cursor' / 'User' / 'globalStorage'
            root.mkdir(parents=True, exist_ok=True)
            conn = sqlite3.connect(root / 'state.vscdb')
            conn.execute('CREATE TABLE IF NOT EXISTS ItemTable(key TEXT PRIMARY KEY, value BLOB)')
            conn.execute("INSERT OR REPLACE INTO ItemTable VALUES ('cursorAuth/accessToken', ?)", (token,))
            conn.commit(); conn.close()
        else:
            (self.home / '.config' / 'cursor').mkdir(parents=True, exist_ok=True)
            (self.home / '.config' / 'cursor' / 'auth.json').write_text(json.dumps({'accessToken': token}))

    @staticmethod
    def jwt(**claims):
        import base64
        encode = lambda value: base64.urlsafe_b64encode(json.dumps(value).encode()).decode().rstrip('=')
        return encode({'alg': 'none'}) + '.' + encode(claims) + '.sig'

    def both(self, call, *args):
        with unittest.mock.patch.dict(os.environ, {'HOME': str(self.home)}):
            os.environ.pop('XDG_CONFIG_HOME', None)
            try:
                expected = cursor.refresh(*args, now=self.now) if call == 'refresh' else cursor.plan(self.home, now=self.now)
            except cursor.CursorError as error:
                expected = {'failure': str(error)}
        env = {k: v for k, v in os.environ.items() if k != 'XDG_CONFIG_HOME'}
        env['HOME'] = str(self.home)
        request = [args[0], str(self.home), args[2], args[3], self.now, self.services.routes] if call == 'refresh' else [str(self.home), self.now, self.services.routes]
        done = subprocess.run([PROBE], input=json.dumps({'call': 'cursor.' + call, 'args': request}).encode(), capture_output=True, check=True, env=env)
        got = json.loads(done.stdout)
        self.assertNotIn('error', got)
        same(self, expected, got['result'])
        return expected

    def test_logins(self):
        self.both('refresh', None, self.home, 0, self.now)
        self.login('cursor-safe-storage:abc')
        self.both('refresh', None, self.home, 0, self.now)
        self.login(self.jwt(sub='auth0|user_1', exp=self.now - 5))
        self.both('refresh', None, self.home, 0, self.now)
        self.login(self.jwt(exp=self.now + 3600))
        self.both('refresh', None, self.home, 0, self.now)
        self.login(self.jwt(sub='google|user_2', exp=self.now + 3600), where='cli')
        self.assertIn('account', self.both('refresh', None, self.home, self.now - 60, self.now))

    def test_events_are_kept_and_topped_up(self):
        self.login(self.jwt(sub='auth0|user_1', exp=self.now + 7200))
        base = int((self.now - 86400 * 3) * 1000)
        for i in range(2300):
            self.events.append({'usageEventId': 'e%d' % i, 'timestamp': str(base + i * 100000), 'model': ['claude-opus-5-5', 'gpt-6.2-sol', None][i % 3],
                                'conversationId': 'c%d' % (i % 7) if i % 5 else None,
                                'tokenUsage': {'inputTokens': i, 'outputTokens': 2, 'cacheReadTokens': 30, 'cacheWriteTokens': i % 2, 'totalCents': [12.5, -1, True][i % 3]}})
        self.events.append({'usageEventId': 'odd', 'timestamp': 'later'})
        start = self.now - 86400 * 2
        store = self.both('refresh', None, self.home, start, self.now - 3600)
        self.assertEqual(len(store['events']), len({e['usageEventId'] for e in self.events if start * 1000 <= int(e['timestamp'] if e['timestamp'].isdigit() else 0) <= (self.now - 3600) * 1000}))
        wider = self.both('refresh', store, self.home, self.now - 86400 * 3, self.now)
        tallied = cursor.tally((wider['events'], wider['complete'], wider['login']), self.now - 86400, self.now, ZoneInfo('Asia/Shanghai'))
        self.assertGreater(tallied['summary']['requests'], 0)
        self.both('refresh', dict(wider, account='someone-else'), self.home, self.now - 3600, self.now)

    def test_plan(self):
        self.login(self.jwt(sub='auth0|user_1', exp=self.now + 7200))
        for summary in ({'membershipType': 'pro', 'billingCycleStart': '2026-11-01T00:00:00.000Z', 'billingCycleEnd': '2026-12-01T00:00:00Z',
                         'individualUsage': {'plan': {'used': 1234, 'limit': 2000, 'totalPercentUsed': 99}, 'onDemand': {'enabled': True, 'used': 50, 'limit': 0}}},
                        {'isUnlimited': True, 'individualUsage': {'plan': {'totalPercentUsed': 12.5}, 'overall': {'used': 7, 'limit': 0}}},
                        {'individualUsage': {'overall': {'used': '3', 'limit': '4'}}}, {'individualUsage': 'none', 'billingCycleEnd': 'soon'}):
            with self.subTest(summary=summary):
                self.summary = summary
                self.both('plan')


@unittest.skipUnless(PROBE, 'Swift toolchain not installed')
class SessionParityTests(unittest.TestCase):
    def test_active_sessions_and_their_turns(self):
        temp = tempfile.TemporaryDirectory(); self.addCleanup(temp.cleanup)
        root = Path(temp.name)
        claude, codex = root / 'projects', root / 'codex'
        now = time.time()
        def write(path, rows, age=0):
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(''.join((r if isinstance(r, str) else json.dumps(r, ensure_ascii=False)) + '\n' for r in rows))
            os.utime(path, (now - age, now - age))
        usage = lambda n: {'input_tokens': n, 'cache_read_input_tokens': 10, 'cache_creation_input_tokens': 5, 'output_tokens': 3}
        talk = [
            {'type': 'ai-title', 'aiTitle': 'Made up'}, {'type': 'user', 'timestamp': '2026-11-18T10:00:00Z', 'message': {'content': 'hi'}},
            {'type': 'assistant', 'cwd': '/w/app', 'message': {'id': 'm1', 'model': 'claude-opus-5-5', 'usage': usage(1000),
             'content': [{'type': 'tool_use', 'name': 'Bash'}, {'type': 'text'}]}},
            {'type': 'assistant', 'message': {'id': 'm1', 'model': 'claude-opus-5-5', 'usage': usage(1000), 'content': [{'type': 'tool_use', 'name': 'Read'}]}},
            {'type': 'assistant', 'isSidechain': True, 'message': {'id': 's1', 'model': 'claude-haiku-5-5', 'usage': usage(50), 'stop_reason': 'end_turn'}},
            {'type': 'user', 'message': {'content': [{'type': 'tool_result'}]}}, {'type': 'user', 'isMeta': True, 'message': {'content': 'meta'}},
            {'type': 'user', 'timestamp': '2026-11-18T10:05:00.123456Z', 'message': {'content': [{'type': 'text', 'text': 'more'}]}},
            {'type': 'assistant', 'message': {'id': 'm2', 'model': 'mystery', 'usage': usage(300000), 'stop_reason': 'end_turn'}},
            {'type': 'custom-title', 'customTitle': ' 维护 '}, 'not json']
        write(claude / 'p' / '7a5d1c2e-0000-4000-8000-000000000001.jsonl', talk, age=5)
        write(claude / 'p' / 'busy.jsonl', [{'type': 'user', 'message': {'content': 'go'}}, {'type': 'assistant', 'message': {'model': 'claude-fable-5-2', 'usage': usage(10)}}], age=1)
        write(claude / 'p' / 'stalled.jsonl', [{'type': 'assistant', 'message': {'stop_reason': 'tool_use'}}], age=live_sessions.STALLED + 5)
        write(claude / 'p' / 'old.jsonl', [{'type': 'assistant', 'message': {'stop_reason': 'end_turn'}}], age=live_sessions.ACTIVE + 5)
        write(claude / 'p' / 'subagents' / 'x.jsonl', [{'type': 'assistant'}], age=1)
        thread = '019a0000-1111-7222-8333-444455556666'
        write(codex / 'sessions' / '2026' / '11' / '03' / ('rollout-2026-11-03T10-00-00-%s.jsonl' % thread), [
            {'type': 'session_meta', 'payload': {'id': thread, 'cwd': '/w/ops'}},
            {'type': 'turn_context', 'payload': {'model': 'gpt-6.2-sol', 'effort': 'ultra'}},
            {'type': 'event_msg', 'timestamp': '2026-11-18T10:00:00Z', 'payload': {'type': 'task_started'}},
            {'type': 'response_item', 'payload': {'type': 'function_call', 'name': 'exec'}}, {'type': 'response_item', 'payload': {'type': 'local_shell_call'}},
            {'type': 'event_msg', 'payload': {'type': 'token_count', 'info': {'model_context_window': 1000, 'total_token_usage': {'t': 1},
             'last_token_usage': {'input_tokens': 300, 'output_tokens': 50, 'cached_input_tokens': 100}}}},
            {'type': 'event_msg', 'payload': {'type': 'token_count', 'info': {'total_token_usage': {'t': 1}, 'last_token_usage': {'input_tokens': 300}}}},
            {'type': 'turn_context', 'payload': {'model': 'codex-auto-review'}},
            {'type': 'event_msg', 'payload': {'type': 'token_count', 'info': {'model_context_window': 100, 'total_token_usage': {'t': 2},
             'last_token_usage': {'input_tokens': 300, 'output_tokens': 50}}}}], age=2)
        write(codex / 'sessions' / '2026' / '11' / '18' / 'rollout-guardian.jsonl', [
            {'type': 'session_meta', 'payload': {'source': {'subagent': {'other': 'guardian'}}}}, {'type': 'event_msg', 'payload': {'type': 'task_started'}}], age=1)
        (codex / 'session_index.jsonl').write_text(json.dumps({'id': thread, 'thread_name': '服务器管理'}) + '\n')
        app = {'7a5d1c2e-0000-4000-8000-000000000001': {'title': 'App 名字', 'effort': 'xhigh'}, 'busy': {'title': 'Busy', 'effort': None}}
        plugin = __import__('types').ModuleType('plugin_sessions')
        with unittest.mock.patch.dict(os.environ, {'OCX_PLUGIN_PATH': str(ROOT / 'starmeter.30s.sh')}):
            exec(compile((ROOT / 'lib/swiftbar_plugin.py').read_text(), 'plugin', 'exec'), plugin.__dict__)
        for prices in (False, True):
            with self.subTest(prices=prices):
                expected = live_sessions.active([claude], codex, now, plugin.session_prices() if prices else None, app)
                got = swift('sessions.active', [str(claude)], str(codex), now, prices, app)
                same(self, expected, got)
                self.assertEqual(len(got), 3)
        # The turns as listed: one with no request left out, MCP tools by their own name.
        turns = [{'at': None, 'requests': 4, 'tokens': 1200, 'cost': 0.4, 'unpriced': 1,
                  'tools': {'mcp__ccd_turn__update_plan': 2, 'mcp__other__update_plan': 1, 'Bash': 3, 'mcp__a___b': 1, 'mcp__x__': 1}},
                 {'at': now, 'requests': 0, 'tokens': 0, 'cost': 0, 'unpriced': 0, 'tools': {}}]
        unit = {'symbol': '¥', 'rate': 7.1}
        for case in (turns, turns[1:], []):
            with self.subTest(turns=len(case)):
                self.assertEqual(swift('sessions.turnLines', case, unit, 'ops'), plugin.turn_lines(case, unit, 'ops'))


@unittest.skipUnless(PROBE, 'Swift toolchain not installed')
class BackgroundParityTests(OpenCodexParityTests):
    """Which reads the background worker runs, and the small ones it adds: status pages and the yuan rate."""

    def python(self, call, *args, extra=None):
        if call not in ('loaders', 'ready', 'status', 'fx'):return super().python(call, *args, extra=extra)
        import types
        environment = dict(self.common, **self.states['python'], **(extra or {}))
        with unittest.mock.patch.dict(os.environ, environment):
            plugin = types.ModuleType('plugin_bg')
            exec(compile((ROOT / 'lib/swiftbar_plugin.py').read_text(), 'plugin', 'exec'), plugin.__dict__)
            if call == 'loaders':return list(plugin.external_loaders())
            if call == 'ready':return {m[0]: bool(plugin.default_ready(m[0])) for m in preferences.MODULES}
            if call == 'status':return service_status.read_all(args[0], 'test')
            payload, error = plugin.exchange_rate_load()
            return {'payload': payload, 'error': error[1] if error else None}

    def swift(self, call, *args, extra=None):
        if call not in ('loaders', 'ready', 'status', 'fx'):return super().swift(call, *args, extra=extra)
        environment = {k: v for k, v in os.environ.items() if k not in ('DEEPSEEK_API_KEY', 'DEEPSEEK_API_KEY_FILE', 'TZ', 'OCX_BASE_URL')}
        environment.update(self.common, **self.states['swift'], **(extra or {}))
        done = subprocess.run([PROBE], input=json.dumps({'call': 'runtime', 'args': [call, self.services.routes, *args]}).encode(),
                              capture_output=True, check=True, env=environment)
        return json.loads(done.stdout)['result']

    def settings(self, **values):
        for engine in self.states.values():
            Path(engine['OCX_STATE_DIR']).mkdir(parents=True, exist_ok=True)
            (Path(engine['OCX_STATE_DIR']) / 'preferences.json').write_text(json.dumps(dict({'schema': 1, 'modules': [m[0] for m in preferences.MODULES]}, **values)))

    def test_loaders_and_readiness(self):
        everything = [m[0] for m in preferences.MODULES if m[0] != 'updates']
        for values, extra in ((dict(visible=everything), {'OCX_SELF_UPDATES': '0'}),
                              (dict(visible=everything, currency='CNY', serviceStatus=True, cursorUsage=True), {'OCX_SELF_UPDATES': '0', 'MOONSHOT_API_KEY': 'k', 'XAI_TEAM_ID': 't', 'XAI_MANAGEMENT_API_KEY': 'k'}),
                              (dict(visible=['claude', 'sessions', 'kimi-api'], currency='CNY', cnyRate=7), {'OCX_SELF_UPDATES': '0', 'KOMARI_BASE_URL': ''}),
                              (dict(visible=['chatgpt', 'opencodex', 'deepseek']), {'OCX_SELF_UPDATES': '0', 'DEEPSEEK_API_KEY': 'sk'})):
            with self.subTest(settings=values, environment=extra):
                self.settings(**values)
                self.assertEqual(self.python('loaders', extra=extra), self.swift('loaders', extra=extra))
                self.assertEqual(self.python('ready', extra=extra), self.swift('ready', extra=extra))

    def test_status_pages_and_rate(self):
        self.answers = {'/status.claude.com/': self.ok({'status': {'indicator': 'minor', 'description': 'Elevated errors' * 30}}),
                        '/status.openai.com/': self.ok({'status': {'indicator': 'odd'}}), '/status.cursor.com/': (500, b'', {}),
                        '/api.frankfurter.dev/': self.ok({'rates': {'CNY': '7.1234'}, 'date': '2026-11-18T00:00'})}
        self.assertEqual(self.python('status', ['claude', 'openai', 'cursor', 'nowhere']), self.swift('status', ['claude', 'openai', 'cursor', 'nowhere']))
        for answer in (self.ok({'rates': {'CNY': '7.1234'}, 'date': '2026-11-18T00:00'}), self.ok({'rates': {'CNY': 30}, 'date': 'x'}), (503, b'', {})):
            self.answers['/api.frankfurter.dev/'] = answer
            self.assertEqual(self.python('fx'), self.swift('fx'))


@unittest.skipUnless(PROBE, 'Swift toolchain not installed')
class PanelParityTests(BackgroundParityTests):
    """The panel's snapshot (homepage rows, module sidebars, settings) and the usage alerts, from the same caches."""

    CALLS = ('snapshot', 'alert_inputs', 'alert_notices', 'alerts_due')

    def python(self, call, *args, extra=None):
        if call not in self.CALLS:return super().python(call, *args, extra=extra)
        import types
        environment = dict(self.common, **self.states['python'], **(extra or {}))
        with unittest.mock.patch.dict(os.environ, environment):
            for name in ('DEEPSEEK_API_KEY', 'DEEPSEEK_API_KEY_FILE', 'TZ'):
                if name not in environment:os.environ.pop(name, None)
            plugin = types.ModuleType('plugin_panel')
            exec(compile((ROOT / 'lib/swiftbar_plugin.py').read_text(), 'plugin', 'exec'), plugin.__dict__)
            if call == 'snapshot':return json.loads(json.dumps(plugin.panel_snapshot(plugin.cached_data())))
            if call == 'alert_notices':return [list(plugin.alert_notice(a)) for a in args[0]]
            if call == 'alerts_due':
                windows, balances, marks, percent, reminder, now = args
                due = plugin.alerts.due(windows, balances, marks, percent, reminder, now)
                return json.loads(json.dumps({'due': due, 'marks': marks}))
            data = plugin.cached_data()
            return {'windows': plugin.alert_windows(data), 'balances': plugin.alert_balances(data), 'menuBar': plugin.menu_bar_value(data)}

    def swift(self, call, *args, extra=None):
        if call not in self.CALLS:return super().swift(call, *args, extra=extra)
        environment = {k: v for k, v in os.environ.items() if k not in ('DEEPSEEK_API_KEY', 'DEEPSEEK_API_KEY_FILE', 'TZ', 'OCX_BASE_URL')}
        environment.update(self.common, **self.states['swift'], **(extra or {}))
        done = subprocess.run([PROBE], input=json.dumps({'call': 'runtime', 'args': [call, self.services.routes, *args]}).encode(),
                              capture_output=True, check=True, env=environment)
        answer = json.loads(done.stdout)
        if 'error' in answer:raise SwiftError(answer['error'])
        return answer['result']

    def caches(self, now):
        def iso(at):return datetime.fromtimestamp(at, timezone.utc).isoformat()
        records = {
            'claude_subscription': {'windows': [{'label': '5 小时', 'used': 42.5, 'resetsAt': iso(now + 3600)},
                                                {'label': '本周', 'used': 91, 'resetsAt': iso(now + 5 * 3600), 'span': 604800},
                                                {'label': 'Opus 本周', 'used': 12, 'resetsAt': iso(now + 4 * 86400 + 60)}],
                                    'source': 'oauth', 'updatedAt': now - 20,
                                    'extraUsage': {'enabled': True, 'used': 12.5, 'limit': 50, 'currency': 'USD', 'balance': 7}},
            'qoder_subscription': {'plan': {'used': 300, 'total': 1000, 'remaining': 700}, 'addOn': {'used': 50.5, 'total': 100, 'remaining': 49.5},
                                   'observedAt': now - 60, 'resetsAt': now + 86400 * 10},
            'chatgpt': {'windows': [{'name': 'codex', 'minutes': 300, 'remaining': 30, 'resetsAt': now + 1000},
                                    {'name': 'Other', 'minutes': 10080, 'remaining': 80.5, 'resetsAt': now + 86400 * 3}], 'plan': 'pro'},
            'minimax': {'windows': [{'name': '本周额度', 'remaining': 5, 'resetsAt': int(now) + 3600, 'startsAt': int(now) - 6 * 86400}]},
            'kimi-api': {'balance': 12.5, 'currency': 'CNY', 'state': '已同步'},
            'grok': {'balance': 3.25, 'currency': 'USD', 'state': '已同步'},
        }
        for engine in self.states.values():
            scope = hashlib.sha256(os.path.abspath(engine['OCX_STATE_DIR']).encode()).hexdigest()[:16]
            root = Path(engine['OCX_CACHE_DIR']) / ('opencodex-swiftbar-v5-%s-%s' % (os.getuid(), scope)); root.mkdir(parents=True, exist_ok=True)
            for name, payload in records.items():
                (root / (name + '.json')).write_text(json.dumps({'schema': 1, 'fetchedAt': now - 30, 'payload': payload}))

    def test_snapshot_and_alerts(self):
        codex = Path(self.common['HOME']) / '.codex'; codex.mkdir()
        (codex / 'auth.json').write_text(json.dumps({'auth_mode': 'chatgpt', 'tokens': {'access_token': 'x'}}))
        everything = [m[0] for m in preferences.MODULES]
        keys = {'OCX_SELF_UPDATES': '0', 'MOONSHOT_API_KEY': 'k', 'XAI_TEAM_ID': 't', 'XAI_MANAGEMENT_API_KEY': 'k', 'MINIMAX_API_KEY': 'm'}
        variants = ((dict(visible=everything), {'OCX_SELF_UPDATES': '0'}),
                    (dict(visible=everything, currency='CNY', cnyRate=7.1234, serviceStatus=True, cursorUsage=True, menuBar='quota', usageAlert=80,
                          resetReminder=True, balanceAlerts={'kimi-api|CNY': 20, 'grok|USD': 1}, theme='dark', allowCollapse=True, pinInterval=5), keys),
                    (dict(visible=[k for k in everything if k != 'opencodex'], menuBar='quota', usageAlert=90, balanceAlerts={'kimi-api|CNY': 20}), keys),
                    (dict(visible=['claude', 'claude-subscription', 'chatgpt', 'sessions', 'kimi-api', 'minimax'], menuBar='cost', usageAlert=0,
                          keepCleanedUsage=False, appearance='solid'), dict(keys, KOMARI_BASE_URL='')),
                    (dict(visible=['chatgpt', 'opencodex', 'deepseek', 'summary'], range='7d'), {'OCX_SELF_UPDATES': '0', 'DEEPSEEK_API_KEY': 'sk'}))
        for values, extra in variants:
            with self.subTest(settings=values):
                self.settings(**values)
                self.caches(time.time())
                expected, got = self.python('snapshot', extra=extra), self.swift('snapshot', extra=extra)
                for key in expected:
                    self.assertEqual(self.steady(expected[key]), self.steady(got.get(key)), key)
                self.assertEqual(list(expected), list(got))
                self.assertEqual(self.python('alert_inputs', extra=extra), self.swift('alert_inputs', extra=extra))

    def test_due_alerts_and_notices(self):
        now = 1795000000.0
        windows = [{'id': 'a|5h', 'used': 95.0, 'resetsAt': now + 3600, 'span': 18000, 'current': True, 'name': 'Claude 订阅', 'window': '5 小时'},
                   {'id': 'a|w', 'used': 40.0, 'resetsAt': now + 7200, 'span': 604800, 'current': True, 'name': 'Claude 订阅', 'window': '本周'},
                   {'id': 'b|w', 'used': 99.0, 'resetsAt': None, 'span': None, 'current': True, 'name': 'ChatGPT 订阅', 'window': 'main'},
                   {'id': 'c|w', 'used': 99.0, 'resetsAt': now + 100, 'span': 86400, 'current': False, 'name': 'X', 'window': 'y'}]
        balances = [{'id': 'kimi-api|org', 'amount': 5.0, 'line': 10, 'currency': 'CNY', 'current': True, 'name': 'Kimi API'},
                    {'id': 'grok|t', 'amount': 50.0, 'line': 10, 'currency': 'USD', 'current': True, 'name': 'Grok API'},
                    {'id': 'deepseek|k', 'amount': 1.0, 'line': None, 'currency': 'CNY', 'current': True, 'name': 'DeepSeek API'}]
        marks = {'w|a|5h': {'at': now - 100, 'until': now + 3000}, 'b|grok|t': {'at': now - 50}, 'w|gone': {'at': now - 50 * 86400}, 'w|recent': {'at': now - 86400},
                 'r|a|w': {'at': now - 10, 'until': now - 4000}}
        for percent, reminder in ((90, True), (0, False), (50, True)):
            with self.subTest(percent=percent, reminder=reminder):
                arguments = (windows, balances, json.loads(json.dumps(marks)), percent, reminder, now)
                self.assertEqual(self.python('alerts_due', *arguments), self.swift('alerts_due', *arguments))
        moment = time.time()
        alerts = [dict(windows[0], kind='usage', resetsAt=moment + 3600), dict(windows[1], kind='renews', resetsAt=moment + 86400 * 3),
                  dict(windows[2], kind='usage'), dict(windows[0], kind='usage', used=89.5, resetsAt=moment + 86400 * 20),
                  dict(windows[0], kind='usage', resetsAt=moment + 86400), dict(balances[0], kind='balance')]
        self.assertEqual(self.python('alert_notices', alerts), self.swift('alert_notices', alerts))


class EngineRunParityTests(PanelParityTests):
    """Whole runs of the entry: the Python plugin and the Swift engine, each on its own copy of the same state."""

    def setUp(self):
        super().setUp()
        root = Path(self.temp.name) / 'plugins'; root.mkdir()
        # A stand-in entry: background runs the menu starts do nothing here.
        self.entry = root / 'starmeter.30s.sh'; self.entry.write_text('exit 0\n')
        self.common.update(OCX_PLUGIN_PATH=str(self.entry), OCX_SELF_UPDATES='0', OCX_PRICING_UPDATES='0')

    def run_engine(self, engine, arguments=(), extra=None):
        environment = {k: v for k, v in os.environ.items() if k not in ('DEEPSEEK_API_KEY', 'DEEPSEEK_API_KEY_FILE', 'TZ', 'OCX_BASE_URL')}
        environment.update(self.common, **self.states[engine], **(extra or {}))
        command = [sys.executable, str(ROOT / 'lib' / 'swiftbar_plugin.py')] if engine == 'python' else [ENGINE]
        done = subprocess.run(command + list(arguments), capture_output=True, env=environment, timeout=120)
        return done.returncode, done.stdout.decode()

    def cache_dir(self, engine):
        state = self.states[engine]['OCX_STATE_DIR']
        scope = hashlib.sha256(os.path.abspath(state).encode()).hexdigest()[:16]
        return Path(self.states[engine]['OCX_CACHE_DIR']) / ('opencodex-swiftbar-v5-%s-%s' % (os.getuid(), scope))

    def preferences(self, engine):
        values = json.loads((Path(self.states[engine]['OCX_STATE_DIR']) / 'preferences.json').read_text())
        values.pop('revision', None)
        return values

    def same_text(self, python, swift):
        # The plugin path differs only by which engine's state it names; nothing else may.
        self.assertEqual(python.splitlines(), swift.splitlines())

    def test_menu_text_and_snapshot(self):
        codex = Path(self.common['HOME']) / '.codex'; codex.mkdir()
        (codex / 'auth.json').write_text(json.dumps({'auth_mode': 'chatgpt'}))
        everything = [m[0] for m in preferences.MODULES]
        keys = {'MOONSHOT_API_KEY': 'k', 'XAI_TEAM_ID': 't', 'XAI_MANAGEMENT_API_KEY': 'k'}
        for values, extra in ((dict(visible=everything), {}), (dict(visible=[k for k in everything if k != 'opencodex'], menuBar='quota'), keys)):
            for panel in (False, True):
                with self.subTest(settings=values, panel=panel):
                    app = self.entry.parent / '.opencodex-panel' / 'OpenCodexPanel.app'
                    if panel:app.mkdir(parents=True, exist_ok=True)
                    elif app.exists():app.rmdir()
                    self.settings(**values)
                    self.caches(time.time())
                    python, swift = self.run_engine('python', extra=extra), self.run_engine('swift', extra=extra)
                    self.assertEqual(python[0], swift[0])
                    self.same_text(python[1], swift[1])
                    if panel:
                        expected = json.loads((self.cache_dir('python') / 'panel-view.json').read_text())
                        got = json.loads((self.cache_dir('swift') / 'panel-view.json').read_text())
                        for key in expected:self.assertEqual(self.steady(expected[key]), self.steady(got.get(key)), key)

    def test_actions_and_panel_requests(self):
        self.settings(visible=['summary', 'claude', 'codex'])
        actions = [['set-module', 'heatmap', 'true'], ['set-theme', 'dark'], ['set-collapse', 'true'], ['set-expanded', 'claude', 'true'],
                   ['set-range', '7d'], ['set-group', 'model'], ['set-pin-interval', '5'], ['set-menu-bar', 'cost'], ['set-usage-alert', '80'],
                   ['set-currency', 'CNY'], ['set-service-status', 'true'], ['set-subscription', 'gemini-subscription', 'true'], ['auto-source', 'claude'],
                   ['set-module', 'nope', 'true']]
        for action in actions:
            with self.subTest(action=action):
                python, swift = self.run_engine('python', ['--action', *action]), self.run_engine('swift', ['--action', *action])
                self.assertEqual(python[0] == 0, swift[0] == 0)
                self.assertEqual(self.preferences('python'), self.preferences('swift'))
                self.assertEqual(json.loads((self.cache_dir('python') / 'view-range.json').read_text()) if (self.cache_dir('python') / 'view-range.json').exists() else None,
                                 json.loads((self.cache_dir('swift') / 'view-range.json').read_text()) if (self.cache_dir('swift') / 'view-range.json').exists() else None)
        # A request from the panel runs only when the rendered panel offered it, and leaves a receipt either way.
        app = self.entry.parent / '.opencodex-panel' / 'OpenCodexPanel.app'; app.mkdir(parents=True)
        for request in (['set-theme', 'light'], ['set-module', 'sessions', 'true'], ['refresh-live'], ['set-pin-interval', '7'], ['set-expanded', 'codex', 'false']):
            with self.subTest(request=request):
                for engine in ('python', 'swift'):
                    self.run_engine(engine)
                    (self.cache_dir(engine) / 'panel-action.json').write_text(json.dumps({'id': 'r1', 'action': request, 'createdAt': time.time()}))
                    self.run_engine(engine)
                receipts = [json.loads((self.cache_dir(e) / 'panel-action-receipt.json').read_text()) for e in ('python', 'swift')]
                for receipt in receipts:
                    receipt.pop('finishedAt')
                    (receipt.get('preferences') or {}).pop('revision', None)
                self.assertEqual(receipts[0], receipts[1])
                self.assertEqual(self.preferences('python'), self.preferences('swift'))


class UpdateParityTests(unittest.TestCase):
    def test_pieces(self):
        import types, uuid
        plugin = types.ModuleType('plugin_updates')
        with unittest.mock.patch.dict(os.environ, {'OCX_PLUGIN_PATH': str(ROOT / 'starmeter.30s.sh')}):
            exec(compile((ROOT / 'lib/swiftbar_plugin.py').read_text(), 'plugin', 'exec'), plugin.__dict__)
        versions = ['1.2.3', 'v2.0', '1.2.3.4.5', '1.2.3.4.5.6', '1.2.3-beta.1', '1.2.3+build', '1.2.3-', ' 3 ', '', None, 'x1', '1..2', 7, '1.2.3-bé']
        outputs = ['2.1.47 (Claude Code)', 'codex-cli 0.130.0-alpha.3\n', 'version: 10', 'v1.2.3', 'a1.2.3', '1.2.3x', 'no version', '1.2.3.4.5.6']
        temp = tempfile.TemporaryDirectory(); self.addCleanup(temp.cleanup)
        root = Path(temp.name)
        def appcast(items):
            body = '<rss xmlns:sparkle="http://www.andymatuschak.org/xml-namespaces/sparkle"><channel>%s</channel></rss>'
            return body % ''.join('<item>%s</item>' % item for item in items)
        casts = [appcast(['<sparkle:shortVersionString>26.1</sparkle:shortVersionString><sparkle:version>2610</sparkle:version>',
                          '<sparkle:shortVersionString>26.2</sparkle:shortVersionString><sparkle:version>2620</sparkle:version><sparkle:channel>beta</sparkle:channel>',
                          '<enclosure sparkle:shortVersionString="26.0" sparkle:version="2700"/>',
                          '<sparkle:shortVersionString>99</sparkle:shortVersionString><sparkle:version>9900</sparkle:version><sparkle:minimumSystemVersion>99.0</sparkle:minimumSystemVersion>']),
                 appcast([]), 'not xml']
        cask = root / 'Caskroom' / 'codex' / '0.1' / 'codex'
        cask.parent.mkdir(parents=True); cask.write_text('')
        (root / 'Caskroom' / 'codex' / '.metadata').mkdir()
        (root / 'Caskroom' / 'codex' / '.metadata' / 'INSTALL_RECEIPT.json').write_text(json.dumps({'source': {'tap': 'homebrew/cask'}}))
        npm = root / 'lib' / 'node_modules' / '@openai' / 'codex' / 'bin' / 'codex.js'
        npm.parent.mkdir(parents=True); npm.write_text('')
        (npm.parent.parent / 'package.json').write_text(json.dumps({'name': '@openai/codex'}))
        loose = root / 'bin' / 'codex'; loose.parent.mkdir(); loose.write_text('')
        got = swift('updates.pieces', versions, outputs, casts, [str(cask), str(npm), str(loose)])
        self.assertEqual(got['versions'], [list(plugin.release_version(v)) if plugin.release_version(v) else None for v in versions])
        import re
        self.assertEqual(got['first'], [(lambda m: m[1] if m else None)(re.search(r'\b(\d+(?:\.\d+){1,4}(?:[-+][a-zA-Z0-9.-]+)?)\b', o)) for o in outputs])
        self.assertEqual(got['uuid'], str(uuid.uuid5(uuid.NAMESPACE_URL, 'opencodex-swiftbar-update-check')))
        expected_casts = []
        for cast in casts:
            try:expected_casts.append(list(plugin.sparkle_release(cast.encode())))
            except Exception:expected_casts.append([None, None])
        self.assertEqual(got['sparkle'], expected_casts)
        self.assertEqual(got['channels'], [update_channels.codex_channel(p) for p in (cask, npm, loose)])


if __name__ == '__main__':
    unittest.main(verbosity=2)
