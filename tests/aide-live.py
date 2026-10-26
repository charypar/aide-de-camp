#!/usr/bin/env python3
"""Opt-in native Codex/tmux acceptance check using disposable repositories.
Uses the configured Codex account for two small turns (one with --detached-status);
never edits user repositories.
"""
import json
import os
from pathlib import Path
import signal
import shutil
import shlex
import sqlite3
import subprocess
import tempfile
import time
import sys

BINARY = Path(__file__).resolve().parents[1] / 'target/debug/aide'
resume_only = '--resume-only' in sys.argv
detached_status = '--detached-status' in sys.argv
base = Path(sys.argv[sys.argv.index('--resume-only') + 1]) if resume_only else Path(tempfile.mkdtemp(prefix='aide-native-', dir='/tmp'))
root = base / 'Project'
root.mkdir(exist_ok=True)
source = base / 'source'
source.mkdir(exist_ok=True)
env = dict(os.environ, AIDE_HOME=str(base / 'home'), AIDE_TMUX_SOCKET=base.name)
# Keep native configuration, but avoid an unrelated interactive update prompt.
wrapper = base / 'codex-no-update'
wrapper.write_text('#!/bin/sh\nexec ' + shlex.quote(shutil.which('codex')) + ' -c check_for_update_on_startup=false "$@"\n')
wrapper.chmod(0o755)
env['AIDE_CODEX'] = str(wrapper)
manager = None
session = None
attached_client = None

def run(*args, cwd=root):
    return subprocess.check_output([str(a) for a in args], cwd=cwd, env=env, text=True, stderr=subprocess.STDOUT).strip()

def wait(predicate, timeout=45):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            result = predicate()
        except subprocess.CalledProcessError:
            result = False
        if result:
            return result
        time.sleep(.3)
    raise RuntimeError('Timed out waiting for native acceptance condition')

def task():
    with sqlite3.connect(root / '.aide/state.sqlite') as db:
        return json.loads(db.execute('SELECT body FROM tasks').fetchone()[0])

def tmux(*args):
    return run('tmux', '-L', base.name, *args)

def managed_pane(role):
    return tmux('show-options', '-qv', '-t', session, '@aide-' + role)

def native_ui_processes():
    # pane_current_command can report the parent fish shell on macOS even when
    # Codex is running. Track the native UI process beneath the work pane instead.
    processes = []
    for line in subprocess.check_output(['ps', '-axo', 'pid,ppid,comm'], text=True).splitlines()[1:]:
        fields = line.strip().split(None, 2)
        if len(fields) == 3:
            processes.append((int(fields[0]), int(fields[1]), fields[2]))
    descendants = {int(tmux('display-message', '-p', '-t', managed_pane('work'), '#{pane_pid}'))}
    while True:
        children = {pid for pid, parent, _ in processes if parent in descendants}
        if children <= descendants:
            break
        descendants |= children
    return {pid for pid, _, name in processes if pid in descendants and 'codex' in name.lower()}

def capture():
    return tmux('capture-pane', '-p', '-t', managed_pane('work'))

def keys(*values):
    tmux('send-keys', '-t', managed_pane('work'), *values)

def prompt(text):
    tmux('send-keys', '-t', managed_pane('work'), '-l', text)
    time.sleep(.6)
    keys('Enter')

def ready():
    text = capture()
    if 'Update now' in text:
        keys('Escape')
        return False
    return 'Ask Codex' in text or 'context left' in text or 'GPT-' in text

def assistant_said(marker):
    path = task().get('native_path')
    if not path or not Path(path).is_file():
        return False
    for line in Path(path).read_text().splitlines():
        try:
            item = json.loads(line).get('payload', {})
        except ValueError:
            continue
        if (item.get('role') == 'assistant' or item.get('type') == 'agent_message') and marker in json.dumps(item):
            return True
    return False

def tool_called(command):
    path = task().get('native_path')
    if not path or not Path(path).is_file():
        return False
    for line in Path(path).read_text().splitlines():
        try:
            item = json.loads(line).get('payload', {})
        except ValueError:
            continue
        if item.get('type') in ('function_call', 'custom_tool_call') and command in str(item.get('arguments', item.get('input', ''))):
            return True
    return False

def start_manager():
    global manager
    log = open(base / 'manager.log', 'a')
    manager = subprocess.Popen([BINARY, '--no-open'], cwd=root, env=env, stdout=log, stderr=log)
    wait(lambda: (base / 'home/manager.json').exists())

def stop_manager():
    global manager
    if manager and manager.poll() is None:
        manager.send_signal(signal.SIGINT)
        manager.wait(timeout=10)
    manager = None

try:
    if not resume_only:
        run('git', 'init', '-b', 'main', cwd=source)
        (source / 'hello.txt').write_text('hello\n')
        run('git', 'add', '.', cwd=source)
        run('git', '-c', 'user.name=Test', '-c', 'user.email=test@example.com', 'commit', '-m', 'initial', cwd=source)
        run('git', 'clone', source, 'repo')
        run(BINARY, 'init', '.', '--repo', 'repo')
        run(BINARY, 'task', 'start', 'native-check', '--workspace-only')
        start_manager()
        run(BINARY, 'task', 'recover', 'native-check')
        session = 'aide-' + task()['key']
        wait(ready)
        if detached_status:
            # A real terminal client, but no GUI or changes to user sessions.
            client_log = open(base / 'attached-client.log', 'w')
            attached_client = subprocess.Popen(
                ['python3', str(BINARY.parents[2] / 'tests/aide-pty.py'), BINARY, 'task', 'attach', 'native-check'],
                cwd=root, env={**{k:v for k,v in env.items() if k not in ('TMUX', 'TMUX_PANE')}, 'TERM': 'xterm-256color'},
                stdin=subprocess.PIPE, stdout=client_log, stderr=client_log,
            )
            wait(lambda: tmux('list-clients', '-t', session))
            print('Detached-status dashboard:', json.loads((base / 'home/manager.json').read_text())['url'], flush=True)
        print('Native terminal ready; sending the test turn.', flush=True)
        prompt('Run the shell command sleep 12, then reply with exactly AIDE_LIVE_CHECK_OK. Do not modify files or use network tools.')
        wait(lambda: task()['thread_id'], timeout=90)
        identity = task()['thread_id']
        wait(lambda: tool_called('sleep 12'), timeout=90)
        if detached_status:
            wait(lambda: '\tWorking\t' in run(BINARY, 'task', 'list'))
            native_ui = native_ui_processes()
            assert native_ui, 'Expected a native Codex UI beneath the work pane'
            tmux('detach-client', '-s', session)
            attached_client.wait(timeout=10)
            assert not tmux('list-clients', '-t', session)
            print('Detached while working; waiting for completion without reattaching.', flush=True)
            wait(lambda: assistant_said('AIDE_LIVE_CHECK_OK'), timeout=180)
            wait(lambda: '\tWaiting\tConversation idle\t' in run(BINARY, 'task', 'list'), timeout=30)
            assert native_ui <= native_ui_processes(), 'Detaching stopped the native Codex UI'
            assert not tmux('list-clients', '-t', session)
            print('Detached status passed: Working -> Waiting with Codex UI alive and no attached clients. Artifacts:', base, flush=True)
            sys.exit(0)
        keys('C-d')
        wait(lambda: tmux('display-message', '-p', '-t', managed_pane('work'), '#{pane_current_command}') == 'fish')
        wait(lambda: assistant_said('AIDE_LIVE_CHECK_OK'), timeout=180)
        print('Active native turn completed after exiting its UI.', flush=True)
        prompt(str(BINARY) + ' codex resume')
        time.sleep(4)
        wait(ready)
        print('Native conversation persisted a completed turn.', flush=True)
        if '--window' in sys.argv:
            try:
                run(BINARY, 'task', 'attach', 'native-check', '--new-window')
                wait(lambda: tmux('list-clients', '-t', session))
                # Close only this test's clients; Alacritty may also own the
                # user's windows now that the launcher reuses its process.
                tmux('detach-client', '-s', session)
                wait(lambda: not tmux('list-clients', '-t', session))
                assert tmux('has-session', '-t', session) == ''
                print('Dedicated Alacritty window attached; closing it preserved the task.', flush=True)
            finally:
                tmux('detach-client', '-s', session)
    
        keys('C-d')
        wait(lambda: tmux('display-message', '-p', '-t', session, '#{pane_current_command}') == 'fish')
        backend = managed_pane('backend')
        worker_pid = tmux('display-message', '-p', '-t', backend, '#{pane_pid}')
        assert tmux('display-message', '-p', '-t', backend, '#{pane_dead}') == '0'
        prompt(str(BINARY) + ' codex resume')
        time.sleep(4)
        wait(ready)
        assert tmux('display-message', '-p', '-t', backend, '#{pane_pid}') == worker_pid
        keys('C-d')
        wait(lambda: tmux('display-message', '-p', '-t', managed_pane('work'), '#{pane_current_command}') == 'fish')
        tmux('send-keys', '-t', backend, 'C-c')
        wait(lambda: tmux('display-message', '-p', '-t', backend, '#{pane_dead}') == '1')
        prompt('set -gx AIDE_LIVE_ENV repaired; ' + str(BINARY) + ' codex resume')
        time.sleep(4)
        wait(ready)
        prompt('Run a shell command that prints only the value of AIDE_LIVE_ENV, then reply with ENV_CHECK followed by that value. Do not modify files or use network tools.')
        wait(lambda: assistant_said('ENV_CHECK') and assistant_said('repaired'), timeout=180)
        assert task()['thread_id'] == identity
        print('Fish environment repair reached resumed native backend.', flush=True)
        pane = tmux('display-message', '-p', '-t', session, '#{pane_pid}')
        stop_manager()
        assert tmux('display-message', '-p', '-t', session, '#{pane_pid}') == pane
        print('Dashboard shutdown preserved the native terminal.', flush=True)
        tmux('kill-session', '-t', session)
        time.sleep(2)
    else:
        session = 'aide-' + task()['key']
        identity = task()['thread_id']
    start_manager()
    wait(lambda: tmux('has-session', '-t', session) == '')
    wait(ready)
    wait(lambda: 'ENV_CHECK' in capture() and 'Resuming session' not in capture(), timeout=90)
    wait(lambda: task()['thread_id'] == identity)
    assert task()['recovered']
    assert 'Attention' in run(BINARY, 'task', 'list')
    assert any((base / 'home/recordings' / task()['key']).iterdir())
    print('Restart restored the same conversation, idle and requiring attention; native recording retained.', flush=True)
    print('Native acceptance passed. Artifacts:', base, flush=True)
finally:
    if attached_client and attached_client.poll() is None:
        attached_client.terminate()
        attached_client.wait(timeout=10)
    if session:
        try:
            print('Final test terminal:', capture()[-3000:], flush=True)
        except subprocess.CalledProcessError:
            pass
    stop_manager()
    if session:
        try:
            tmux('kill-server')
        except subprocess.CalledProcessError:
            pass
