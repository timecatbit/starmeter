#!/usr/bin/env python3
"""Package the Windows tray: Starmeter.exe with its engine, price tables and logos as a portable ZIP and a per-user
Inno Setup installer; then smoke-test the engine, install silently into a scratch folder, check what the installer
left, and uninstall."""
import argparse
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time
import zipfile

from build import ROOT, build_app, build_engine
from package import downloads, version

APP_ID = '{66818B0F-B59E-45CC-81AA-99315BFB1931}'
RUN_KEY = r'Software\Microsoft\Windows\CurrentVersion\Run'
WEBVIEW2_BOOTSTRAPPER = 'https://go.microsoft.com/fwlink/p/?LinkId=2124703'


def iscc():
    candidates = [shutil.which('ISCC.exe')]
    for base in (os.environ.get('ProgramFiles(x86)'), os.environ.get('ProgramFiles')):
        if base:
            candidates += [str(Path(base) / 'Inno Setup 6' / 'ISCC.exe'), str(Path(base) / 'Inno Setup 7' / 'ISCC.exe')]
    if os.environ.get('LOCALAPPDATA'):
        candidates.append(str(Path(os.environ['LOCALAPPDATA']) / 'Programs' / 'Inno Setup 6' / 'ISCC.exe'))
    found = next((path for path in candidates if path and Path(path).is_file()), None)
    if not found:
        raise SystemExit('Inno Setup 6.5 or later is needed to build the installer.')
    return found


def bounded(what, command, seconds):
    """Waits for that process only, so a hung step says what hung instead of running into the job's limit."""
    print(time.strftime('%H:%M:%S'), what, flush=True)
    subprocess.run(command, check=True, timeout=seconds)


COMMAND = """@echo off
rem The starmeter command: the engine beside this file, as the command line. Add this folder to PATH to type
rem starmeter anywhere.
"%~dp0starmeter-engine.exe" cli %*
exit /b %errorlevel%
"""


def add_command(folder):
    """bin\\starmeter.cmd beside the engine; the installer and the ZIP take the whole folder."""
    (folder / 'bin' / 'starmeter.cmd').write_text(COMMAND.replace('\n', '\r\n'), encoding='ascii', newline='')


def check_command(folder, release):
    """The starmeter command answers from where it was put."""
    done = subprocess.run([str(folder / 'bin' / 'starmeter.cmd'), '--version'], capture_output=True, text=True, timeout=60)
    if done.returncode != 0 or done.stdout.strip() != 'starmeter ' + release:
        raise SystemExit('bin\\starmeter.cmd --version answered %r (%d): %s' % (done.stdout, done.returncode, done.stderr))


def smoke(folder, scratch, program='starmeter-engine.exe'):
    """The engine beside the program, with private settings and no automatic network checks."""
    home = scratch / 'home'
    home.mkdir(parents=True, exist_ok=True)
    environment = dict(os.environ, USERPROFILE=str(home), HOME=str(home), OCX_SELF_UPDATES='0', OCX_PRICING_UPDATES='0',
                       TOKEN_MONITOR_SMOKE_TEST='1', OCX_PLUGIN_PATH=str(folder / 'starmeter'),
                       OCX_STATE_DIR=str(scratch / 'state'), OCX_CACHE_DIR=str(scratch / 'cache'))
    print(time.strftime('%H:%M:%S'), 'Engine smoke test in', folder, flush=True)
    subprocess.run([str(folder / 'bin' / program), '--worker', 'smoke-test'], env=environment, check=True, timeout=120)


def registry(name):
    import winreg
    try:
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, RUN_KEY) as key:
            return winreg.QueryValueEx(key, name)[0]
    except OSError:
        return None


def uninstall_entry():
    import winreg
    try:
        winreg.OpenKey(winreg.HKEY_CURRENT_USER, r'Software\Microsoft\Windows\CurrentVersion\Uninstall\%s_is1' % APP_ID).Close()
        return True
    except OSError:
        return False


def check_installer(setup, scratch):
    installed = scratch / 'installed'
    program = installed / 'Starmeter.exe'
    shortcut = Path(os.environ['APPDATA']) / 'Microsoft/Windows/Start Menu/Programs/Starmeter.lnk'
    bounded('Silent install', [str(setup), '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/TASKS=autostart',
                               '/DIR=%s' % installed, '/LOG=%s' % (scratch / 'setup.log')], 600)
    if not (program.is_file() and shortcut.is_file() and uninstall_entry() and (installed / 'lib/model-pricing.json').is_file()):
        raise SystemExit('The installer left out the program, its price tables, its Start menu shortcut or its uninstall entry.')
    if registry('TokenMonitor') != '"%s" --background' % program:
        raise SystemExit('The sign-in entry does not start the installed copy.')
    smoke(installed, scratch / 'installed-smoke')
    check_command(installed, version())
    bounded('Silent uninstall', [str(installed / 'unins000.exe'), '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART',
                                 '/LOG=%s' % (scratch / 'uninstall.log')], 300)
    # The uninstaller finishes from a copy of itself in the temp folder, after this process has returned.
    deadline = time.time() + 60
    while (program.exists() or shortcut.exists() or uninstall_entry()) and time.time() < deadline:
        time.sleep(0.5)
    if program.exists() or shortcut.exists() or uninstall_entry():
        raise SystemExit('Uninstalling left the program, its shortcut or its entry behind.')
    if registry('TokenMonitor') is not None:
        raise SystemExit('Uninstalling left the sign-in entry behind.')
    print(time.strftime('%H:%M:%S'), 'Installer checks passed', flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, default=ROOT / 'dist')
    parser.add_argument('--tag', default='')
    parser.add_argument('--no-install-test', action='store_true', help='Build only; skip the silent install and uninstall.')
    args = parser.parse_args()
    if os.name != 'nt':
        raise SystemExit('The Windows tray is built on Windows.')
    release = version()
    if args.tag and args.tag != 'v' + release:
        raise SystemExit('Tag %s does not match version %s.' % (args.tag, release))
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    names = dict(downloads(release))
    portable = next(name for name in names if name.endswith('-windows-x64.zip'))
    setup = next(name for name in names if name.endswith('-windows-x64-setup.exe'))
    scratch = Path(tempfile.mkdtemp(prefix='starmeter-windows-'))
    folder = scratch / 'Starmeter'
    engine = build_engine(scratch / 'engine' / 'starmeter-engine.exe')
    build_app(folder, 'app', engine=engine)
    add_command(folder)
    smoke(folder, scratch / 'smoke')
    check_command(folder, release)
    archive = output / portable
    archive.unlink(missing_ok=True)
    with zipfile.ZipFile(archive, 'w', zipfile.ZIP_DEFLATED) as bundle:
        for path in sorted(folder.rglob('*')):
            if path.is_file():
                bundle.write(path, Path('Starmeter') / path.relative_to(folder))
    print(archive)
    # Windows 10 may lack the WebView2 runtime the panel draws with; the installer starts Microsoft's bootstrapper
    # only where it is missing.
    bootstrapper = scratch / 'MicrosoftEdgeWebview2Setup.exe'
    subprocess.run(['curl', '-fsSL', '-o', str(bootstrapper), WEBVIEW2_BOOTSTRAPPER], check=True, timeout=300)
    subprocess.run([iscc(), '/DAppVersion=%s' % release, '/DAppDir=%s' % folder,
                    '/DIconFile=%s' % (ROOT / 'app/src-tauri/icons/icon.ico'), '/DWebView2=%s' % bootstrapper,
                    '/O%s' % output, '/F%s' % setup[:-len('.exe')], str(ROOT / 'windows/installer/starmeter.iss')], check=True)
    print(output / setup)
    if not args.no_install_test:
        check_installer(output / setup, scratch)
    shutil.rmtree(scratch, ignore_errors=True)


if __name__ == '__main__':
    main()
