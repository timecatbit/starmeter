#!/usr/bin/env python3
"""Build the Rust engine (macOS, Windows or Linux) and Starmeter.app (macOS); Mac release builds carry both
architectures."""
import argparse
import os
from pathlib import Path
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
MAC_TARGETS = ('aarch64-apple-darwin', 'x86_64-apple-darwin')


def cargo():
    found = shutil.which('cargo') or str(Path.home() / '.cargo' / 'bin' / 'cargo')
    if not os.path.isfile(found):
        raise SystemExit('Install Rust first: https://rustup.rs')
    return found


def build_engine(output, universal=False):
    """The engine every host runs: SwiftBar's entry, the apps' worker and the command line. Universal builds
    run on Apple silicon and Intel Macs without a local compiler."""
    output = Path(output).resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    name = 'starmeter-engine.exe' if os.name == 'nt' else 'starmeter-engine'
    command = [cargo(), 'build', '--release', '--locked', '-p', 'starmeter-engine']
    environment = dict(os.environ, MACOSX_DEPLOYMENT_TARGET='13.0')
    if universal:
        if sys.platform != 'darwin':
            raise SystemExit('Universal builds are for macOS.')
        slices = []
        for target in MAC_TARGETS:
            subprocess.run(command + ['--target', target], cwd=ROOT, check=True, env=environment)
            slices.append(ROOT / 'target' / target / 'release' / name)
        built = ROOT / 'target' / 'universal' / name
        built.parent.mkdir(parents=True, exist_ok=True)
        subprocess.run(['lipo', '-create'] + [str(part) for part in slices] + ['-output', str(built)], check=True)
    else:
        subprocess.run(command, cwd=ROOT, check=True, env=environment)
        built = ROOT / 'target' / 'release' / name
    # Copied beside the target, then renamed over it: rewriting a running signed binary in place would kill it.
    staged = output.with_name('.' + output.name + '.new')
    shutil.copyfile(built, staged)
    staged.chmod(0o755)
    if sys.platform == 'darwin':
        subprocess.run(['codesign', '--force', '--sign', '-', str(staged)], check=True, capture_output=True)
    os.replace(staged, output)
    print(output)
    return output


def npm():
    found = shutil.which('npm')
    if not found:
        raise SystemExit('Install Node.js first: https://nodejs.org')
    return found


def build(output, universal=False):
    """The panel app: Tauri builds the web UI, compiles the app and bundles Starmeter.app. Universal builds
    carry both Mac architectures."""
    if sys.platform != 'darwin':
        raise SystemExit('Starmeter.app is built on macOS; Windows and Linux packages come from their own builds.')
    app = ROOT / 'app'
    if not (app / 'node_modules').is_dir():
        subprocess.run([npm(), 'ci'], cwd=app, check=True)
    command = [npm(), 'run', 'tauri', '--', 'build', '--bundles', 'app']
    if universal:
        command += ['--target', 'universal-apple-darwin']
    subprocess.run(command, cwd=app, check=True, env=dict(os.environ, MACOSX_DEPLOYMENT_TARGET='13.0'))
    target = ROOT / 'target' / 'universal-apple-darwin' if universal else ROOT / 'target'
    built = target / 'release' / 'bundle' / 'macos' / 'Starmeter.app'
    output = Path(output).resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    # ditto merges into an existing bundle, so a file the new build no longer has would stay and break its seal.
    if output.is_dir():
        shutil.rmtree(output)
    subprocess.run(['ditto', '--norsrc', '--noextattr', str(built), str(output)], check=True)
    print(output)
    return output


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--universal', action='store_true', help='Build arm64 and x86_64 slices, as Mac release packages do.')
    parser.add_argument('--engine', type=Path, default=ROOT / 'build' / 'starmeter-engine', help='Where the engine program goes.')
    parser.add_argument('--app', type=Path, help='Also build Starmeter.app here.')
    args = parser.parse_args()
    build_engine(args.engine, args.universal)
    if args.app:
        build(args.app, args.universal)
