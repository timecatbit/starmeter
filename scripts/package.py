#!/usr/bin/env python3
"""Package a release zip with a prebuilt panel and engine, so users install without git or a Rust toolchain; and the
starmeter command for Macs on its own, as Homebrew's starmeter-cli installs it."""
import argparse
import io
from pathlib import Path
import re
import shutil
import subprocess
import tarfile
import tempfile
import time
from urllib.parse import quote

import json

from build import build_app, build_engine

ROOT = Path(__file__).resolve().parents[1]
ENTRY = 'starmeter.30s.sh'
REPOSITORY = 'https://github.com/timecatbit/starmeter'
# The installer runs from the package root with the prebuilt panel and engine beside it; lib/ keeps only data.
CONTENTS = [ENTRY, 'lib', 'assets', 'config.example.sh', 'README.md', 'CHANGELOG.md', 'LICENSE', 'scripts/install.py']
APP = Path('build/OpenCodexPanel.app')
ENGINE = Path('build/starmeter-engine')


def version():
    """One version everywhere it is written. The Python reference in lib/ keeps 5.x's number; it isn't shipped."""
    engine = re.search(r'^version = "([^"]+)"$', (ROOT / 'Cargo.toml').read_text(encoding='utf-8'), re.M)
    entry = re.search(r'<xbar.version>([^<]+)</xbar.version>', (ROOT / ENTRY).read_text(encoding='utf-8'))
    app = json.loads((ROOT / 'app/src-tauri/tauri.conf.json').read_text(encoding='utf-8')).get('version')
    ui = json.loads((ROOT / 'app/package.json').read_text(encoding='utf-8')).get('version')
    if not engine or not entry or len({engine.group(1), entry.group(1), app, ui}) != 1:
        raise SystemExit('The workspace version in Cargo.toml, version in app/src-tauri/tauri.conf.json and app/package.json '
                         'and xbar.version in %s must match.' % ENTRY)
    return engine.group(1)


def downloads(number):
    """What a release offers, in the order GitHub lists it (by file name): file and the English label the page
    shows instead of the name. In-app updates look these names up; the SwiftBar plugin's is the short one."""
    name = 'starmeter-' + number
    return [
        (name + '-linux-x64-cli.tar.gz', 'Linux · Command line'),
        (name + '-linux-x64.AppImage', 'Linux · AppImage'),
        (name + '-linux-x64.deb', 'Linux · Debian/Ubuntu'),
        (name + '-macos-apple-silicon.dmg', 'Mac · Apple Silicon'),
        (name + '-macos-cli.tar.gz', 'Mac · Command line'),
        (name + '-macos-intel.dmg', 'Mac · Intel'),
        (name + '-windows-x64-setup.exe', 'Windows · Installer'),
        (name + '-windows-x64.zip', 'Windows · Portable'),
        (name + '.zip', 'Mac · SwiftBar plugin'),
    ]


def release_notes(number):
    """Only what someone downloading needs. The changes live in CHANGELOG.md and the full download guide in
    the README; the release links to both, pinned to this tag, instead of repeating them."""
    if not re.search(r'^## %s$' % re.escape(number), (ROOT / 'CHANGELOG.md').read_text(), re.M):
        raise SystemExit('CHANGELOG.md has no section for ' + number)
    tree = '%s/blob/v%s/' % (REPOSITORY, number)
    # GitHub drops the dots from a heading's anchor: "## 5.23.0" becomes #5230.
    return ('在下方附件中选一个文件：Mac 按芯片选 **Mac · Apple Silicon** 或 **Mac · Intel**，Windows 推荐 **Windows · Installer**，'
            '已经在用 SwiftBar 的选 **Mac · SwiftBar plugin**，Linux 选 **Linux · Debian/Ubuntu** 或任意发行版都能用的 **Linux · AppImage**；'
            '只要终端里的 `starmeter` 命令就用 `brew install timecatbit/tap/starmeter-cli` 或 **Command line** 压缩包。'
            '怎么选、怎么安装见 [下载与安装](%sREADME.md#%s)。\n\n'
            'Mac 独立版预览未做 Apple 公证，首次打开被拦时到「系统设置 → 隐私与安全性 → 仍要打开」。\n\n'
            '本版改动见 [更新记录](%sCHANGELOG.md#%s)。\n'
            % (tree, quote('下载与安装'), tree, number.replace('.', '')))


def command_line_archive(engine, output, name):
    """The starmeter command on its own, for Homebrew and for copying anywhere: the engine under the command's name,
    the price tables with their licenses beside it in lib/, and the license, in a folder named like the archive."""
    folder = name[:-len('.tar.gz')]
    archive = Path(output) / name
    archive.unlink(missing_ok=True)

    def add(bundle, data, path, mode):
        entry = tarfile.TarInfo('%s/%s' % (folder, path))
        entry.size, entry.mode, entry.uname, entry.gname, entry.mtime = len(data), mode, 'root', 'root', int(time.time())
        bundle.addfile(entry, io.BytesIO(data))

    with tarfile.open(archive, 'w:gz') as bundle:
        add(bundle, Path(engine).read_bytes(), 'starmeter', 0o755)
        for path in sorted((ROOT / 'lib').iterdir()):
            if path.suffix == '.json' or path.name.endswith('LICENSE'):
                add(bundle, path.read_bytes(), 'lib/' + path.name, 0o644)
        add(bundle, (ROOT / 'LICENSE').read_bytes(), 'LICENSE', 0o644)
    print(archive)
    return archive


def verify_command_line(archive, universal=False):
    """Unpack the command's archive and run what is inside, as Homebrew installs it."""
    from check import command_line
    with tempfile.TemporaryDirectory(prefix='starmeter-cli-verify-') as scratch:
        with tarfile.open(archive) as bundle:
            # Our own archive; newer Pythons want the safe filter named, 3.9 has none.
            bundle.extractall(scratch, **({'filter': 'data'} if hasattr(tarfile, 'data_filter') else {}))
        folder = Path(scratch) / Path(archive).name[:-len('.tar.gz')]
        program = folder / 'starmeter'
        for relative in ('starmeter', 'lib/codex-pricing.json', 'lib/model-pricing.json', 'lib/CCUSAGE_LICENSE', 'LICENSE'):
            if not (folder / relative).is_file():
                raise SystemExit('%s is missing %s' % (Path(archive).name, relative))
        if universal:
            subprocess.run(['codesign', '--verify', '--strict', str(program)], check=True)
            architectures = subprocess.run(['lipo', '-archs', str(program)], capture_output=True, text=True, check=True).stdout.split()
            if sorted(architectures) != ['arm64', 'x86_64']:
                raise SystemExit('The command line is not universal: ' + ' '.join(architectures))
        command_line(program)


def verify(archive, name):
    """Unpack like a user would and check the files and signature the installer relies on."""
    with tempfile.TemporaryDirectory(prefix='starmeter-verify-') as scratch:
        subprocess.run(['ditto', '-x', '-k', str(archive), scratch], check=True)
        package = Path(scratch) / name
        for relative in ['install.py', ENTRY, 'lib/codex-pricing.json', 'lib/model-pricing.json', 'lib/CCUSAGE_LICENSE',
                         'assets/logos/LICENSE', str(APP / 'Contents/Info.plist'), str(ENGINE)]:
            if not (package / relative).is_file():
                raise SystemExit('Package is missing ' + relative)
        if (list(package.rglob('__pycache__')) or list(package.rglob('.DS_Store')) or list(package.rglob('*.py')) != [package / 'install.py']
                or (package / 'panel').exists() or (package / 'engine').exists()):
            raise SystemExit('Package contains development files.')
        subprocess.run(['codesign', '--verify', '--strict', str(package / ENGINE)], check=True)
        engine = subprocess.run(['lipo', '-archs', str(package / ENGINE)], capture_output=True, text=True, check=True).stdout.split()
        if sorted(engine) != ['arm64', 'x86_64']:
            raise SystemExit('Engine binary is not universal: ' + ' '.join(engine))
        subprocess.run(['codesign', '--verify', '--deep', '--strict', str(package / APP)], check=True)
        architectures = subprocess.run(['lipo', '-archs', str(package / APP / 'Contents/MacOS/OpenCodexPanel')],
                                       capture_output=True, text=True, check=True).stdout.split()
        if sorted(architectures) != ['arm64', 'x86_64']:
            raise SystemExit('Panel binary is not universal: ' + ' '.join(architectures))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--tag', help='Fail unless the tag names this version, for example v5.12.0.')
    parser.add_argument('--output', type=Path, default=ROOT / 'dist')
    args = parser.parse_args()
    number = version()
    if args.tag and args.tag != 'v' + number:
        raise SystemExit('Tag %s does not match version %s.' % (args.tag, number))
    # A tagged release needs its CHANGELOG section; a trial package doesn't.
    notes = release_notes(number) if args.tag else None
    name = 'starmeter-' + number
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    archive = output / (name + '.zip')
    with tempfile.TemporaryDirectory(prefix='starmeter-release-') as scratch:
        package = Path(scratch) / name
        package.mkdir()
        for relative in CONTENTS:
            source, target = ROOT / relative, package / Path(relative).name
            if relative == 'lib':
                shutil.copytree(source, target, ignore=lambda folder, names: [n for n in names if not (n.endswith('.json') or n.endswith('LICENSE'))])
            elif source.is_dir():
                shutil.copytree(source, target, ignore=shutil.ignore_patterns('__pycache__', '*.pyc', '.DS_Store'))
            else:
                shutil.copy2(source, target)
        build_app(package / APP, 'panel', universal=True)
        build_engine(package / ENGINE, universal=True)
        cli = command_line_archive(package / ENGINE, output, name + '-macos-cli.tar.gz')
        # ditto keeps the bundle's signature and executable bits; no AppleDouble or quarantine data.
        archive.unlink(missing_ok=True)
        subprocess.run(['ditto', '-c', '-k', '--keepParent', '--norsrc', '--noextattr', '--noacl',
                        str(package), str(archive)], check=True)
    verify(archive, name)
    verify_command_line(cli, universal=True)
    if notes:
        (output / 'release-notes.md').write_text(notes)
    # What the publish step uploads, as gh's file#label arguments.
    (output / 'release-assets.txt').write_text(''.join('%s#%s\n' % (file, label) for file, label in downloads(number)))
    print(archive)


if __name__ == '__main__':
    main()
