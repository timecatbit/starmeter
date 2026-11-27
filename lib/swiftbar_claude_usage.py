"""Incremental usage-only Claude transcript reader.

Response deduplication, advisor records and cache-duration accounting are adapted
from ccusage (MIT), commit 615bd802. See CCUSAGE_LICENSE. This collector never
reads credentials and never persists message text, paths or raw response IDs; of a
project it keeps only the folder name.
"""
import hashlib
import json
import math
import os
from pathlib import Path
import sqlite3

import swiftbar_projects
from swiftbar_time import parse_iso


def digest(*values):
    """128-bit hashed identifiers keep the index compact; raw IDs and paths are never stored."""
    return hashlib.sha256(json.dumps(values,ensure_ascii=False,separators=(',',':')).encode()).digest()[:16]


def counter(value):
    if value is None:return 0
    if type(value) is not int or not 0<=value<2**63:raise ValueError('invalid token counter')
    return value


def normalize(usage):
    if not isinstance(usage,dict):raise ValueError('invalid usage')
    breakdown=usage.get('cache_creation')
    if isinstance(breakdown,dict):
        w5=counter(breakdown.get('ephemeral_5m_input_tokens'))
        w1=counter(breakdown.get('ephemeral_1h_input_tokens'))
    else:w5,w1=counter(usage.get('cache_creation_input_tokens')),0
    return [counter(usage.get('input_tokens')),counter(usage.get('output_tokens')),
            w5,w1,counter(usage.get('cache_read_input_tokens'))]


def project_name(path):
    """The project of the folder a request ran in (see swiftbar_projects), by its name only."""
    if not isinstance(path,str):return None
    return swiftbar_projects.name_of(path)


def records(row,session_fallback,location):
    if not isinstance(row,dict) or row.get('type')!='assistant':return []
    message=row.get('message');usage=message.get('usage') if isinstance(message,dict) else None
    if not isinstance(usage,dict):return []
    model=message.get('model')
    if not isinstance(model,str) or not model or model in ('<synthetic>','synthetic'):return []
    session=row.get('sessionId') or session_fallback
    if not isinstance(session,str):return []
    timestamp=parse_iso(row.get('timestamp',''))
    if timestamp.tzinfo is None:raise ValueError('timestamp needs timezone')
    stamp=timestamp.timestamp();request=row.get('requestId');message_id=message.get('id')
    if request is not None and (not isinstance(request,str) or not request):return []
    if message_id is not None and (not isinstance(message_id,str) or not message_id):return []
    parts=[(model,usage,row.get('costUSD'))]
    iterations=usage.get('iterations')
    if isinstance(iterations,list):
        parts.extend((r['model'],r,None) for r in iterations if isinstance(r,dict) and
                     r.get('type')=='advisor_message' and isinstance(r.get('model'),str) and r['model'])
    output=[]
    for slot,(model,counts,cost) in enumerate(parts):
        try:metrics=normalize(counts)
        except ValueError:continue
        stored=cost if type(cost) in (int,float) and math.isfinite(cost) and cost>=0 else None
        if not sum(metrics) and stored is None:continue
        identifier=(message_id+(':advisor:'+str(slot) if slot else '')) if message_id else None
        # Without a request ID, gateway placeholders are scoped to session + time.
        # msg_/resp_ IDs represent one streamed response across content blocks.
        key=digest(identifier,request) if request and identifier else (
            digest(identifier,session,None if message_id.startswith(('msg_','resp_')) else stamp)
            if identifier else digest('location',location,slot))
        output.append({'key':key,'message':digest(identifier) if identifier else None,
                       'request':digest(request) if request else None,'session':digest(session),
                       'ts':stamp,'model':model,'metrics':metrics,'cost':stored,
                       'sidechain':row.get('isSidechain') is True,
                       'speed':counts.get('speed') if isinstance(counts.get('speed'),str) else None,
                       'geo':counts.get('inference_geo') if isinstance(counts.get('inference_geo'),str) else 'global',
                       'tier':counts.get('service_tier') if isinstance(counts.get('service_tier'),str) else 'standard',
                       'slot':slot,'project':project_name(row.get('cwd'))})
    return output


def deduplicate(events):
    """Keep complete responses; parent entries win over sidechain replays."""
    output=[];exact={};routes={}
    def rank(row):return (not row['sidechain'],sum(row['metrics']),row['speed'] is not None,row['cost'] is not None,-row['ts'])
    for row in events:
        found=exact.get(row['key']);route=(row['message'],row['session'])
        if found is None and row['message']:
            for index in routes.get(route,()):
                other=output[index]
                if (row['sidechain'] or other['sidechain']) and (
                    row['ts']==other['ts'] or (row['request'] is None and other['request'] is None)):
                    found=index;break
        if found is None:
            found=len(output);output.append(row)
        elif rank(row)>rank(output[found]):
            output[found]=row
        exact[row['key']]=found
        if row['message']:
            indexes=routes.setdefault(route,[])
            if found not in indexes:indexes.append(found)
    return output


COLUMNS='key,message,request,session,ts,model,i,o,w5,w1,r,cost,sidechain,speed,geo,tier,project'


def stored_event(row):
    key,message,request,session,ts,model,i,o,w5,w1,r,cost,sidechain,speed,geo,tier,project=row
    return {'key':key,'message':message,'request':request,'session':session,'ts':ts,'model':model,
            'metrics':[i,o,w5,w1,r],'cost':cost,'sidechain':bool(sidechain),'speed':speed,'geo':geo,'tier':tier,'project':project}


def isolated(conn,added,allowed):
    """Appended responses sharing no ID or route with indexed ones form their own dedupe groups."""
    fresh={(row['file'],row['offset'],row['slot']) for row in added}
    def clash(sql,args):
        return any((file,offset,slot) not in fresh and (allowed is None or session in allowed)
                   for file,offset,slot,session in conn.execute(sql,args))
    for row in added:
        if clash('SELECT file,offset,slot,session FROM everything WHERE key=?',(row['key'],)):return False
        if row['message'] and clash('SELECT file,offset,slot,session FROM everything WHERE message=? AND session=?',(row['message'],row['session'])):return False
    return True


def store_canonical(conn,rows,first=0):
    conn.executemany('INSERT INTO canonical VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?)',
        ((first+index,row['ts'],row['model'],*row['metrics'],row['cost'],row['speed'],row['geo'],row['tier'],row['session'],row.get('project')) for index,row in enumerate(rows)))


def best_of(rows):
    """Winner among rows sharing the same dedupe key: strictly higher rank wins, ties keep the first."""
    def rank(row):return (not row['sidechain'],sum(row['metrics']),row['speed'] is not None,row['cost'] is not None,-row['ts'])
    winner=rows[0]
    for row in rows[1:]:
        if rank(row)>rank(winner):winner=row
    return winner


def patch_canonical(conn,added,allowed):
    """Incremental in-place canonical update for streaming chunks sharing existing keys without route clashes."""
    fresh={(row['file'],row['offset'],row['slot']) for row in added}
    checked_routes=set()
    def route_clashes(message,session,expected_key):
        pair=(message,session)
        if pair in checked_routes:return False
        checked_routes.add(pair)
        for hit_session,hit_key in conn.execute(
                'SELECT session,key FROM everything WHERE message=? AND session=?',
                (message,session)):
            if (allowed is None or hit_session in allowed) and hit_key!=expected_key:
                return True
        return False
    for row in added:
        if row['message'] is not None and route_clashes(row['message'],row['session'],row['key']):
            return False
    keys=[];seen=set()
    for row in added:
        if row['key'] not in seen:seen.add(row['key']);keys.append(row['key'])
    def same_canonical(a,b):
        return (a['ts']==b['ts'] and a['model']==b['model'] and a['metrics']==b['metrics'] and a['cost']==b['cost']
                and a['speed']==b['speed'] and a['geo']==b['geo'] and a['tier']==b['tier'] and a['session']==b['session']
                and a['project']==b['project'])
    updates=[];updated_ids=set();fresh_only=[]
    for key in keys:
        old_rows=[];all_rows=[]
        for row in conn.execute('SELECT file,offset,slot,'+COLUMNS+' FROM everything WHERE key=? ORDER BY file,offset,slot',(key,)):
            if allowed is not None and row[6] not in allowed:continue
            rec=stored_event(row[3:])
            if (row[0],row[1],row[2]) not in fresh:old_rows.append(rec)
            all_rows.append(rec)
        for rec in all_rows:
            if rec['message'] is not None and route_clashes(rec['message'],rec['session'],key):
                return False
        if not old_rows:fresh_only.extend(all_rows);continue
        old_w,new_w=best_of(old_rows),best_of(all_rows)
        if same_canonical(old_w,new_w):continue
        candidates=conn.execute(
            'SELECT id,cost,speed,project FROM canonical WHERE ts=? AND model=? AND i=? AND o=? AND w5=? AND w1=? AND r=? AND geo=? AND tier=? AND session=?',
            (old_w['ts'],old_w['model'],*old_w['metrics'],old_w['geo'],old_w['tier'],old_w['session'])).fetchall()
        matches=[cid for cid,cost,speed,project in candidates if cost==old_w['cost'] and speed==old_w['speed'] and project==old_w['project']]
        if len(matches)!=1 or matches[0] in updated_ids:return False
        updated_ids.add(matches[0]);updates.append((matches[0],new_w))
    for cid,row in updates:
        conn.execute('UPDATE canonical SET ts=?,model=?,i=?,o=?,w5=?,w1=?,r=?,cost=?,speed=?,geo=?,tier=?,session=?,project=? WHERE id=?',
                     (row['ts'],row['model'],*row['metrics'],row['cost'],row['speed'],row['geo'],row['tier'],row['session'],row['project'],cid))
    if fresh_only:
        store_canonical(conn,deduplicate(fresh_only),conn.execute('SELECT COALESCE(MAX(id)+1,0) FROM canonical').fetchone()[0])
    return True


def read_events(files,db_path,allowed_sessions=None,start=0,end=float('inf'),archive=None):
    """Reindex only changed files; retain only normalized counters and hashed IDs. archive: a durable file that
    keeps the records of transcripts that disappear (Claude Code cleans up after 30 days); they are deduplicated
    together with the live ones, so a response that survives in another file still counts once."""
    db_path=Path(db_path);db_path.parent.mkdir(parents=True,exist_ok=True,mode=0o700)
    conn=sqlite3.connect(db_path,timeout=10);os.chmod(db_path,0o600)
    failures=0;rebuild=False;appended=[]
    try:
        conn.executescript('''CREATE TABLE IF NOT EXISTS files(path BLOB PRIMARY KEY,inode INTEGER,size INTEGER,mtime INTEGER,offset INTEGER);
          CREATE TABLE IF NOT EXISTS records(file BLOB,offset INTEGER,slot INTEGER,key BLOB,message BLOB,request BLOB,session BLOB,ts REAL,model TEXT,
            i INTEGER,o INTEGER,w5 INTEGER,w1 INTEGER,r INTEGER,cost REAL,sidechain INTEGER,speed TEXT,geo TEXT,tier TEXT,project TEXT,PRIMARY KEY(file,offset,slot));
          CREATE INDEX IF NOT EXISTS records_key ON records(key);
          CREATE INDEX IF NOT EXISTS records_route ON records(message,session);
          CREATE TABLE IF NOT EXISTS metadata(key TEXT PRIMARY KEY,value TEXT);
          CREATE TABLE IF NOT EXISTS canonical(id INTEGER PRIMARY KEY,ts REAL,model TEXT,i INTEGER,o INTEGER,w5 INTEGER,w1 INTEGER,r INTEGER,cost REAL,speed TEXT,geo TEXT,tier TEXT,session BLOB,project TEXT);
          CREATE INDEX IF NOT EXISTS canonical_ts ON canonical(ts);''')
        if archive:
            Path(archive).parent.mkdir(parents=True,exist_ok=True,mode=0o700)
            conn.execute('ATTACH DATABASE ? AS kept',(str(archive),))
            conn.execute('''CREATE TABLE IF NOT EXISTS kept.records(file BLOB,offset INTEGER,slot INTEGER,key BLOB,message BLOB,request BLOB,session BLOB,ts REAL,model TEXT,
              i INTEGER,o INTEGER,w5 INTEGER,w1 INTEGER,r INTEGER,cost REAL,sidechain INTEGER,speed TEXT,geo TEXT,tier TEXT,project TEXT,PRIMARY KEY(file,offset,slot))''')
            # Archives written before records knew their project gain the column, empty for what they hold.
            if 'project' not in [row[1] for row in conn.execute('PRAGMA kept.table_info(records)')]:conn.execute('ALTER TABLE kept.records ADD COLUMN project TEXT')
            os.chmod(archive,0o600)
            # A transcript that comes back reads as itself again.
            conn.execute('CREATE TEMP VIEW everything AS SELECT * FROM main.records UNION ALL SELECT * FROM kept.records WHERE file NOT IN (SELECT path FROM main.files)')
        else:conn.execute('CREATE TEMP VIEW everything AS SELECT * FROM main.records')
        conn.execute('BEGIN')
        paths=sorted({Path(p).resolve() for p in files});active={digest(str(p)) for p in paths}
        for (key,) in conn.execute('SELECT path FROM files').fetchall():
            if key not in active:
                rebuild=True
                if archive:conn.execute('INSERT OR IGNORE INTO kept.records SELECT * FROM main.records WHERE file=?',(key,))
                conn.execute('DELETE FROM records WHERE file=?',(key,));conn.execute('DELETE FROM files WHERE path=?',(key,))
        for path in paths:
            key=digest(str(path))
            try:
                stat=path.stat();old=conn.execute('SELECT inode,size,mtime,offset FROM files WHERE path=?',(key,)).fetchone()
                # A transcript the archive holds has come back: its records replace the archived ones.
                if archive and not old and conn.execute('SELECT 1 FROM kept.records WHERE file=? LIMIT 1',(key,)).fetchone():rebuild=True
                if old and old[:3]==(stat.st_ino,stat.st_size,stat.st_mtime_ns):continue
                incremental=bool(old and old[0]==stat.st_ino and stat.st_size>old[1])
                offset=old[3] if incremental else 0
                fallback=next((part for part in reversed(path.parts) if allowed_sessions and part in allowed_sessions),path.stem)
                # The folder Claude files the conversation under is its project, wherever it went on to work.
                project=swiftbar_projects.transcript_project(path);folder=swiftbar_projects.transcript_folder(path)
                added=[]
                conn.execute('SAVEPOINT source')
                try:
                    if not incremental:conn.execute('DELETE FROM records WHERE file=?',(key,))
                    with path.open('rb') as handle:
                        handle.seek(offset)
                        while True:
                            # Not "start": that name is the range bound used by the final query.
                            position=handle.tell();line=handle.readline(32*1024*1024)
                            if not line:break
                            if not line.endswith(b'\n'):
                                handle.seek(position);break
                            if b'"usage"' not in line or b'"assistant"' not in line:continue
                            try:
                                row=json.loads(line)
                                for event in records(row,fallback,(key.hex(),position)):
                                    if project:event['project']=project
                                    elif folder and isinstance(row.get('cwd'),str) and (name:=swiftbar_projects.within(row['cwd'],folder)):event['project']=name
                                    conn.execute('INSERT OR REPLACE INTO records VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)',
                                                 (key,position,event['slot'],event['key'],event['message'],event['request'],event['session'],event['ts'],event['model'],
                                                  *event['metrics'],event['cost'],int(event['sidechain']),event['speed'],event['geo'],event['tier'],event['project']))
                                    added.append(dict(event,file=key,offset=position))
                            except (ValueError,TypeError,OverflowError):continue
                        offset=handle.tell()
                    conn.execute('INSERT OR REPLACE INTO files VALUES (?,?,?,?,?)',(key,stat.st_ino,stat.st_size,stat.st_mtime_ns,offset))
                    conn.execute('RELEASE source')
                    # A rewritten file replaces earlier rows; a new or grown file only adds rows.
                    if old and not incremental:rebuild=True
                    appended.extend(added)
                except Exception:
                    conn.execute('ROLLBACK TO source');conn.execute('RELEASE source');raise
            except (OSError,ValueError):failures+=1
        allowed={digest(s) for s in allowed_sessions} if allowed_sessions is not None else None
        # Switching the archive on or off changes what is deduplicated, so it starts the dedupe over.
        scope=digest(sorted(allowed_sessions) if allowed_sessions is not None else None,bool(archive)).hex()
        old_scope=conn.execute("SELECT value FROM metadata WHERE key='dedupe-scope'").fetchone()
        added=sorted((row for row in appended if allowed is None or row['session'] in allowed),key=lambda row:(row['file'],row['offset'],row['slot']))
        is_iso=bool(added) and isolated(conn,added,allowed)
        if rebuild or not old_scope or old_scope[0]!=scope or (added and not is_iso and not patch_canonical(conn,added,allowed)):
            # Deduplicate before date filtering, once per file/ownership change.
            rows=conn.execute('SELECT '+COLUMNS+' FROM everything ORDER BY file,offset,slot')
            # Close the streaming cursor even when deduplication fails: an open read would keep
            # a shared lock that blocks the next run's commit.
            try:unique=deduplicate(stored_event(row) for row in rows if allowed is None or row[3] in allowed)
            finally:rows.close()
            conn.execute('DELETE FROM canonical')
            store_canonical(conn,unique)
            conn.execute("INSERT OR REPLACE INTO metadata VALUES ('dedupe-scope',?)",(scope,))
        elif added and is_iso:
            store_canonical(conn,deduplicate(added),conn.execute('SELECT COALESCE(MAX(id)+1,0) FROM canonical').fetchone()[0])
        conn.commit()
        events=[{'ts':ts,'model':model,'metrics':[i,o,w5,w1,r],'cost':cost,'speed':speed,'geo':geo,'tier':tier,'session':session,'project':project}
                for ts,model,i,o,w5,w1,r,cost,speed,geo,tier,session,project in conn.execute(
                    'SELECT ts,model,i,o,w5,w1,r,cost,speed,geo,tier,session,project FROM canonical WHERE ts>=? AND ts<=?',(start,end))]
        return events,failures
    except BaseException:
        # Python 3.11+ no longer resets open cursors on close, so an interrupted reindex would
        # keep its write lock until the cursor is collected. Roll back explicitly instead.
        conn.rollback();raise
    finally:conn.close()
