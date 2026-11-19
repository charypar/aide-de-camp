#!/usr/bin/env python3
"""Native picker acceptance: disposable task and copied history, no model prompts.
Uses an isolated CODEX_HOME; optionally copies local auth for native UI startup.
"""
import argparse
import json
import os
from pathlib import Path
import shlex
import shutil
import sqlite3
import subprocess
import tempfile
import time

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('recording', type=Path)
args = parser.parse_args()
binary = Path(__file__).resolve().parents[1] / 'target/debug/aide'
base = Path(tempfile.mkdtemp(prefix='aide-picker-native-', dir='/tmp')).resolve()
original = args.recording.read_bytes()
identity = json.loads(original.splitlines()[0])['payload']['id']
codex_home = base / 'codex'
sessions = codex_home / 'sessions' / args.recording.parent.parent.parent.name / args.recording.parent.parent.name / args.recording.parent.name
sessions.mkdir(parents=True)
source_recording = sessions / args.recording.name
source_recording.write_bytes(original)
auth = Path(os.environ.get('CODEX_HOME', str(Path.home() / '.codex'))) / 'auth.json'
if auth.exists():
    shutil.copyfile(auth, codex_home / 'auth.json')
    (codex_home / 'auth.json').chmod(0o600)
root = base / 'project'
root.mkdir()
source = base / 'source'
source.mkdir()
wrapper = base / 'native-codex'
wrapper.write_text('#!/bin/sh\nexec '+shlex.quote(shutil.which('codex'))+' -c check_for_update_on_startup=false "$@"\n')
wrapper.chmod(0o755)
env = {**os.environ, 'AIDE_HOME': str(base / 'home'), 'CODEX_HOME': str(codex_home),
       'AIDE_TMUX_SOCKET': base.name, 'AIDE_DESKTOP': '0', 'AIDE_CODEX': str(wrapper)}

def run(*command, cwd=root):
    return subprocess.check_output([str(c) for c in command], cwd=cwd, env=env, text=True, stderr=subprocess.STDOUT).strip()

def tmux(*command):
    return run('tmux', '-L', base.name, *command)

def wait(check, timeout=30):
    end = time.monotonic()+timeout
    while time.monotonic() < end:
        result = check()
        if result:
            return result
        time.sleep(.2)
    raise RuntimeError('Timed out; artifacts: '+str(base))

def task():
    with sqlite3.connect(root / '.aide/state.sqlite') as db:
        return json.loads(db.execute("SELECT body FROM tasks WHERE id='destination'").fetchone()[0])

manager = None
try:
    run('git','init','-b','main',cwd=source)
    (source/'file').write_text('test\n')
    run('git','add','.',cwd=source)
    run('git','-c','user.name=Test','-c','user.email=test@example.com','commit','-m','initial',cwd=source)
    run('git','clone',source,'repo')
    run(binary,'init','.','--repo','repo')
    run(binary,'task','start','destination','--workspace-only')
    (codex_home/'config.toml').write_text('[projects.'+json.dumps(task()['workspace'])+']\ntrust_level = \"trusted\"\n')
    log = (base/'manager.log').open('w')
    manager = subprocess.Popen([str(binary),'--no-open','--port','0'],cwd=root,env=env,stdout=log,stderr=log)
    wait(lambda:(base/'home/manager.json').exists())
    run(binary,'task','recover','destination')
    session = 'aide-'+task()['key']
    pane = tmux('show-options','-qv','-t',session,'@aide-work')
    def capture():
        screen=tmux('capture-pane','-p','-t',pane)
        (base/'screen.txt').write_text(screen)
        return screen
    def command(text):
        tmux('send-keys','-t',pane,'-l',text)
        tmux('send-keys','-t',pane,'Enter')
    def shell():
        return tmux('display-message','-p','-t',pane,'#{pane_current_command}')=='fish'
    wait(lambda:'OpenAI Codex' in capture() or 'Sign in' in capture(),60)
    tmux('send-keys','-t',pane,'C-c')
    time.sleep(.5)
    tmux('send-keys','-t',pane,'C-c')
    wait(shell)
    before = task()['thread_id']
    command(shlex.quote(str(binary))+' codex resume --pick')
    wait(lambda:'Fork' in capture() and 'search' in capture().lower(),60)
    assert 'Conversation number' not in capture()
    tmux('send-keys','-t',pane,'C-c')
    wait(shell)
    assert task()['thread_id'] == before
    print('Native picker cancellation preserved binding.',flush=True)
    command(shlex.quote(str(binary))+' codex resume --pick')
    wait(lambda:'Fork' in capture() and 'search' in capture().lower(),60)
    # Exercise native search and arrow navigation, not a fixture prompt.
    user_messages = []
    for line in original.splitlines():
        payload = json.loads(line).get('payload', {})
        if payload.get('type') == 'user_message':
            user_messages.append(payload['message'])
        item = payload.get('item', {})
        if item.get('type') == 'UserMessage':
            user_messages.extend(c['text'] for c in item.get('content', []) if c.get('type') == 'text')
    user_message = next(text for text in user_messages if text.strip())
    search = ' '.join(user_message.split()[:4])
    tmux('send-keys','-t',pane,'-l',search)
    wait(lambda:search in capture())
    tmux('send-keys','-t',pane,'Down','Up','Enter')
    fork = wait(lambda:task()['thread_id'] if task()['thread_id'] not in (None,before,identity) else None,60)
    print('Native arrow-key selection pinned a new fork.',flush=True)
    current=task()
    assert Path(current['native_path']).is_file()
    assert source_recording.read_bytes() == original
    time.sleep(2)
    capture()
    assert 'Conversation number' not in capture()
    tmux('send-keys','-t',pane,'C-c')
    time.sleep(.5)
    tmux('send-keys','-t',pane,'C-c')
    wait(shell)
    command(shlex.quote(str(binary))+' codex resume')
    wait(lambda:not shell())
    assert task()['thread_id'] == fork
    assert args.recording.read_bytes() == original
    print('PASS: native picker, search, cancellation, arrow keys, exact fork binding, source preservation and ordinary resume; no model prompts.')
    print('Isolated artifacts:',base)
finally:
    if manager:
        manager.terminate()
        try: manager.wait(timeout=10)
        except subprocess.TimeoutExpired: manager.kill(); manager.wait()
    try: tmux('kill-server')
    except subprocess.CalledProcessError: pass
    # The test never retains copied login credentials in its artifacts.
    (codex_home/'auth.json').unlink(missing_ok=True)
