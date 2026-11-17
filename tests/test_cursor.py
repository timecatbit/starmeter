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
        result = cursor.usage(self.home, start, start + 86400, zone, opener=opener)
        summary = result['summary']
        # Cursor's input excludes the cache: 100 input + 1000 read + 50 written + 20 output.
        self.assertEqual((summary['requests'], summary['totalTokens'], summary['inputTokens'], summary['cachedInputTokens']), (2, 2340, 2300, 2000))
        self.assertAlmostEqual(summary['estimatedCostUsd'], 0.015);self.assertEqual(summary['unpricedRequests'], 1)
        self.assertEqual(summary['sessions'], 2);self.assertEqual(result['login'], 'Cursor 桌面版')
        self.assertEqual([h['timestamp'] for h in result['hours']], [noon // 1000 // 3600 * 3600])

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
                                      'OCX_PLUGIN_PATH': str(ROOT / 'token-monitor.30s.sh')})
        env.start();self.addCleanup(env.stop)
        self.m = types.ModuleType('cursor_plugin_test')
        exec(compile((ROOT / 'lib/swiftbar_plugin.py').read_text(), 'plugin', 'exec'), self.m.__dict__)
        self.now = datetime(2026, 11, 17, 12, tzinfo=ZoneInfo('Asia/Taipei'))

    def test_nothing_is_read_until_the_user_turns_it_on(self):
        start, end = self.m.range_bounds('today', self.now)
        with patch.object(self.m.cursor, 'usage') as read:
            self.assertEqual(self.m.cursor_range_usage('today', start, end, self.now), {'error': '未开启用量读取'})
        read.assert_not_called()
        with patch.object(self.m, 'confirm', return_value=False):self.m.run_action('set-cursor-usage', ['true'])
        self.assertFalse(preferences.read(self.m.STATE_DIR)['cursorUsage'], 'Cancelling writes nothing')
        with patch.object(self.m, 'confirm', return_value=True) as ask, patch.object(self.m, 'schedule_background_refresh'):
            self.m.run_action('set-cursor-usage', ['true'])
        self.assertIn('只在内存中使用', ask.call_args.args[1])
        saved = preferences.read(self.m.STATE_DIR);self.assertTrue(saved['cursorUsage']);self.assertIn('cursor', saved['visible'])

    def test_figures_are_cached_per_range_and_refetched_on_a_new_day(self):
        preferences.update(self.m.STATE_DIR, 'cursor-usage', 'true')
        start, end = self.m.range_bounds('today', self.now)
        with patch.object(self.m.cursor, 'usage', return_value={'summary': {'requests': 3, 'totalTokens': 30}}) as read:
            first = self.m.cursor_range_usage('today', start, end, self.now)
            self.m.cursor_range_usage('today', start, end, self.now)
            self.assertEqual(read.call_count, 1, 'A fresh read is reused for five minutes')
            later = self.now.replace(day=18);start, end = self.m.range_bounds('today', later)
            self.m.cursor_range_usage('today', start, end, later)
            self.assertEqual(read.call_count, 2, 'A new day reads again')
        self.assertEqual(first['summary']['requests'], 3);self.assertNotIn('_fetchedAt', first)
        with patch.object(self.m.cursor, 'usage', side_effect=self.m.cursor.CursorError('Cursor 登录已失效，请打开 Cursor 重新登录')):
            later = self.now.replace(day=19);start, end = self.m.range_bounds('today', later)
            self.assertEqual(self.m.cursor_range_usage('today', start, end, later), {'error': 'Cursor 登录已失效，请打开 Cursor 重新登录'})

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
        self.assertEqual(sidebar[0]['action'], ['set-cursor-usage', 'true']);self.assertFalse(sidebar[0]['checked'])
        self.m.preferences.write(self.m.STATE_DIR, dict(self.m.view_settings(), visible=['summary', 'cursor']))
        with patch.object(self.m, 'usage_now', return_value=self.now):
            card = self.m.dashboard_payload({'range_usage': {'range': 'today', 'cursor': {'summary': {'requests': 1, 'totalTokens': 9}}}})['providers'][0]
        self.assertEqual((card['id'], card['name'], card['template']), ('cursor', 'Cursor', True))


if __name__ == '__main__':
    unittest.main()
