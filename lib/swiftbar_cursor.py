"""Cursor usage, from the account's own usage records on cursor.com.

Cursor keeps no token counts on this computer: the desktop app's chat records carry zeros and the CLI's
transcripts none. So, only after the user turns it on, this reads the login the Cursor desktop app or
Cursor CLI saved, keeps the token in memory, and asks cursor.com for this account's usage events: one
record per request with its model, tokens and Cursor's list-price cost. The desktop app and the CLI sign in
to the same account, so both are in those records. The token is never stored, refreshed or written back,
and goes to cursor.com only. Request and cookie shapes follow CodexBar's Cursor provider (MIT, see
CODEXBAR_LICENSE). A desktop app that keeps its login encrypted (Electron's safeStorage, its key in the keychain
as “Cursor Safe Storage”) is read only once the user has allowed it on the Cursor page, and macOS has asked them.
"""
import base64
import ctypes
import hashlib
import json
import math
import os
from pathlib import Path
import sqlite3
import subprocess
import sys
import time
import urllib.error
import urllib.request

ORIGIN = 'https://cursor.com'
EVENTS_URL = ORIGIN + '/api/dashboard/get-filtered-usage-events'
SUMMARY_URL = ORIGIN + '/api/usage-summary'
PAGE_SIZE = 1000
MAX_PAGES = 50
ENCRYPTED = 'cursor-safe-storage:'
# Read again on every refresh: requests still running and records that land late.
OVERLAP = 3600


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


ENCRYPTED_LOGIN = '这个版本的 Cursor 把登录加密存进了钥匙串；打开下面的“从钥匙串读取登录”，系统询问时选“始终允许”'
KEYCHAIN_DENIED = '钥匙串没有允许读取 Cursor 登录；关掉再打开下面的“从钥匙串读取登录”，系统询问时选“始终允许”'


def login_encrypted(home):
    """Whether the desktop app keeps its login encrypted (what the Cursor page offers to read from the keychain for)."""
    if sys.platform != 'darwin':return False  # the keychain read is macOS's
    token = app_token(app_root(home))
    return bool(token) and token.startswith(ENCRYPTED)


def _crypt(encrypt, key, data):
    """AES-128-CBC, PKCS7 padding, an IV of sixteen spaces, through the system's CommonCrypto; None when it fails."""
    crypto = ctypes.CDLL('/usr/lib/system/libcommonCrypto.dylib')
    crypto.CCCrypt.argtypes = [ctypes.c_uint, ctypes.c_uint, ctypes.c_uint, ctypes.c_void_p, ctypes.c_size_t, ctypes.c_void_p,
                               ctypes.c_void_p, ctypes.c_size_t, ctypes.c_void_p, ctypes.c_size_t, ctypes.POINTER(ctypes.c_size_t)]
    crypto.CCCrypt.restype = ctypes.c_int
    output = ctypes.create_string_buffer(len(data) + 16);written = ctypes.c_size_t()
    if crypto.CCCrypt(0 if encrypt else 1, 0, 1, key, len(key), b' ' * 16, data, len(data), output, len(output), ctypes.byref(written)):return None
    return output.raw[:written.value]


def safe_storage_key(password):
    return hashlib.pbkdf2_hmac('sha1', password, b'saltysalt', 1003, 16)


def decrypt_login(stored, password):
    """Electron's safeStorage as Cursor keeps its login: `cursor-safe-storage:v1:` and base64 of `v10` and the
    AES-128-CBC cipher text, the key PBKDF2-SHA1 of the keychain's password over "saltysalt", 1003 rounds."""
    if not stored.startswith(ENCRYPTED):return None
    body = stored[len(ENCRYPTED):]
    if body.startswith('v1:'):body = body[3:]
    try:data = base64.b64decode(body.strip(), validate=True)
    except (ValueError, TypeError):return None
    if not data.startswith(b'v10'):return None
    try:clear = _crypt(False, safe_storage_key(password), data[3:])
    except OSError:return None
    try:token = clear.decode('utf-8').strip() if clear is not None else ''
    except UnicodeDecodeError:return None
    return token or None


def keychain_password():
    """The keychain's “Cursor Safe Storage” password, through the security tool (macOS asks the user the first time)."""
    try:run = subprocess.run(['/usr/bin/security', 'find-generic-password', '-s', 'Cursor Safe Storage', '-w'], capture_output=True)
    except OSError:raise CursorError(KEYCHAIN_DENIED) from None
    if run.returncode == 44:raise CursorError('钥匙串里没有 Cursor 的密钥，请打开 Cursor 重新登录')
    if run.returncode:raise CursorError(KEYCHAIN_DENIED)
    return run.stdout.rstrip(b'\r\n\t ')


def session(home, now=None, keychain=False):
    """(user ID, token, where it came from) for the first usable login, desktop app first, then the CLI. An encrypted
    desktop login is opened with the keychain's key only when the user has allowed it (keychain)."""
    now = time.time() if now is None else now;problems = [];denied = None
    for where, token in (('Cursor 桌面版', app_token(app_root(home))), ('Cursor CLI', file_token(cli_auth(home)))):
        if not token:continue
        if token.startswith(ENCRYPTED):
            if not keychain:problems.append('encrypted');continue
            try:clear = decrypt_login(token, keychain_password())
            except CursorError as exc:denied = exc;continue
            if not clear:problems.append('undecrypted');continue
            token = clear
        info = claims(token);subject = str(info.get('sub') or '').split('|')[-1]
        if not subject:problems.append('format');continue
        if not isinstance(info.get('exp'), (int, float)) or info['exp'] <= now + 60:problems.append('expired');continue
        return subject, token, where
    if 'expired' in problems:raise CursorError('Cursor 登录已过期，请打开 Cursor 重新登录')
    if denied is not None:raise denied
    if 'undecrypted' in problems:raise CursorError('钥匙串里的密钥打不开 Cursor 登录，请打开 Cursor 重新登录')
    if 'encrypted' in problems:raise CursorError(ENCRYPTED_LOGIN)
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


def plan(home, now=None, opener=None, keychain=False):
    """This billing cycle's included plan usage, as Cursor's dashboard shows it. Cursor's own words ("You've used 1%
    of your included total usage") follow used ÷ limit, so that is the share; its percent fields only stand in
    when there is no limit. Amounts are in cents."""
    from swiftbar_time import parse_iso
    user, token, where = session(home, now, keychain)
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


def account(user):
    """Which account a kept history belongs to, without keeping the account ID itself."""
    return hashlib.sha256(('cursor:' + user).encode()).hexdigest()[:16]


def refresh(store, home, start, until, now=None, opener=None, keychain=False):
    """The account's events from start to until, kept in one store that every range shares. Only what the store
    lacks is asked for: the days before it when a range reaches further back, and the time since the last read
    together with the hour before it, where requests still running and late records land. A store for another
    account, or one a read could not finish, starts over. Returns the new store."""
    user, token, where = session(home, now, keychain)
    owner = account(user);held = store if isinstance(store, dict) else {}
    low, high, kept = held.get('from'), held.get('through'), held.get('events')
    if held.get('account') != owner or held.get('complete') is not True or not isinstance(kept, list) \
            or not all(isinstance(v, (int, float)) and not isinstance(v, bool) for v in (low, high)) or low > high:
        low = high = None;kept = []
    complete = True

    def read(begin, end):
        nonlocal complete
        rows, done = events(user, token, begin, end, opener);complete = complete and done
        return [e for e in map(normalize, rows) if e]
    if low is None:
        kept, low = read(start, until), start
    else:
        # Each read runs a second past its seam and each side keeps its own events, so whether Cursor counts
        # the ends of a window in or out never loses or doubles one.
        if start < low:
            kept = [e for e in read(start, low + 1) if e['timestamp'] < low] + kept;low = start
        seam = max(low, high - OVERLAP)
        if until > seam:
            kept = [e for e in kept if e['timestamp'] < seam] + [e for e in read(seam - 1, until) if e['timestamp'] >= seam]
    return {'account': owner, 'events': sorted(kept, key=lambda e: e['timestamp']), 'from': low,
            'through': max(until, high or until), 'complete': complete, 'login': where}


def tally(found, start, end, zone, carriers=None):
    """The card's figures from the kept events. A call Cursor also reported to Magpie is in Magpie's log as well;
    carriers let Summary count it once, from Magpie, as for every other client a gateway logs."""
    import swiftbar_sources as sources
    rows, complete, where = found
    result = sources.aggregate(rows, start, end, zone, price=sources.model_cost, carriers=carriers)
    result.update(source='Cursor 账户用量记录', login=where)
    if not complete:result.update(partial=True, warning='用量记录过多，只读取了一部分')
    return result
