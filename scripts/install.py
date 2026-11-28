#!/usr/bin/env python3
"""Install this plugin into a selected SwiftBar directory, with a local backup. Standard library only and Python
3.9 compatible: an installed 5.x plugin runs this file from the release zip when it updates itself."""
import argparse
import ctypes
from datetime import datetime
import errno
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import uuid

ENTRY = 'starmeter.30s.sh'
# Earlier releases: the old entry names (OpenCodex, then Token Monitor) and a visible assets folder,
# which SwiftBar ran as plugins.
LEGACY = [Path('opencodex.30s.sh'), Path('token-monitor.30s.sh'), Path('assets/deepseek-logo.svg'), Path('assets/deepseek-logo.png'), Path('assets/logos')]
LEGACY_DATA = ('auto-update.json', 'updates-notified.json')
HERE = Path(__file__).resolve().parent
BACKUPS = Path.home() / 'Library/Application Support/SwiftBarToken/backups'
# Each install backs up the version it replaces under a folder named for when it ran; the newest few are kept.
BACKUP_NAME = re.compile(r'\d{8}-\d{6}-[0-9a-f]{6}')
KEEP_BACKUPS = 5
# A source checkout keeps this script in scripts/; a release package keeps it beside the entry.
ROOT = HERE if (HERE / ENTRY).is_file() else HERE.parent
ENGINE = Path('build/starmeter-engine')
# Parts earlier versions installed that the engine replaced: the Python library and its HTTP helper.
RETIRED = [Path('.opencodex-panel/python')]
STATE = Path.home() / 'Library/Application Support/SwiftBarToken'

def data_files(folder, names):
    """lib/ ships only what the engine reads: the price tables and the licenses of the code they came with."""
    return [name for name in names if not (name.endswith('.json') or name.endswith('LICENSE'))]

def migrate_config(destination, state, explicit=None):
    """An existing user file always wins, whatever it holds; checkout config is never imported silently. The copy is
    read and written in full beside the target before it takes the target's name, so a read or write that fails
    leaves nothing there and the next install migrates again. The name is taken by an operation that never replaces
    a file; when the volume has none, the legacy file stays where it was and the install reports it."""
    target = Path(state) / 'config.local.sh'
    legacy = Path(destination) / '.opencodex-panel/config.local.sh'
    source = Path(explicit).expanduser() if explicit else legacy
    if explicit and target.exists():
        raise ValueError('本机配置已存在；请在连接设置中修改，不覆盖现有文件。')
    if not target.exists() and source.is_file():
        contents = source.read_bytes()
        target.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
        fd, temporary = tempfile.mkstemp(prefix='.config-', dir=str(target.parent))
        try:
            with os.fdopen(fd, 'wb') as out:
                out.write(contents)
                out.flush()
                os.fsync(out.fileno())
            os.chmod(temporary, 0o600)
            publish_config(temporary, target)
        finally:
            if os.path.exists(temporary): os.unlink(temporary)
    # The installer already backed up the legacy file before this migration.
    if legacy.exists(): legacy.unlink()
    return target

def publish_config(temporary, target):
    """Give the finished copy the target's name in one step that never replaces a file: a hard link, or on a volume
    without hard links the system's exclusive rename. A file that appeared there meanwhile wins either way, with no
    window between a check and the rename. A volume with neither operation leaves the copy unpublished: the caller
    keeps the legacy file and the next install migrates again."""
    try:
        os.link(temporary, target)
        return
    except FileExistsError:
        return
    except OSError:
        pass
    try:
        rename_exclusive(temporary, target)
    except FileExistsError:
        pass
    except OSError as error:
        if error.errno not in NO_EXCLUSIVE_RENAME: raise
        raise OSError(error.errno, '本机配置没有迁移：这个卷既不支持硬链接，也不支持不覆盖已有文件的改名，复制件无法独占发布。'
                      '旧配置原样保留，重新安装会再次迁移。') from error

# The system's rename that refuses an existing target: macOS renamex_np with RENAME_EXCL, Linux renameat2 with
# RENAME_NOREPLACE; and the errors a system or volume without it gives.
RENAME_EXCL = 0x4
RENAME_NOREPLACE = 0x1
AT_FDCWD = -100
NO_EXCLUSIVE_RENAME = {errno.ENOTSUP, errno.EOPNOTSUPP, errno.EINVAL, errno.ENOSYS}

def rename_exclusive(source, target):
    """Rename source to target, refusing with FileExistsError when target exists: the check and the rename are one
    operation in the kernel. OSError with an errno in NO_EXCLUSIVE_RENAME when this system or volume has no such rename."""
    if sys.platform == 'darwin':
        name = 'renamex_np'
        signature = (ctypes.c_char_p, ctypes.c_char_p, ctypes.c_uint)
        arguments = (os.fsencode(source), os.fsencode(target), RENAME_EXCL)
    elif sys.platform.startswith('linux'):
        name = 'renameat2'
        signature = (ctypes.c_int, ctypes.c_char_p, ctypes.c_int, ctypes.c_char_p, ctypes.c_uint)
        arguments = (AT_FDCWD, os.fsencode(source), AT_FDCWD, os.fsencode(target), RENAME_NOREPLACE)
    else:
        raise OSError(errno.ENOTSUP, 'no exclusive rename on ' + sys.platform)
    try:
        rename = getattr(ctypes.CDLL(None, use_errno=True), name)
    except (AttributeError, OSError):
        raise OSError(errno.ENOSYS, name + ' is not available on this system') from None
    rename.argtypes, rename.restype = signature, ctypes.c_int
    if rename(*arguments) != 0:
        code = ctypes.get_errno()
        raise OSError(code, os.strerror(code), str(source), None, str(target))

def copy(source, target):
    target.parent.mkdir(parents=True, exist_ok=True)
    if source.is_dir():
        subprocess.run(['ditto', '--norsrc', '--noextattr', str(source), str(target)], check=True)
    else:
        shutil.copy2(source, target)

def stage(destination):
    """Build every part the install replaces in a scratch folder beside the installed ones and check it,
    so a bad helper or a failed copy leaves the running plugin untouched."""
    staging = destination / '.opencodex-panel' / ('.install-' + uuid.uuid4().hex[:8])
    staging.mkdir(parents=True)
    try:
        copy(ROOT / 'build/OpenCodexPanel.app', staging / 'OpenCodexPanel.app')
        subprocess.run(['codesign', '--verify', '--deep', str(staging / 'OpenCodexPanel.app')], check=True)
        copy(ROOT / 'assets', staging / 'assets')
        shutil.copytree(ROOT / 'lib', staging / 'lib', ignore=data_files)
        (staging / 'bin').mkdir()
        shutil.copy2(ROOT / ENGINE, staging / 'bin' / ENGINE.name)
        subprocess.run(['codesign', '--verify', '--strict', str(staging / 'bin' / ENGINE.name)], check=True)
    except BaseException:
        shutil.rmtree(staging, ignore_errors=True)
        raise
    return staging

def back_up(destination, previous, root=BACKUPS):
    """Copy the parts this install replaces, then drop the older install backups."""
    backup = root / (datetime.now().strftime('%Y%m%d-%H%M%S') + '-' + uuid.uuid4().hex[:6])
    backup.mkdir(parents=True, mode=0o700)
    for relative in previous:
        copy(destination / relative, backup / relative)
    removed = prune_backups(root, backup.name)
    if removed: print('Removed %d older backup%s' % (len(removed), '' if len(removed) == 1 else 's'))
    return backup

def prune_backups(root, current, keep=KEEP_BACKUPS):
    """Keep the backup just written and the newest others, up to keep in all. Only folders named like an
    install backup go; other folders here (removed plugins, cleaned-up configuration) and links stay.
    A backup that can't be removed is left for the next install, which never fails over it."""
    try:
        others = sorted(entry.name for entry in os.scandir(root)
                        if entry.name != current and BACKUP_NAME.fullmatch(entry.name) and entry.is_dir(follow_symlinks=False))
    except OSError:
        return []
    removed = []
    for name in others[:max(0, len(others) - (keep - 1))]:
        try: shutil.rmtree(root / name)
        except OSError: continue
        removed.append(name)
    return removed

def remove(path):
    if path.is_dir() and not path.is_symlink(): shutil.rmtree(path)
    elif path.exists() or path.is_symlink(): path.unlink()

def swap(pairs, keep, moved):
    """Rename each staged part into place, keeping the part it replaces under keep; moved records what to undo."""
    for n, (new, target) in enumerate(pairs):
        old = keep / ('replaced-%d' % n)
        if target.exists() or target.is_symlink(): os.replace(target, old)
        else: old = None
        moved.append((target, old))
        target.parent.mkdir(parents=True, exist_ok=True)
        os.replace(new, target)

def restore(moved):
    """Put every replaced part back; a part that was new is removed."""
    for target, old in reversed(moved):
        remove(target)
        if old is not None: os.replace(old, target)

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--plugin-dir', type=Path, help='Defaults to the folder configured in SwiftBar.')
    parser.add_argument('--no-refresh', action='store_true', help='Copy files without closing the panel or refreshing SwiftBar.')
    parser.add_argument('--skip-build', action='store_true', help='Install an already built helper.')
    parser.add_argument('--config', type=Path, help='Explicit first-install config import; existing user settings are never overwritten.')
    args = parser.parse_args()
    if args.plugin_dir is None:
        detected = subprocess.run(['defaults', 'read', 'com.ameba.SwiftBar', 'PluginDirectory'], capture_output=True, text=True)
        if detected.returncode or not detected.stdout.strip():
            parser.error('Set a plugin folder in SwiftBar first, or pass --plugin-dir PATH.')
        args.plugin_dir = Path(detected.stdout.strip())
    destination = args.plugin_dir.expanduser().resolve()
    if destination == ROOT:
        parser.error('Use the SwiftBar runtime directory, not the source directory.')
    if args.config and (Path.home() / 'Library/Application Support/SwiftBarToken/config.local.sh').exists():
        parser.error('User config already exists; edit it through connection settings.')
    if args.config and not args.config.expanduser().is_file():
        parser.error('--config must name an existing file.')
    # Release packages ship the prebuilt panel and engine and no source, so they need no compiler. A source
    # checkout builds both: the Rust engine and the Tauri panel.
    if not args.skip_build and (ROOT / 'Cargo.toml').is_file():
        subprocess.run([sys.executable, str(ROOT / 'scripts' / 'build.py'), '--panel', str(ROOT / 'build/OpenCodexPanel.app')], check=True)
    if not (ROOT / 'build/OpenCodexPanel.app').is_dir() or not (ROOT / ENGINE).is_file():
        parser.error('The panel helper or engine is missing; download the release zip again or build from source.')
    relative_app = Path('.opencodex-panel/OpenCodexPanel.app')
    local_config = Path('.opencodex-panel/config.local.sh')
    relative_assets = Path('.opencodex-panel/assets')
    relative_engine = Path('.opencodex-panel/bin')
    paths = [Path(ENTRY), relative_app, relative_assets, local_config, Path('.opencodex-panel/lib'), relative_engine] + RETIRED + LEGACY
    previous = [p for p in paths if (destination / p).exists()]
    if previous:
        print('Backup:', back_up(destination, previous))
    staging = stage(destination)
    # Only close an already running helper. Never launch SwiftBar via AppleScript.
    # Tauri's ExitRequested handler prevents AppleScript `quit` from exiting the tray helper, so follow up with
    # pkill on the panel binary path so upgrades never leave an old OpenCodexPanel serving stale UI in memory.
    if not args.no_refresh:
        subprocess.run(['osascript', '-e',
                    'if application id "cc.opencodex.swiftbar.panel" is running then tell application id "cc.opencodex.swiftbar.panel" to quit'],
                   capture_output=True, timeout=8, check=False)
        subprocess.run(['pkill', '-f', '.opencodex-panel/OpenCodexPanel.app/Contents/MacOS/OpenCodexPanel'],
                   capture_output=True, timeout=5, check=False)
    # Each part goes in by rename. If any step up to publishing the entry fails, every part is put back,
    # so the entry never runs beside a half-installed panel, library or helper.
    moved = []
    try:
        swap([(staging / 'OpenCodexPanel.app', destination / relative_app), (staging / 'assets', destination / relative_assets),
              (staging / 'lib', destination / '.opencodex-panel/lib'), (staging / 'bin', destination / relative_engine)],
             staging, moved)
        migrate_config(destination, STATE, args.config)
        state = STATE
        # Update reminders used to live in SwiftBar's per-plugin data folder, which follows the file name.
        old_data = Path.home() / 'Library/Application Support/SwiftBar/Plugins/opencodex.30s.sh'
        for name in LEGACY_DATA:
            if (old_data / name).is_file() and not (state / name).exists():
                state.mkdir(parents=True, exist_ok=True, mode=0o700); shutil.copy2(old_data / name, state / name)
        # Publish the script last; other plugins and runtime state are preserved.
        temporary = destination / '.opencodex-install.tmp'
        shutil.copyfile(ROOT / ENTRY, temporary)
        temporary.chmod(0o755)
        os.replace(temporary, destination / ENTRY)
    except BaseException:
        restore(moved)
        print('Install failed; the previous version is back in place.', file=sys.stderr)
        raise
    finally:
        shutil.rmtree(staging, ignore_errors=True)
    # One plugin only: the old entry, visible assets and retired parts are in the backup above.
    for relative in LEGACY + RETIRED:
        target = destination / relative
        if target.is_dir() and not target.is_symlink(): shutil.rmtree(target)
        elif target.exists() or target.is_symlink(): target.unlink()
    try: (destination / 'assets').rmdir()
    except OSError: pass
    # A Claude status line connected earlier now runs from this engine.
    subprocess.run(['/bin/bash', '-p', str(destination / ENTRY), '--refresh-claude-bridge'], capture_output=True, timeout=30)
    detected = subprocess.run(['/bin/bash', '-p', str(destination / ENTRY), '--initialize-modules'],
                              capture_output=True, text=True, timeout=30)
    if detected.returncode:
        print('Module detection could not finish; choose modules from the right-click menu.')
    else:
        print(detected.stdout.strip())
    if not args.no_refresh:
        subprocess.run(['open', '-g', 'swiftbar://refreshplugin?name=' + ENTRY.split('.')[0]], check=True)
    print('Installed:', destination / ENTRY)

if __name__ == '__main__':
    main()
