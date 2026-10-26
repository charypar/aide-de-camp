#!/usr/bin/env python3
"""Opt-in native Codex/tmux acceptance check using disposable repositories.
Uses the configured Codex account for two small turns; never edits user repositories.
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
        print('Native terminal ready; sending the test turn.', flush=True)
        prompt('Run the shell command sleep 12, then reply with exactly AIDE_LIVE_CHECK_OK. Do not modify files or use network tools.')
        wait(lambda: task()['thread_id'], timeout=90)
        identity = task()['thread_id']
        wait(lambda: tool_called('sleep 12'), timeout=90)
        keys('C-d')
        wait(lambda: tmux('display-message', '-p', '-t', managed_pane('work'), '#{pane_current_command}') == 'fish')
        wait(lambda: assistant_said('AIDE_LIVE_CHECK_OK'), timeout=180)
        print('Active native turn completed after exiting its UI.', flush=True)
        prompt(str(BINARY) + ' codex resume')
        time.sleep(4)
        wait(ready)
        print('Native conversation persisted a completed turn.', flush=True)
        if '--window' in sys.argv:
            terminal = subprocess.Popen(['alacritty', '--title', 'Aide acceptance test', '--working-directory', task()['workspace'], '-e', 'tmux', '-L', base.name, 'attach-session', '-t', session], env={k:v for k,v in env.items() if k not in ('TMUX','TMUX_PANE')}, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            try:
                wait(lambda: tmux('list-clients', '-t', session))
                terminal.terminate()
                terminal.wait(timeout=10)
                assert tmux('has-session', '-t', session) == ''
                print('Dedicated Alacritty window attached; closing it preserved the task.', flush=True)
            finally:
                if terminal.poll() is None: terminal.terminate()
    
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
