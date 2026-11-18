"""Holds the Swift engine (engine/) against the Python one: the same inputs must give the same answers, down to
int-or-float and key order, because the two share state files while the port is under way."""
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import unittest
from datetime import timezone

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'lib'))
import swiftbar_catalog as catalog
import swiftbar_files as files
import swiftbar_preferences as preferences
from swiftbar_time import parse_iso

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


if __name__ == '__main__':
    unittest.main(verbosity=2)
