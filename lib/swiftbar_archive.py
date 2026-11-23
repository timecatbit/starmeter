"""Usage that outlives its logs.

Tools clean up their own records (Claude Code deletes transcripts after 30 days by default), which would
quietly shrink 全部 and every earlier day. When a log file disappears, the counters the index held for it
move to a durable archive in the state folder, and the readers add them back: one copy per request across both,
the one whose time is the most exact, the index's on a tie. Only what the indexes already keep is archived: counters, model names, times and
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
    """Archive (id, timestamp, event JSON) rows; the first copy of an event stays, unless a later one knows its time
    more exactly (its 'timing': Antigravity's app and IDE can both keep a generation, AGY-2)."""
    rows = list(rows)
    if not path or not rows:return
    conn = connect(path)
    try:
        with conn:conn.executemany("INSERT INTO events VALUES (?,?,?) ON CONFLICT(id) DO UPDATE SET ts=excluded.ts, data=excluded.data WHERE COALESCE(json_extract(excluded.data,'$.timing'),0) > COALESCE(json_extract(events.data,'$.timing'),0)", rows)
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


def merged(index, live, path, start, end):
    """The index's events (live, in the range, from index) plus the archived ones it no longer has: one copy per event
    across both and every day, the one whose time is the most exact ('timing'), the index's on a tie, and only then the
    range. A generation whose exact copy the index keeps for today isn't counted again on the day its archived copy
    guessed, and the other way round (AGY-2)."""
    live = list(live)
    if not path or not Path(path).is_file():return live
    kept = connect(path)
    try:archived = {id_: float(exact) for id_, exact in kept.execute("SELECT id, COALESCE(json_extract(data,'$.timing'),0) FROM events")}
    finally:kept.close()
    timing = lambda event: float(event.get('timing') or 0)
    out = [event for event in live if archived.get(event.get('id')) is None or archived[event['id']] <= timing(event)]
    for event in events(path, start, end):
        best = None
        if isinstance(event.get('id'), str):
            best = index.execute("SELECT MAX(COALESCE(json_extract(data,'$.timing'),0)) FROM events WHERE id=?", (event['id'],)).fetchone()[0]
        if best is None or best < timing(event):out.append(event)
    return out
