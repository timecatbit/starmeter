"""Cursor usage, from the account's own usage records on cursor.com.

Cursor keeps no token counts on this computer: the desktop app's chat records carry zeros and the CLI's
transcripts none. So, only after the user turns it on, this reads the login the Cursor desktop app or
Cursor CLI saved, keeps the token in memory, and asks cursor.com for this account's usage events: one
record per request with its model, tokens and Cursor's list-price cost. The desktop app and the CLI sign in
to the same account, so both are in those records. The token is never stored, refreshed or written back,
and goes to cursor.com only. Request and cookie shapes follow CodexBar's Cursor provider (MIT, see
CODEXBAR_LICENSE).
"""
import base64
import json
import math
import os
from pathlib import Path
import sqlite3
import time
import urllib.error
import urllib.request

ORIGIN = 'https://cursor.com'
EVENTS_URL = ORIGIN + '/api/dashboard/get-filtered-usage-events'
SUMMARY_URL = ORIGIN + '/api/usage-summary'
PAGE_SIZE = 1000
MAX_PAGES = 50
ENCRYPTED = 'cursor-safe-storage:'


class CursorError(Exception):
    """A reason the usage can't be read, worded for the card."""


def app_root(home):
    """The desktop app's global state folder, where it keeps its login."""
    home = Path(home)
    if os.name == 'nt':
        return Path(os.environ.get('APPDATA') or home / 'AppData/Roaming') / 'Cursor/User/globalStorage'
    if os.uname().sysname == 'Darwin':
        return home / 'Library/Application Support/Cursor/User/globalStorage'
    return Path(os.environ.get('XDG_CONFIG_HOME') or home / '.config') / 'Cursor/User/globalStorage'


def cli_auth(home):
    """Where `cursor-agent login` keeps its login when it uses a file."""
    return Path(os.environ.get('XDG_CONFIG_HOME') or Path(home) / '.config') / 'cursor/auth.json'


def app_token(root):
    """The desktop app's access token, read-only; never recreates Cursor's own files."""
    path = Path(root) / 'state.vscdb'
    if not path.is_file():return None
    def read(uri):
        connection = sqlite3.connect(uri, uri=True, timeout=2)
        try:
            row = connection.execute("SELECT value FROM ItemTable WHERE key='cursorAuth/accessToken' LIMIT 1").fetchone()
        finally:connection.close()
        return row[0] if row else None
    try:value = read(path.resolve().as_uri() + '?mode=ro')
    except sqlite3.Error:
        # An idle database can keep WAL mode with no sidecars; immutable reads it without creating them.
        if any(Path(str(path) + s).exists() for s in ('-wal', '-shm')):return None
        try:value = read(path.resolve().as_uri() + '?immutable=1')
        except sqlite3.Error:return None
    if isinstance(value, bytes):
        value = value.decode('utf-16-le' if value[1:2] == b'\0' else 'utf-8', 'replace')
    return value.strip() if isinstance(value, str) else None


def file_token(path):
    try:value = json.loads(Path(path).read_text(encoding='utf-8')).get('accessToken')
    except (OSError, ValueError, AttributeError):return None
    return value.strip() if isinstance(value, str) else None


def claims(token):
    try:
        part = token.split('.')[1];part += '=' * (-len(part) % 4)
        value = json.loads(base64.urlsafe_b64decode(part))
        return value if isinstance(value, dict) else {}
    except (IndexError, ValueError, TypeError):return {}


def session(home, now=None):
    """(user ID, token, where it came from) for the first usable login, desktop app first, then the CLI."""
    now = time.time() if now is None else now;problems = []
    for where, token in (('Cursor 桌面版', app_token(app_root(home))), ('Cursor CLI', file_token(cli_auth(home)))):
        if not token:continue
        if token.startswith(ENCRYPTED):problems.append('encrypted');continue
        info = claims(token);subject = str(info.get('sub') or '').split('|')[-1]
        if not subject:problems.append('format');continue
        if not isinstance(info.get('exp'), (int, float)) or info['exp'] <= now + 60:problems.append('expired');continue
        return subject, token, where
    if 'expired' in problems:raise CursorError('Cursor 登录已过期，请打开 Cursor 重新登录')
    if 'encrypted' in problems:raise CursorError('Cursor 登录已加密保存，暂时无法读取')
    if 'format' in problems:raise CursorError('无法识别 Cursor 登录信息')
    raise CursorError('未找到 Cursor 登录，请先登录 Cursor 桌面版或 Cursor CLI')


class NoRedirect(urllib.request.HTTPRedirectHandler):
    # The session cookie goes to cursor.com only; a redirect means the login was not accepted.
    def redirect_request(self, *args, **kwargs):return None


def post(body, user, token, opener=None, url=EVENTS_URL):
    """POST the body to a cursor.com endpoint (GET when there is none), signed in with the session cookie."""
    headers = {'Accept': 'application/json', 'Origin': ORIGIN, 'Cookie': 'WorkosCursorSessionToken=%s%%3A%%3A%s' % (user, token)}
    if body is not None:headers['Content-Type'] = 'application/json'
    request = urllib.request.Request(url, data=None if body is None else json.dumps(body).encode(),
                                     method='GET' if body is None else 'POST', headers=headers)
    opener = opener or urllib.request.build_opener(NoRedirect)
    try:
        with opener.open(request, timeout=20) as response:raw = response.read(32 * 1024 * 1024 + 1)
    except urllib.error.HTTPError as exc:
        if exc.code in (301, 302, 303, 307, 308, 401):raise CursorError('Cursor 登录已失效，请打开 Cursor 重新登录') from None
        if exc.code == 403:raise CursorError('cursor.com 拒绝读取用量记录') from None
        if exc.code == 429:raise CursorError('cursor.com 请求过于频繁，稍后重试') from None
        raise CursorError('cursor.com 暂时不可用（HTTP %d）' % exc.code) from None
    except (urllib.error.URLError, OSError):raise CursorError('无法连接 cursor.com') from None
    if len(raw) > 32 * 1024 * 1024:raise CursorError('用量记录过大')
    try:value = json.loads(raw)
    except ValueError:raise CursorError('用量记录格式异常') from None
    if not isinstance(value, dict):raise CursorError('用量记录格式异常')
    return value


def events(user, token, start, end, opener=None):
    """Every usage event between start and end (seconds), page by page. Returns (events, complete).
    Pages can repeat a row at their boundary, so an event ID seen before is skipped."""
    rows = [];seen = set()
    for page in range(1, MAX_PAGES + 1):
        value = post({'page': page, 'pageSize': PAGE_SIZE, 'startDate': str(int(max(0, start) * 1000)),
                      'endDate': str(int(end * 1000))}, user, token, opener)
        batch = value.get('usageEventsDisplay') or []
        if not isinstance(batch, list):raise CursorError('用量记录格式异常')
        for row in batch:
            if not isinstance(row, dict):continue
            key = row.get('usageEventId')
            if key:
                if key in seen:continue
                seen.add(key)
            rows.append(row)
        total = value.get('totalUsageEventsCount')
        if len(batch) < PAGE_SIZE or (isinstance(total, int) and len(rows) >= total):return rows, True
    return rows, False


def number(value):
    try:
        value = float(value)
        return value if math.isfinite(value) and value >= 0 else 0
    except (TypeError, ValueError):return 0


def normalize(row):
    """One event in the shape the shared tally reads. Cursor's input count excludes cache reads and writes,
    and totalCents is the request at the model's list price, which is what Summary estimates everywhere."""
    usage = row.get('tokenUsage') if isinstance(row.get('tokenUsage'), dict) else {}
    try:stamp = int(str(row.get('timestamp'))) / 1000
    except (TypeError, ValueError):return None
    cents = usage.get('totalCents')
    cost = number(cents) / 100 if isinstance(cents, (int, float)) and not isinstance(cents, bool) and cents >= 0 else None
    return {'timestamp': stamp, 'model': str(row.get('model') or 'default'), 'session': row.get('conversationId') or None,
            'input': number(usage.get('inputTokens')), 'output': number(usage.get('outputTokens')),
            'cached': number(usage.get('cacheReadTokens')), 'writes': number(usage.get('cacheWriteTokens')), 'cost': cost}


def plan(home, now=None, opener=None):
    """This billing cycle's included plan usage, as Cursor's dashboard shows it. Cursor's own words ("You've used 1%
    of your included total usage") follow used ÷ limit, so that is the share; its percent fields only stand in
    when there is no limit. Amounts are in cents."""
    from swiftbar_time import parse_iso
    user, token, where = session(home, now)
    value = post(None, user, token, opener, SUMMARY_URL)
    usage = value.get('individualUsage') if isinstance(value.get('individualUsage'), dict) else {}
    part = lambda name: usage.get(name) if isinstance(usage.get(name), dict) else {}
    plan, overall, extra = part('plan'), part('overall'), part('onDemand')
    used = limit = percent = None
    for block in (plan, overall):
        if number(block.get('limit')) > 0:
            used, limit = number(block.get('used')), number(block.get('limit'));percent = 100 * used / limit;break
    if percent is None and isinstance(plan.get('totalPercentUsed'), (int, float)) and not isinstance(plan['totalPercentUsed'], bool):
        percent = number(plan['totalPercentUsed'])
    def stamp(key):
        try:return parse_iso(value[key]).timestamp()
        except (KeyError, TypeError, ValueError, OverflowError, AttributeError):return None
    return {'membership': str(value.get('membershipType') or ''), 'unlimited': value.get('isUnlimited') is True,
            'percent': percent, 'usedCents': used, 'limitCents': limit, 'cycleStart': stamp('billingCycleStart'),
            'cycleEnd': stamp('billingCycleEnd'), 'login': where,
            'onDemand': {'enabled': extra.get('enabled') is True, 'usedCents': number(extra.get('used')),
                         'limitCents': number(extra.get('limit')) if number(extra.get('limit')) > 0 else None}}


def fetch(home, start, end, now=None, opener=None):
    """This range's usage events in the shape the shared tally reads, whether all of them came, and whose login read them."""
    user, token, where = session(home, now)
    rows, complete = events(user, token, start, end, opener)
    return [e for e in map(normalize, rows) if e], complete, where


def tally(found, start, end, zone, carriers=None):
    """The card's figures from fetched events. A call Cursor also reported to Magpie is in Magpie's log as well;
    carriers let Summary count it once, from Magpie, as for every other client a gateway logs."""
    import swiftbar_sources as sources
    rows, complete, where = found
    result = sources.aggregate(rows, start, end, zone, price=sources.model_cost, carriers=carriers)
    result.update(source='Cursor 账户用量记录', login=where)
    if not complete:result.update(partial=True, warning='用量记录过多，只读取了一部分')
    return result


def usage(home, start, end, zone, now=None, opener=None, carriers=None):
    return tally(fetch(home, start, end, now, opener), start, end, zone, carriers)
