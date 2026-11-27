#!/usr/bin/env python3
"""The desktop load scenario: every core busy at normal priority while 40 agents stream output,
with the window driven through each path a person uses. Prints each path's main-thread stalls.

    cargo build --locked --release -p workspace-desktop -p workspace-host --bins \\
      --example many_agents --example inbox_questions
    python3 crates/workspace-desktop/examples/desktop_under_load.py <new folder>

It seeds the many-agents fixture in the folder, gives its coordinators 30 questions for the
Inbox, starts one busy process per core, opens the desktop on the store with
`WIFFLETREE_RENDER_COUNTS` set and `busy_provider.py` as its Claude, and drives it through its
automation socket. The window opens on screen; leave it uncovered, since macOS throttles a
hidden one. Coordinates assume the window's 1280x800 layout with the first project selected.
It exits with status 1 when any stall is longer than `--budget-ms`, 50 by default.
See docs/performance/desktop-under-load.md.
"""
import argparse
import base64
import json
import os
import plistlib
import re
import shutil
import socket
import subprocess
import sys
import time

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), '..', '..', '..'))
RELEASE = os.path.join(ROOT, 'target', 'release')
STALL = re.compile(r'^stall ([\d.]+)ms \(cpu ([\d.]+)ms\) ran: (.*)$')

# Window points in the 1280x800 layout.
COMPOSER = (700, 718)
INBOX_DISMISS = (1053, 593)
INBOX_PANEL = (1248, 225)
PROJECTS = [(100, 268), (100, 335), (100, 402)]


class Window:
    def __init__(self, path):
        self.path = path

    def step(self, **step):
        with socket.socket(socket.AF_UNIX) as connection:
            connection.connect(self.path)
            connection.sendall((json.dumps(step) + '\n').encode())
            response = json.loads(connection.makefile().readline())
        if not response['ok']:
            sys.exit(response['error'])
        return response['result']

    def click(self, at):
        self.step(do='click', x=at[0], y=at[1])


def run(*command):
    subprocess.run(command, check=True)


def phases(window):
    """Each path a person uses, as (name, what it does)."""
    def streaming():
        time.sleep(10)

    def type_text(text):
        # One key per step, 40 ms apart, as a fast typist's.
        for character in text:
            window.step(do='type', text=character)
            time.sleep(0.04)

    def typing():
        window.click(COMPOSER)
        time.sleep(0.3)
        type_text('The quick brown fox jumps over the lazy dog, twice over. ' * 2)
        time.sleep(1)

    def send():
        for n in range(3):
            window.click(COMPOSER)
            type_text(f'Carry on with part {n}.')
            time.sleep(0.3)
            window.step(do='key', keystroke='enter')
            time.sleep(1.5)

    def inbox():
        for _ in range(5):
            window.click(INBOX_DISMISS)
            time.sleep(1.2)
        for _ in range(3):
            window.click(INBOX_PANEL)
            time.sleep(1)
            window.click(INBOX_PANEL)
            time.sleep(1)

    def switching():
        for _ in range(3):
            for at in PROJECTS + [PROJECTS[0]]:
                window.click(at)
                time.sleep(0.6)

    return [('streaming', streaming), ('typing', typing), ('send', send),
            ('inbox', inbox), ('switching sessions', switching)]


def stalls(lines):
    found = []
    for line in lines:
        if (match := STALL.match(line)):
            found.append((float(match[1]), float(match[2]), match[3]))
    return found


def summary(name, found, seconds):
    """Prints a path's stalls, each longer than a frame, and returns the longest."""
    longest = max((s[0] for s in found), default=0)
    print(f'\n{name}: {seconds:.0f}s, {len(found)} stalls over 16 ms, '
          f'{sum(s[0] > 50 for s in found)} over 50 ms, longest {longest:.1f} ms')
    for wall, cpu, ran in sorted(found, reverse=True)[:8]:
        print(f'  {wall:6.1f} ms (cpu {cpu:5.1f} ms): {ran}')
    return longest


def app_bundle(folder, binaries):
    """A bundle around the release binaries, so LaunchServices starts the app as a person's
    click would: at normal priority, with an app's scheduling, whatever niceness this script has."""
    macos = os.path.join(folder, 'LoadTest.app', 'Contents', 'MacOS')
    os.makedirs(macos)
    for binary in ('workspace-desktop', 'wiffletree-host'):
        shutil.copy2(os.path.join(binaries, binary), macos)
    with open(os.path.join(macos, '..', 'Info.plist'), 'wb') as info:
        plistlib.dump({'CFBundleIdentifier': 'com.vyttle.wiffletree.loadtest', 'CFBundleName': 'LoadTest',
                       'CFBundleExecutable': 'workspace-desktop', 'CFBundlePackageType': 'APPL'}, info)
    return os.path.join(folder, 'LoadTest.app')


class BusyCores:
    """One busy process per core, run by launchd as a normal-priority job, since a child of this
    script would inherit its niceness: an agent's shell runs at 10."""
    def __init__(self, folder):
        self.label = 'com.vyttle.wiffletree.loadtest.busy'
        self.domain = f'gui/{os.getuid()}'
        plist = os.path.join(folder, 'busy.plist')
        burn = f'for i in $(seq {os.cpu_count()}); do yes >/dev/null & done; wait'
        with open(plist, 'wb') as written:
            plistlib.dump({'Label': self.label, 'ProgramArguments': ['/bin/sh', '-c', burn],
                           'ProcessType': 'Standard', 'Nice': 0, 'RunAtLoad': True}, written)
        run('launchctl', 'bootstrap', self.domain, plist)

    def stop(self):
        subprocess.run(['launchctl', 'bootout', f'{self.domain}/{self.label}'])


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument('folder', help='a new folder for the fixture')
    parser.add_argument('--idle-host', action='store_true', help='no busy processes, for comparison')
    parser.add_argument('--from-shell', action='store_true',
                        help="start the app as this script's child, with its niceness, as a terminal or agent would")
    parser.add_argument('--binaries', default=RELEASE, help='where workspace-desktop and wiffletree-host are')
    parser.add_argument('--warm-up', type=float, default=20)
    parser.add_argument('--budget-ms', type=float, default=50, help='the longest stall allowed')
    args = parser.parse_args()
    folder = os.path.abspath(args.folder)
    run(os.path.join(RELEASE, 'examples', 'many_agents'), 'seed', folder)
    run(os.path.join(RELEASE, 'examples', 'inbox_questions'), folder, '6')
    app = app_bundle(folder, args.binaries)

    busy = None if args.idle_host else BusyCores(folder)
    automation = os.path.join(folder, 'ui.sock')
    log_path = os.path.join(folder, 'desktop.log')
    provider = os.path.join(ROOT, 'crates', 'workspace-host', 'examples', 'busy_provider.py')
    env = {'WIFFLETREE_RENDER_COUNTS': '1', 'WORKSPACE_CLAUDE_BIN': provider}
    app_args = ['--data-dir', os.path.join(folder, 'home'), '--automation-socket', automation]
    if args.from_shell:
        with open(log_path, 'w') as log:
            subprocess.Popen([os.path.join(app, 'Contents', 'MacOS', 'workspace-desktop'), *app_args],
                             stderr=log, env=dict(os.environ, **env))
    else:
        envs = [part for name, value in env.items() for part in ('--env', f'{name}={value}')]
        run('open', '-n', *envs, '--stderr', log_path, app, '--args', *app_args)
    try:
        while not os.path.exists(automation):
            time.sleep(0.2)
        window = Window(automation)
        window.step(do='resize', width=1280, height=800)
        time.sleep(args.warm_up)
        print(f'busy cores: {0 if busy is None else os.cpu_count()}; load average {os.getloadavg()[0]:.0f}')
        longest = 0
        for name, drive in phases(window):
            with open(log_path) as log:
                log.seek(0, os.SEEK_END)
                started = time.monotonic()
                drive()
                time.sleep(0.5)
                found = stalls(log.readlines())
                longest = max(longest, summary(name, found, time.monotonic() - started))
            shot = window.step(do='capture')
            with open(os.path.join(folder, f'{name}.png'), 'wb') as image:
                image.write(base64.b64decode(shot['png']))
        print(f'\nlongest stall {longest:.1f} ms; budget {args.budget_ms:g} ms')
        if longest > args.budget_ms:
            sys.exit(1)
    finally:
        subprocess.run(['pkill', '-f', os.path.join(app, 'Contents', 'MacOS', 'workspace-desktop')])
        if busy is not None:
            busy.stop()


if __name__ == '__main__':
    main()
