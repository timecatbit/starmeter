#!/usr/bin/env python3
"""Build and ad-hoc sign the native AppKit helper and the Swift engine on macOS."""
import argparse
import os
import hashlib
from pathlib import Path
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]

def build(output, universal=False):
    if os.uname().sysname != 'Darwin':
        raise SystemExit('The AppKit helper must be built on macOS.')
    output = Path(output).resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    cache = ROOT / 'build' / ('swift-module-cache-' + hashlib.sha256(os.fsencode(ROOT)).hexdigest()[:10])
    cache.mkdir(parents=True, exist_ok=True)
    # Sign outside File Provider folders, which may add disallowed Finder metadata.
    with tempfile.TemporaryDirectory(prefix='swiftbar-panel-') as temporary:
        app = Path(temporary) / 'OpenCodexPanel.app'
        binary = app / 'Contents' / 'MacOS' / 'OpenCodexPanel'
        binary.parent.mkdir(parents=True)
        shutil.copyfile(ROOT / 'panel' / 'Info.plist', app / 'Contents' / 'Info.plist')
        # The app icon, drawn from the logo's SVG: Finder, notifications, About and the share picture's signature use it.
        resources = app / 'Contents' / 'Resources'
        iconset = Path(temporary) / 'Starmeter.iconset'
        subprocess.run(['xcrun', 'swift', '-module-cache-path', str(cache), str(ROOT / 'scripts' / 'make_icon.swift'),
                        str(ROOT / 'assets' / 'starmeter.svg'), str(ROOT / 'assets' / 'starmeter-small.svg'), str(iconset)],
                       check=True)
        resources.mkdir()
        subprocess.run(['iconutil', '-c', 'icns', str(iconset), '-o', str(resources / 'Starmeter.icns')], check=True)
        environment = dict(os.environ, MACOSX_DEPLOYMENT_TARGET='13.0')
        command = ['xcrun', 'swiftc', '-O', '-module-cache-path', str(cache), str(ROOT / 'panel' / 'Panel.swift')]
        if universal:
            # Release packages run on Apple silicon and Intel Macs without a local compiler.
            slices = [Path(temporary) / arch for arch in ('arm64', 'x86_64')]
            for part in slices:
                subprocess.run(command + ['-target', part.name + '-apple-macos13.0', '-o', str(part)],
                               check=True, env=environment)
            subprocess.run(['lipo', '-create'] + [str(part) for part in slices] + ['-output', str(binary)], check=True)
        else:
            subprocess.run(command + ['-o', str(binary)], check=True, env=environment)
        subprocess.run(['codesign', '--force', '--sign', '-', str(app)], check=True)
        subprocess.run(['codesign', '--verify', '--deep', '--strict', str(app)], check=True)
        # ditto merges into an existing bundle, so a file the new build no longer has would stay and break its seal.
        if output.is_dir() and output.suffix == '.app':
            shutil.rmtree(output)
        subprocess.run(['ditto', '--norsrc', '--noextattr', str(app), str(output)], check=True)
    print(output)
    return output

def build_engine(output, universal=False):
    """The engine every Mac host runs: SwiftBar's entry and the standalone app's worker. Release builds carry
    both architectures, so it runs without a local compiler."""
    if os.uname().sysname != 'Darwin':
        raise SystemExit('The engine is built here on macOS; Windows builds it in its own workflow.')
    output = Path(output).resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    command = ['swift', 'build', '-c', 'release', '--package-path', str(ROOT / 'engine'), '--scratch-path', str(ROOT / 'build' / 'engine-release'),
               '--product', 'starmeter-engine']
    if universal:
        command += ['--arch', 'arm64', '--arch', 'x86_64']
    subprocess.run(command, check=True, env=dict(os.environ, MACOSX_DEPLOYMENT_TARGET='13.0'))
    folder = subprocess.run(command + ['--show-bin-path'], check=True, capture_output=True, text=True).stdout.strip()
    # Signed beside the target, then renamed over it: rewriting a running signed binary in place would kill it.
    binary = output.with_name('.' + output.name + '.new')
    shutil.copyfile(Path(folder) / 'starmeter-engine', binary)
    binary.chmod(0o755)
    subprocess.run(['codesign', '--force', '--sign', '-', str(binary)], check=True, capture_output=True)
    os.replace(binary, output)
    subprocess.run(['codesign', '--verify', '--strict', str(output)], check=True)
    print(output)
    return output

if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, default=ROOT / 'build' / 'OpenCodexPanel.app')
    parser.add_argument('--universal', action='store_true', help='Build arm64 and x86_64 slices, as release packages do.')
    parser.add_argument('--engine', type=Path, default=ROOT / 'build' / 'starmeter-engine', help='Where the engine program goes.')
    args = parser.parse_args()
    build(args.output, args.universal)
    build_engine(args.engine, args.universal)
