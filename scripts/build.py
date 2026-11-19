#!/usr/bin/env python3
"""Build the Rust engine and the Tauri app. The app comes in two flavors from one source: Starmeter (the menu bar
or tray app) and, on macOS, OpenCodexPanel.app, the panel the SwiftBar plugin opens. Mac release builds carry both
architectures."""
import argparse
import json
import os
from pathlib import Path
import plistlib
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
MAC_TARGETS = ('aarch64-apple-darwin', 'x86_64-apple-darwin')
MACH_O = (b'\xcf\xfa\xed\xfe', b'\xfe\xed\xfa\xcf', b'\xca\xfe\xba\xbe', b'\xbe\xba\xfe\xca', b'\xca\xfe\xba\xbf', b'\xbf\xba\xfe\xca')
# The panel keeps 5.x's name and bundle id: installed plugins quit it by that id before replacing it.
FLAVORS = {
    'app': {'productName': 'Starmeter', 'mainBinaryName': 'Starmeter', 'identifier': 'com.timecatbit.token-monitor'},
    'panel': {'productName': 'OpenCodexPanel', 'mainBinaryName': 'OpenCodexPanel', 'identifier': 'cc.opencodex.swiftbar.panel'},
}


def cargo():
    found = shutil.which('cargo') or str(Path.home() / '.cargo' / 'bin' / 'cargo')
    if not os.path.isfile(found):
        raise SystemExit('Install Rust first: https://rustup.rs')
    return found


def npm():
    found = shutil.which('npm')
    if not found:
        raise SystemExit('Install Node.js first: https://nodejs.org')
    return found


def sign(path, identity='-'):
    """Ad hoc by default; a Developer ID signature also gets the hardened runtime and a timestamp."""
    if sys.platform != 'darwin':
        return
    options = ['--force', '--sign', identity]
    if identity != '-':
        options += ['--options', 'runtime', '--timestamp']
    if Path(path).is_dir():
        options.append('--deep')
    subprocess.run(['codesign'] + options + [str(path)], check=True, capture_output=True)


def build_engine(output, universal=False, target=None):
    """The engine every host runs: SwiftBar's entry and the command line. Universal builds run on Apple silicon
    and Intel Macs without a local compiler."""
    output = Path(output).resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    name = 'starmeter-engine.exe' if os.name == 'nt' else 'starmeter-engine'
    command = [cargo(), 'build', '--release', '--locked', '-p', 'starmeter-engine']
    environment = dict(os.environ, MACOSX_DEPLOYMENT_TARGET='13.0')
    if universal:
        if sys.platform != 'darwin':
            raise SystemExit('Universal builds are for macOS.')
        slices = []
        for triple in MAC_TARGETS:
            subprocess.run(command + ['--target', triple], cwd=ROOT, check=True, env=environment)
            slices.append(ROOT / 'target' / triple / 'release' / name)
        built = ROOT / 'target' / 'universal' / name
        built.parent.mkdir(parents=True, exist_ok=True)
        subprocess.run(['lipo', '-create'] + [str(part) for part in slices] + ['-output', str(built)], check=True)
    elif target:
        subprocess.run(command + ['--target', target], cwd=ROOT, check=True, env=environment)
        built = ROOT / 'target' / target / 'release' / name
    else:
        subprocess.run(command, cwd=ROOT, check=True, env=environment)
        built = ROOT / 'target' / 'release' / name
    # Copied beside the target, then renamed over it: rewriting a running signed binary in place would kill it.
    staged = output.with_name('.' + output.name + '.new')
    shutil.copyfile(built, staged)
    staged.chmod(0o755)
    sign(staged)
    os.replace(staged, output)
    print(output)
    return output


def resources(engine=None):
    """What the core reads beside the app: the price tables with the licenses they came with, and the logos; and the
    engine program, which the Claude status line runs and the package checks run. Paths are relative to
    app/src-tauri, where Tauri reads its configuration."""
    mapping = {}
    if engine:
        # In bin/, as in the SwiftBar plugin; Tauri can't copy a resource onto the engine cargo built beside the app.
        mapping[str(Path(engine).resolve())] = 'bin/' + Path(engine).name
    for path in sorted((ROOT / 'lib').iterdir()):
        if path.suffix == '.json' or path.name.endswith('LICENSE'):
            mapping['../../lib/' + path.name] = 'lib/' + path.name
    mapping['../../assets/'] = 'assets/'
    mapping['../../LICENSE'] = 'LICENSE'
    return mapping


def tauri_config(flavor, engine=None):
    """The flavor's names and the shipped files, laid over app/src-tauri/tauri.conf.json."""
    return dict(FLAVORS[flavor], bundle={'resources': resources(engine)})


def build_app(output, flavor='app', universal=False, target=None, bundles=None, identity='-', engine=None):
    """Tauri builds the web UI and the app. On macOS the bundle is copied to output; elsewhere output is a folder
    with the program and the files it reads, ready for the installer, and bundles (appimage, deb) may be asked for."""
    app = ROOT / 'app'
    if not (app / 'node_modules').is_dir():
        subprocess.run([npm(), 'ci'], cwd=app, check=True)
    names = FLAVORS[flavor]
    command = [npm(), 'run', 'tauri', '--', 'build', '--config', json.dumps(tauri_config(flavor, engine), ensure_ascii=False)]
    if sys.platform == 'darwin':
        command += ['--bundles', 'app']
    elif bundles:
        command += ['--bundles', ','.join(bundles)]
    else:
        command += ['--no-bundle']
    triple = 'universal-apple-darwin' if universal else target
    if triple:
        command += ['--target', triple]
    subprocess.run(command, cwd=app, check=True, env=dict(os.environ, MACOSX_DEPLOYMENT_TARGET='13.0'))
    release = ROOT / 'target' / triple / 'release' if triple else ROOT / 'target' / 'release'
    output = Path(output).resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    if output.is_dir():
        shutil.rmtree(output)
    if sys.platform == 'darwin':
        built = release / 'bundle' / 'macos' / (names['productName'] + '.app')
        # ditto merges into an existing bundle, so a file the new build no longer has would stay and break its seal.
        subprocess.run(['ditto', '--norsrc', '--noextattr', str(built), str(output)], check=True)
        info = output / 'Contents' / 'Info.plist'
        metadata = plistlib.loads(info.read_bytes())
        # A menu bar app: no Dock icon, and the panel opens over the app the user was in.
        metadata.update(LSUIElement=True, CFBundleDisplayName='Starmeter')
        if flavor == 'app':
            metadata.update(TokenMonitorStandalone=True, NSAppleEventsUsageDescription='在你点击更新 CLI 时打开终端并显示结果。')
        info.write_bytes(plistlib.dumps(metadata))
        # codesign --deep leaves programs among the resources alone; the engine is signed on its own first.
        for file in sorted((output / 'Contents' / 'Resources').rglob('*')):
            if file.is_file() and not file.is_symlink():
                with file.open('rb') as stream:
                    magic = stream.read(4)
                if magic in MACH_O:
                    sign(file, identity)
        sign(output, identity)
        subprocess.run(['codesign', '--verify', '--deep', '--strict', str(output)], check=True)
    else:
        program = names['mainBinaryName'] + ('.exe' if os.name == 'nt' else '')
        output.mkdir(parents=True)
        shutil.copy2(release / program, output / program)
        for source, target_name in resources(engine).items():
            source_path = (app / 'src-tauri' / source).resolve()
            destination = output / target_name.rstrip('/')
            if source_path.is_dir():
                shutil.copytree(source_path, destination, ignore=shutil.ignore_patterns('.DS_Store'))
            else:
                destination.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(source_path, destination)
    print(output)
    return output


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--universal', action='store_true', help='Build arm64 and x86_64 slices, as Mac release packages do.')
    parser.add_argument('--engine', type=Path, default=ROOT / 'build' / 'starmeter-engine', help='Where the engine program goes.')
    parser.add_argument('--panel', type=Path, help='Also build the SwiftBar panel (macOS) here, e.g. build/OpenCodexPanel.app.')
    parser.add_argument('--app', type=Path, help='Also build the Starmeter app here.')
    args = parser.parse_args()
    build_engine(args.engine, args.universal)
    if args.panel:
        build_app(args.panel, 'panel', args.universal)
    if args.app:
        build_app(args.app, 'app', args.universal)
