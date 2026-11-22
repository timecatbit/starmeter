import base64
import io
import json
import os
from pathlib import Path
import sqlite3
import sys
import tempfile
import types
import unittest
from datetime import datetime
from unittest.mock import patch
from zoneinfo import ZoneInfo
import urllib.error

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'lib'))
import swiftbar_cursor as cursor
import swiftbar_discovery as discovery
import swiftbar_preferences as preferences


def jwt(sub='auth0|user_01ABC', exp=None):
    part = lambda value: base64.urlsafe_b64encode(json.dumps(value).encode()).decode().rstrip('=')
    return part({'alg': 'HS256'}) + '.' + part({'sub': sub, 'exp': exp if exp is not None else 4102444800}) + '.sig'


class Response(io.BytesIO):
    def __enter__(self):return self
    def __exit__(self, *args):return False


class Opener:
    """Answers each POST with the next page and remembers what was sent."""
    def __init__(self, pages):self.pages = list(pages);self.sent = []
    def open(self, request, timeout=None):
        self.sent.append(request)
        page = self.pages.pop(0)
        if isinstance(page, Exception):raise page
        return Response(json.dumps(page).encode())


def event(ms, model='claude-4.5-sonnet', tokens=(100, 20, 1000, 50), cents=1.5, conversation='c1', key=None):
    usage = dict(zip(('inputTokens', 'outputTokens', 'cacheReadTokens', 'cacheWriteTokens'), tokens))
    if cents is not None:usage['totalCents'] = cents
    return {'timestamp': str(ms), 'model': model, 'tokenUsage': usage, 'conversationId': conversation,
            'usageEventId': key or 'e%d' % ms, 'kind': 'USAGE_EVENT_KIND_INCLUDED_IN_PRO'}


class Server:
    """Cursor's usage records: answers each read with the events in its window and remembers the windows."""
    def __init__(self, rows):self.rows = list(rows);self.windows = []
    def events(self, user, token, begin, end, opener=None):
        self.windows.append((begin, end))
        return [r for r in self.rows if begin * 1000 <= int(r['timestamp']) <= end * 1000], True


LOGIN = ('user_01ABC', 'tok', 'Cursor 桌面版')


class CursorTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory();self.addCleanup(self.temp.cleanup);self.home = Path(self.temp.name)
        env = patch.dict(os.environ, {'XDG_CONFIG_HOME': str(self.home / 'config')});env.start();self.addCleanup(env.stop)
        self.root = cursor.app_root(self.home)

    def app(self, token):
        self.root.mkdir(parents=True, exist_ok=True)
        db = sqlite3.connect(self.root / 'state.vscdb')
        db.execute('CREATE TABLE IF NOT EXISTS ItemTable (key TEXT UNIQUE, value BLOB)')
        db.execute("INSERT OR REPLACE INTO ItemTable VALUES ('cursorAuth/accessToken', ?)", (token,));db.commit();db.close()

    def cli(self, token):
        path = cursor.cli_auth(self.home);path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps({'accessToken': token}))

    def test_the_desktop_login_comes_first_and_the_cli_login_stands_in(self):
        with self.assertRaisesRegex(cursor.CursorError, '未找到 Cursor 登录'):cursor.session(self.home)
        self.cli(jwt('github|cli_user'))
        self.assertEqual(cursor.session(self.home)[::2], ('cli_user', 'Cursor CLI'))
        self.app(jwt())
        user, token, where = cursor.session(self.home)
        self.assertEqual((user, where), ('user_01ABC', 'Cursor 桌面版'));self.assertEqual(token, jwt())

    def test_expired_or_encrypted_logins_are_not_sent_and_say_why(self):
        self.app(jwt(exp=1000))
        with self.assertRaisesRegex(cursor.CursorError, '已过期'):cursor.session(self.home, now=2000)
        self.app('cursor-safe-storage:v1:abcd')
        with self.assertRaisesRegex(cursor.CursorError, '已加密'):cursor.session(self.home)
        # A UTF-16 value, as some Cursor versions store it, still reads.
        self.app(jwt().encode('utf-16-le'))
        self.assertEqual(cursor.session(self.home)[0], 'user_01ABC')

    def test_reading_never_creates_files_beside_cursors_database(self):
        self.app(jwt());before = sorted(p.name for p in self.root.iterdir())
        cursor.session(self.home)
        self.assertEqual(sorted(p.name for p in self.root.iterdir()), before)

    def test_events_page_through_and_skip_repeated_rows(self):
        cursor.PAGE_SIZE, size = 2, cursor.PAGE_SIZE
        self.addCleanup(setattr, cursor, 'PAGE_SIZE', size)
        opener = Opener([{'usageEventsDisplay': [event(3), event(2)], 'totalUsageEventsCount': 3},
                         {'usageEventsDisplay': [event(2), event(1)], 'totalUsageEventsCount': 3},
                         {'totalUsageEventsCount': 3}])
        rows, complete = cursor.events('user_01ABC', 'tok', 10, 20, opener)
        self.assertTrue(complete);self.assertEqual([r['usageEventId'] for r in rows], ['e3', 'e2', 'e1'])
        request = opener.sent[0]
        self.assertEqual(request.full_url, 'https://cursor.com/api/dashboard/get-filtered-usage-events')
        self.assertEqual(request.get_header('Origin'), 'https://cursor.com')
        self.assertEqual(request.get_header('Cookie'), 'WorkosCursorSessionToken=user_01ABC%3A%3Atok')
        self.assertEqual(json.loads(request.data), {'page': 1, 'pageSize': 2, 'startDate': '10000', 'endDate': '20000'})

    def test_refusals_explain_themselves(self):
        def failing(code):return Opener([urllib.error.HTTPError(cursor.EVENTS_URL, code, 'x', {}, None)])
        for code, words in ((401, '登录已失效'), (302, '登录已失效'), (403, '拒绝'), (500, 'HTTP 500')):
            with self.assertRaisesRegex(cursor.CursorError, words):cursor.events('u', 't', 0, 1, failing(code))
        with self.assertRaisesRegex(cursor.CursorError, '无法连接'):
            cursor.events('u', 't', 0, 1, Opener([urllib.error.URLError('offline')]))

    def test_usage_counts_tokens_cost_and_conversations_like_other_sources(self):
        self.app(jwt());zone = ZoneInfo('Asia/Taipei')
        noon = int(datetime(2026, 11, 17, 12, tzinfo=zone).timestamp() * 1000)
        rows = [event(noon), event(noon + 1000, conversation='c2', cents=None, model='mystery-model'),
                event(noon + 2000, tokens=(0, 0, 0, 0), cents=0)]
        opener = Opener([{'usageEventsDisplay': rows, 'totalUsageEventsCount': 3}])
        start = datetime(2026, 11, 17, tzinfo=zone).timestamp()
        store = cursor.refresh(None, self.home, start, start + 86400, opener=opener)
        result = cursor.tally((store['events'], store['complete'], store['login']), start, start + 86400, zone)
        summary = result['summary']
        # Cursor's input excludes the cache: 100 input + 1000 read + 50 written + 20 output.
        self.assertEqual((summary['requests'], summary['totalTokens'], summary['inputTokens'], summary['cachedInputTokens']), (2, 2340, 2300, 2000))
        self.assertAlmostEqual(summary['estimatedCostUsd'], 0.015);self.assertEqual(summary['unpricedRequests'], 1)
        self.assertEqual(summary['sessions'], 2);self.assertEqual(result['login'], 'Cursor 桌面版')
        self.assertEqual([h['timestamp'] for h in result['hours']], [noon // 1000 // 3600 * 3600])

    def test_the_store_reads_only_what_it_lacks(self):
        day = 1_800_000_000;now = day + 12 * 3600
        server = Server([event((day + 3600) * 1000), event((now - 600) * 1000)])
        with patch.object(cursor, 'session', return_value=LOGIN), patch.object(cursor, 'events', server.events):
            store = cursor.refresh(None, self.home, day, now)
            self.assertEqual(server.windows, [(day, now)]);self.assertEqual((store['from'], store['through']), (day, now))
            # Ten minutes on, only the time since, with the hour before it, is read; there a record changed.
            server.rows[1]['tokenUsage']['totalCents'] = 3;server.rows.append(event((now + 300) * 1000))
            store = cursor.refresh(store, self.home, day, now + 600)
            self.assertEqual(server.windows[1], (now - 3601, now + 600))
            self.assertEqual([round(e['cost'] or 0, 3) for e in store['events']], [0.015, 0.03, 0.015])
            # A range reaching six days back reads only those days, then the minutes since.
            server.rows.insert(0, event((day - 5 * 86400) * 1000))
            store = cursor.refresh(store, self.home, day - 6 * 86400, now + 900)
            self.assertEqual(server.windows[2:], [(day - 6 * 86400, day + 1), (now - 3001, now + 900)])
            self.assertEqual((len(store['events']), store['from']), (4, day - 6 * 86400))
            # Another account's store, or one a read could not finish, starts over with one read.
            for held in (dict(store, account='someone-else'), dict(store, complete=False)):
                server.windows.clear();cursor.refresh(held, self.home, day, now + 900)
                self.assertEqual(server.windows, [(day, now + 900)])
        self.assertNotIn('user_01ABC', json.dumps(store), 'The account ID itself is not kept')

    def test_plan_share_follows_used_over_limit_as_cursor_words_it(self):
        self.app(jwt())
        summary={'billingCycleStart':'2026-11-03T08:50:27.000Z','billingCycleEnd':'2026-12-03T08:50:27.000Z','membershipType':'enterprise',
                 'isUnlimited':False,'individualUsage':{'plan':{'used':20,'limit':2000,'totalPercentUsed':0.08},'onDemand':{'enabled':False,'used':0,'limit':None}}}
        opener=Opener([summary])
        plan=cursor.plan(self.home,opener=opener)
        self.assertEqual(opener.sent[0].get_method(),'GET');self.assertEqual(opener.sent[0].full_url,'https://cursor.com/api/usage-summary')
        self.assertEqual((plan['percent'],plan['usedCents'],plan['limitCents'],plan['membership']),(1.0,20,2000,'enterprise'))
        self.assertEqual(plan['cycleEnd']-plan['cycleStart'],30*86400);self.assertFalse(plan['onDemand']['enabled'])
        # Without a limit, Cursor's own percentage stands in.
        plan=cursor.plan(self.home,opener=Opener([{'individualUsage':{'plan':{'totalPercentUsed':12.5}}}]))
        self.assertEqual((plan['percent'],plan['limitCents'],plan['cycleEnd']),(12.5,None,None))

    def test_detection_offers_cursor_when_the_app_or_cli_is_installed(self):
        apps = self.home / 'Applications';(apps / 'Cursor.app').mkdir(parents=True)
        found = discovery.detect({}, self.home, which=lambda name: None, app_roots=[apps])
        self.assertIn('cursor', found)
        found = discovery.detect({}, self.home, which=lambda name: '/bin/x' if name == 'cursor-agent' else None)
        self.assertIn('cursor', found)
        self.assertNotIn('cursor', discovery.detect({}, self.home, which=lambda name: None))


class CursorPluginTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory();self.addCleanup(self.temp.cleanup)
        env = patch.dict(os.environ, {'OCX_CACHE_DIR': self.temp.name, 'OCX_STATE_DIR': self.temp.name + '/settings',
                                      'OCX_PLUGIN_PATH': str(ROOT / 'starmeter.30s.sh')})
        env.start();self.addCleanup(env.stop)
        self.m = types.ModuleType('cursor_plugin_test')
        exec(compile((ROOT / 'lib/swiftbar_plugin.py').read_text(), 'plugin', 'exec'), self.m.__dict__)
        self.now = datetime(2026, 11, 17, 12, tzinfo=ZoneInfo('Asia/Taipei'))

    def test_nothing_is_read_until_the_user_turns_it_on(self):
        start, end = self.m.range_bounds('today', self.now)
        with patch.object(self.m.cursor, 'refresh') as read:
            self.assertEqual(self.m.cursor_range_usage(start, end, self.now), {'error': '未开启用量读取'})
        read.assert_not_called()
        with patch.object(self.m, 'confirm', return_value=False):self.m.run_action('set-cursor-usage', ['true'])
        self.assertFalse(preferences.read(self.m.STATE_DIR)['cursorUsage'], 'Cancelling writes nothing')
        with patch.object(self.m, 'confirm', return_value=True) as ask, patch.object(self.m, 'schedule_background_refresh'):
            self.m.run_action('set-cursor-usage', ['true'])
        self.assertIn('只在内存中使用', ask.call_args.args[1])
        saved = preferences.read(self.m.STATE_DIR);self.assertTrue(saved['cursorUsage']);self.assertIn('cursor', saved['visible'])

    def test_every_range_shares_one_store_and_reads_only_what_it_lacks(self):
        preferences.update(self.m.STATE_DIR, 'cursor-usage', 'true')
        start, end = self.m.range_bounds('today', self.now)
        server = Server([event(int(start + 3600) * 1000, model='grok-4.7-high', tokens=(100, 20, 50, 0), cents=1),
                         event(int(start - 2 * 86400) * 1000)])
        with patch.object(self.m.cursor, 'session', return_value=LOGIN), patch.object(self.m.cursor, 'events', server.events):
            first = self.m.cursor_range_usage(start, end, self.now)
            self.m.cursor_range_usage(start, end, self.now)
            self.assertEqual(len(server.windows), 1, 'A fresh read is reused for five minutes')
            week = self.m.cursor_range_usage(*self.m.range_bounds('7d', self.now), self.now)
            self.assertEqual(server.windows[1][0], self.m.range_bounds('7d', self.now)[0], 'Only the older days are read')
            self.assertEqual(week['summary']['requests'], 2)
            reads = len(server.windows)
            for key in ('today', 'yesterday', '24h'):self.m.cursor_range_usage(*self.m.range_bounds(key, self.now), self.now)
            self.assertEqual(len(server.windows), reads, 'Ranges inside the store read nothing')
            later = self.now.replace(day=18);start, end = self.m.range_bounds('today', later)
            self.m.cursor_range_usage(start, end, later)
            self.assertEqual(len(server.windows), reads + 1, 'A store behind the range reads the time since')
            self.assertEqual(server.windows[-1][1], later.timestamp())
        self.assertEqual((first['summary']['requests'], first['summary']['totalTokens'], first['login']), (1, 170, 'Cursor 桌面版'))
        names = [n for n in os.listdir(self.m.CACHE_DIR) if n.startswith('cursor_usage_')]
        self.assertEqual(names, ['cursor_usage_events.json'])
        with patch.object(self.m.cursor, 'session', side_effect=self.m.cursor.CursorError('Cursor 登录已失效，请打开 Cursor 重新登录')):
            later = self.now.replace(day=19);start, end = self.m.range_bounds('today', later)
            self.assertEqual(self.m.cursor_range_usage(start, end, later), {'error': 'Cursor 登录已失效，请打开 Cursor 重新登录'})

    def test_a_call_cursor_reported_to_magpie_counts_once_from_magpie(self):
        preferences.update(self.m.STATE_DIR, 'cursor-usage', 'true')
        start, end = self.m.range_bounds('today', self.now)
        reported = {'timestamp': start + 3600, 'model': 'grok-4.7-high', 'session': 'c1', 'input': 11535, 'output': 95, 'cached': 5760, 'writes': 0, 'cost': 0.0265}
        other = dict(reported, timestamp=start + 7200, input=451, output=235, cached=1152, cost=0.0029)
        # Magpie logs input with cache reads: 11535 + 5760.
        magpie = [('Magpie', {(17295, 5760, 0, 95): [start + 3610]})]
        store = {'events': [reported, other], 'complete': True, 'login': 'Cursor 桌面版', 'from': start, 'through': end}
        with patch.object(self.m.cursor, 'session', return_value=LOGIN), patch.object(self.m.cursor, 'refresh', return_value=store):
            result = self.m.cursor_range_usage(start, end, self.now, magpie)
        self.assertEqual(result['summary']['requests'], 2, 'The card keeps every Cursor call')
        self.assertEqual(result['carried'], {'Magpie': 1});self.assertEqual(result['counted']['summary']['requests'], 1)

    def test_the_subscription_row_waits_for_consent_then_shows_the_cycle(self):
        lines=lambda data:self.m.panel_entries(self.m.cursor_subscription_lines(data))[0]
        row=lines({});self.assertEqual(row['value'],'未开启读取')
        self.assertIn(['set-cursor-usage','true'],[c.get('action') for c in row['children']])
        self.assertNotIn('cursor_subscription',self.m.external_loaders())
        preferences.update(self.m.STATE_DIR,'cursor-usage','true');preferences.update(self.m.STATE_DIR,'module','cursor-subscription','true')
        self.assertIn('cursor_subscription',self.m.external_loaders())
        start=self.now.timestamp()-15*86400
        payload={'percent':92.0,'usedCents':1840,'limitCents':2000,'cycleStart':start,'cycleEnd':start+30*86400,'onDemand':{'enabled':True,'usedCents':350,'limitCents':5000},'_fetchedAt':self.now.timestamp()}
        with patch.object(self.m.time,'time',return_value=self.now.timestamp()):
            row=lines({'cursor_subscription':payload,'cursor_subscription_error':None})
        self.assertEqual((row['value'],row['usageLevel']),('已用 92%','critical'))
        cycle=next(c for c in row['children'] if c['label']=='本周期')
        self.assertTrue(cycle['detail'].startswith('重置于 '));self.assertIn('比匀速多用 42%',cycle['detail'])
        self.assertEqual(next(c['value'] for c in row['children'] if c['label']=='套餐额度'),'$18.40 / $20.00')
        self.assertEqual(next(c['value'] for c in row['children'] if c['label']=='按需用量'),'$3.50 / $50.00')

    def test_every_module_has_its_icon_in_the_list_and_on_its_card(self):
        rows = {row['action'][1]: row for row in self.m.module_options() if (row.get('action') or [''])[0] == 'set-module'}
        self.assertEqual(set(rows), preferences.KNOWN)
        for key, row in rows.items():
            self.assertTrue(row.get('image') or row.get('symbol'), key)
        self.assertTrue(rows['cursor']['template']);self.assertFalse(rows['claude']['template'])
        self.assertEqual(rows['summary']['symbol'], 'chart.xyaxis.line')
        self.assertEqual(base64.b64decode(rows['cursor']['image'])[:4], b'<svg')
        sidebar = rows['cursor']['children']
        # Its page leads with whether it shows on the homepage, then reading Cursor's usage.
        self.assertEqual((sidebar[0]['label'], sidebar[0]['action']), ('在主页显示', ['set-module', 'cursor', 'true']));self.assertTrue(sidebar[1]['separator'])
        self.assertEqual(sidebar[2]['action'], ['set-cursor-usage', 'true']);self.assertFalse(sidebar[2]['checked'])
        self.m.preferences.write(self.m.STATE_DIR, dict(self.m.view_settings(), visible=['summary', 'cursor']))
        with patch.object(self.m, 'usage_now', return_value=self.now):
            card = self.m.dashboard_payload({'range_usage': {'range': 'today', 'cursor': {'summary': {'requests': 1, 'totalTokens': 9}}}})['providers'][0]
        self.assertEqual((card['id'], card['name'], card['template']), ('cursor', 'Cursor', True))


if __name__ == '__main__':
    unittest.main()
