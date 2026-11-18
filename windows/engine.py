"""The Swift engine as the tray's backend. Each request is one run of starmeter-engine; when it needs the user
(a confirmation, a file, a notice) it writes one JSON line and waits for the tray's one-line answer."""
import json
import os
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]


def binary():
    """The engine shipped beside the tray, or a source checkout's own build."""
    here = Path(sys.executable).parent if getattr(sys, 'frozen', False) else ROOT
    name = 'starmeter-engine.exe' if os.name == 'nt' else 'starmeter-engine'
    for candidate in (here / 'engine' / name, ROOT / 'build' / 'engine-release' / 'release' / name, ROOT / 'build' / name):
        if candidate.is_file():
            return candidate
    raise FileNotFoundError('starmeter-engine')


def hidden():
    """The engine gets a console of its own that is never shown, so the CLIs it reads run in it rather than
    flashing a window each."""
    if os.name != 'nt':
        return {}
    info = subprocess.STARTUPINFO()
    info.dwFlags |= subprocess.STARTF_USESHOWWINDOW
    info.wShowWindow = 0  # SW_HIDE
    return {'creationflags': subprocess.CREATE_NEW_CONSOLE, 'startupinfo': info}


class EngineError(RuntimeError):
    pass


def run(arguments, answer, timeout=600):
    """Runs one request; answer(message) handles each question and returns its value. The last line is the result."""
    process = subprocess.Popen([str(binary()), '--desktop', *arguments], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                               stderr=subprocess.DEVNULL, env=dict(os.environ), **hidden())
    result = None
    try:
        for line in process.stdout:
            try:
                message = json.loads(line)
            except ValueError:
                continue
            if not isinstance(message, dict):
                continue
            if 'result' in message:
                result = message['result']
                break
            try:
                value = answer(message)
            except Exception:
                value = None
            process.stdin.write((json.dumps({'value': value}) + '\n').encode('ascii'))
            process.stdin.flush()
    finally:
        try:
            process.stdin.close()
        except OSError:
            pass
        try:
            process.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()
    if isinstance(result, dict) and set(result) == {'error'}:
        raise EngineError(result['error'])
    if result is None:
        raise EngineError('no answer from the engine')
    return result
