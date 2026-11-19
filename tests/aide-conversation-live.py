#!/usr/bin/env python3
"""Verify native history listing/fork/resume without model turns or user-state writes.
Copies the supplied recording into an isolated temporary CODEX_HOME.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import queue
import subprocess
import tempfile
import threading

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('recording', type=Path)
args = parser.parse_args()
original = args.recording.read_bytes()
metadata = json.loads(original.splitlines()[0])['payload']
source_id = metadata['id']
base = Path(tempfile.mkdtemp(prefix='aide-conversation-native-', dir='/tmp')).resolve()
home = base / 'codex'
sessions = home / 'sessions' / args.recording.parent.parent.parent.name / args.recording.parent.parent.name / args.recording.parent.name
sessions.mkdir(parents=True)
source = sessions / args.recording.name
source.write_bytes(original)
workspace = base / 'new-task'
workspace.mkdir()

class Server:
    def __init__(self):
        self.log = (base / 'backend.log').open('a')
        self.process = subprocess.Popen(
            ['codex', 'app-server', '--listen', 'stdio://'],
            cwd=workspace, env={**os.environ, 'CODEX_HOME': str(home)},
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=self.log, text=True)
        self.messages = queue.Queue()
        def read():
            for line in self.process.stdout:
                self.messages.put(json.loads(line))
        threading.Thread(target=read, daemon=True).start()
        self.sequence = 0
        self.call('initialize', {'clientInfo': {'name': 'aide-native-test', 'version': '1'}, 'capabilities': {'experimentalApi': True}})
        self.process.stdin.write(json.dumps({'method': 'initialized'})+'\n')
        self.process.stdin.flush()

    def call(self, method, params):
        self.sequence += 1
        self.process.stdin.write(json.dumps({'id': self.sequence, 'method': method, 'params': params})+'\n')
        self.process.stdin.flush()
        while True:
            result = self.messages.get(timeout=45)
            if result.get('id') == self.sequence and 'method' not in result:
                if 'error' in result:
                    raise RuntimeError(f'{method}: {result["error"]}')
                return result['result']

    def close(self):
        self.process.terminate()
        try:
            self.process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            self.process.kill()
            self.process.wait()
        self.log.close()

server = Server()
try:
    listing = server.call('thread/list', {'limit': 100, 'sourceKinds': ['cli','vscode','appServer']})
    assert source_id in [t['id'] for t in listing['data']], 'Saved source missing from picker listing'
    source_thread = server.call('thread/resume', {'threadId': source_id, 'cwd': str(workspace)})['thread']
    source_before_fork = source.read_bytes()
    fork = server.call('thread/fork', {'threadId': source_id, 'cwd': str(workspace), 'developerInstructions': 'Destination task instructions.'})['thread']
    assert fork['id'] != source_id
    assert fork['cwd'] == str(workspace), (fork['cwd'], str(workspace))
    assert Path(fork['path']).is_file()
    assert fork['status']['type'] == 'idle'
    fork_read = server.call('thread/read', {'threadId': fork['id'], 'includeTurns': True})['thread']
    assert len(fork_read['turns']) == len(source_thread['turns']), (len(fork_read['turns']), len(source_thread['turns']))
    assert source.read_bytes() == source_before_fork, 'Fork modified source history'
finally:
    server.close()
server = Server()
try:
    restored = server.call('thread/resume', {'threadId': fork['id'], 'cwd': str(workspace), 'developerInstructions': 'Destination task instructions.'})['thread']
    assert restored['id'] == fork['id']
    assert restored['cwd'] == str(workspace)
    assert restored['status']['type'] == 'idle'
    restored_read = server.call('thread/read', {'threadId': fork['id'], 'includeTurns': True})['thread']
    assert len(restored_read['turns']) == len(source_thread['turns'])
finally:
    server.close()
assert hashlib.sha256(args.recording.read_bytes()).digest() == hashlib.sha256(original).digest()
print('PASS: native listing, forked history, new cwd, source preservation, restart and idle resume; no model turns.')
print('Isolated artifacts:', base)
