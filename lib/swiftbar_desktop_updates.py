"""Read ChatGPT's own Sparkle release metadata, without login or device identifiers.

The anonymous public appcast can lag an account's rollout. Read only the staged
app Info.plist and the latest official appcast already cached by this client.
"""
from datetime import timezone
from itertools import islice
from pathlib import Path
import plistlib
import re
import sqlite3
import time
from urllib.parse import quote, urlsplit

from swiftbar_time import parse_iso

LIMIT=1024*1024
BUNDLE='com.openai.codex'


def version(value):
    if not isinstance(value,str) or not re.fullmatch(r'\d+(?:\.\d+){0,4}',value):return ()
    parts=tuple(map(int,value.split('.')))
    return parts+(0,)*(5-len(parts))


def bounded_read(path):
    with Path(path).open('rb') as stream:data=stream.read(LIMIT+1)
    if len(data)>LIMIT:raise ValueError('release metadata too large')
    return data


def cached_feed(root,parse_release,now):
    database=root/'Cache.db'
    if not database.is_file():return None
    connection=None
    try:
        connection=sqlite3.connect('file:'+quote(str(database))+'?mode=ro',uri=True,timeout=.2)
        rows=connection.execute('''SELECT r.request_key,r.time_stamp,d.isDataOnFS,d.receiver_data
            FROM cfurl_cache_response r JOIN cfurl_cache_receiver_data d USING(entry_ID)
            WHERE r.request_key LIKE 'https://persistent.oaistatic.com/codex-app-prod/appcast%'
              AND length(d.receiver_data)<=?
            ORDER BY r.time_stamp DESC LIMIT 12''',(LIMIT,))
        # Newest first: one older than a week ends the look (the rest are older still); one that gives no release (its
        # file evicted, an empty answer, a body that says no version) gives way to the one before it.
        for url,stamp,on_disk,body in rows:
            parts=urlsplit(url)
            if parts.hostname!='persistent.oaistatic.com' or parts.port not in (None,443) or parts.username or parts.password:continue
            if not re.fullmatch(r'/codex-app-prod/appcast(?:-\d+(?:\.\d+){0,4})?\.xml',parts.path):continue
            try:checked=parse_iso(stamp).replace(tzinfo=timezone.utc).timestamp()
            except (ValueError,TypeError,AttributeError):continue
            if not -60<=now-checked<=7*86400:return None
            if on_disk:
                try:name=body.decode('ascii') if isinstance(body,bytes) else body
                except UnicodeDecodeError:continue
                if not isinstance(name,str) or not re.fullmatch(r'[a-zA-Z0-9-]{1,80}',name):continue
                path=root/'fsCachedData'/name
                if path.is_symlink():continue
                try:body=bounded_read(path)
                except (OSError,ValueError):continue
            if not isinstance(body,bytes):continue
            latest,build=parse_release(body)
            if version(latest) and version(build):
                return {'latest':latest,'build':build,'releaseSource':'client-feed','observedAt':checked}
    except (OSError,ValueError,TypeError,sqlite3.Error):pass
    finally:
        if connection is not None:connection.close()
    return None


def chatgpt_release(home,bundle_id,parse_release,now=None):
    if bundle_id!=BUNDLE:return None
    now=time.time() if now is None else now
    root=Path(home)/'Library/Caches'/BUNDLE
    candidates=[]
    try:
        for path in islice((root/'org.sparkle-project.Sparkle/Installation').glob('*/*/*.app/Contents/Info.plist'),32):
            try:
                info=plistlib.loads(bounded_read(path))
                latest,build=info.get('CFBundleShortVersionString'),info.get('CFBundleVersion')
                if info.get('CFBundleIdentifier')==bundle_id and version(latest) and version(build):
                    candidates.append({'latest':latest,'build':build,'releaseSource':'client-download','observedAt':now})
            except (OSError,ValueError,TypeError,AttributeError):continue
    except OSError:pass
    try:feed=cached_feed(root,parse_release,now)
    except Exception:feed=None  # A malformed client cache must not stop other checks.
    if feed:candidates.append(feed)
    return max(candidates,key=lambda row:version(row['build'])) if candidates else None
