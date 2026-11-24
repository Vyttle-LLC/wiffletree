#!/usr/bin/env python3
"""A Claude double for the Assistant's conversation and its work. Only used via WORKSPACE_*_BIN
in tests.

Each turn appends its arguments, working folder, prompt and the tools its workspace server
offers to `turns.jsonl` in WORKSPACE_TEST_THREAD_DIR. It then follows its prompt line by line:
"tool <name> <json arguments>" calls that workspace tool and appends the call and its answer to
`calls.jsonl`; "use <tool name>" announces a provider tool use, such as WebFetch or
mcp__gmail__search_threads, as Claude's stream does; "sleep <seconds>" waits. It replies "done"."""
import json
import os
from pathlib import Path
import subprocess
import sys
import time
import uuid

args = sys.argv[1:]
if args[:2] == ['auth', 'status']:
    print(json.dumps({'loggedIn': True, 'email': 'fixture@example.invalid'}))
    sys.exit(0)
if '--mcp-config' not in args:
    sys.exit(0)
prompt = sys.stdin.read()
log = Path(os.environ['WORKSPACE_TEST_THREAD_DIR'])
config = json.loads(args[args.index('--mcp-config') + 1])['mcpServers']['agent_workspace']


def workspace(method, params):
    request = json.dumps({'jsonrpc': '2.0', 'id': 1, 'method': method, 'params': params})
    done = subprocess.run([config['command'], *config['args']], input=request + '\n',
                          capture_output=True, text=True, timeout=30)
    return json.loads(done.stdout)['result']


def emit(value):
    print(json.dumps(value), flush=True)


def append(name, value):
    with open(log / name, 'a') as lines:
        lines.write(json.dumps(value) + '\n')


tools = [t['name'] for t in workspace('tools/list', {})['tools']]
append('turns.jsonl', {'args': args, 'cwd': os.getcwd(), 'prompt': prompt, 'tools': tools,
                       'helper': config['args']})
session = args[args.index('--resume') + 1] if '--resume' in args else str(uuid.uuid4())
emit({'type': 'system', 'subtype': 'init', 'session_id': session,
      'mcp_servers': [{'name': 'agent_workspace', 'status': 'connected'}],
      'tools': ['Bash', 'Read', 'Write', *('mcp__agent_workspace__' + t for t in tools)]})
for line in prompt.splitlines():
    if line.startswith('tool '):
        _, name, arguments = line.split(' ', 2)
        answer = workspace('tools/call', {'name': name, 'arguments': json.loads(arguments)})
        append('calls.jsonl', {'helper': config['args'], 'name': name, 'error': answer['isError'],
                               'text': answer['content'][0]['text']})
    elif line.startswith('use '):
        emit({'type': 'assistant', 'message': {'content': [
            {'type': 'tool_use', 'id': str(uuid.uuid4()), 'name': line[4:].strip(), 'input': {}}]}})
        # The host reads the stream while the tool runs, as it would for a real one.
        time.sleep(0.5)
    elif line.startswith('sleep '):
        time.sleep(float(line[6:]))
emit({'type': 'assistant', 'message': {'content': [{'type': 'text', 'text': 'done'}]}})
emit({'type': 'result', 'is_error': False, 'result': 'done',
      'usage': {'input_tokens': 10, 'output_tokens': 5}, 'modelUsage': {}})
