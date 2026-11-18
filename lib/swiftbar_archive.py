"""Usage that outlives its logs.

Tools clean up their own records (Claude Code deletes transcripts after 30 days by default), which would
quietly shrink 全部 and every earlier day. When a log file disappears, the counters the index held for it
move to a durable archive in the state folder, and the readers add them back, the index winning for any
request it still has. Only what the indexes already keep is archived: counters, model names, times and
hashed IDs, never content. A file that is rewritten in place is not archived; its new content counts.
"""
import json
import os
from pathlib import Path
import shutil
import sqlite3

FOLDER = 'archive'


def location(state, source, root_id):
    """The archive of one source read from one folder; root_id tells folders apart, as the indexes do."""
    return Path(state) / FOLDER / ('%s-v1-%s.sqlite3' % (source, root_id))


def clear(state):
    """Turning the setting off forgets everything kept so far."""
    shutil.rmtree(Path(state) / FOLDER, ignore_errors=True)


def connect(path):
    path = Path(path); path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    conn = sqlite3.connect(path, timeout=10); os.chmod(path, 0o600)
    conn.execute('CREATE TABLE IF NOT EXISTS events(id TEXT PRIMARY KEY, ts REAL, data TEXT)')
    conn.execute('CREATE INDEX IF NOT EXISTS events_ts ON events(ts)')
    return conn


def keep(path, rows):
    """Archive (id, timestamp, event JSON) rows; the first copy of an event stays."""
    rows = list(rows)
    if not path or not rows:return
    conn = connect(path)
    try:
        with conn:conn.executemany('INSERT OR IGNORE INTO events VALUES (?,?,?)', rows)
    finally:conn.close()


def events(path, start, end):
    """The archived events in start..end."""
    if not path or not Path(path).is_file():return []
    conn = connect(path)
    try:
        rows = conn.execute('SELECT data FROM events WHERE ts>=? AND ts<=?', (start, end))
        try:return [json.loads(row[0]) for row in rows]
        finally:rows.close()
    finally:conn.close()


def merged(live, path, start, end):
    """The index's events plus the archived ones it no longer has."""
    live = list(live)
    seen = {event['id'] for event in live}
    return live + [event for event in events(path, start, end) if event.get('id') not in seen]
