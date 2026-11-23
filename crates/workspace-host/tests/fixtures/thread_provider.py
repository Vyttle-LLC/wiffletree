#!/usr/bin/env python3
"""A Claude and Codex double for assistant threads. Only used via WORKSPACE_*_BIN in tests.

Each Claude turn appends its arguments, working folder and prompt to `turns.jsonl` in
WORKSPACE_TEST_THREAD_DIR. It lists the tools its workspace server offers, then follows the
prompt: "save <file> as <title>" writes the file in the working folder and saves it with
save_output; "offer github" reports an MCP server no source lists. It replies with the prompt's
first line. A Codex turn, `exec`, reports no inventory at its start, as Codex does: it calls
the tool the prompt names, as "call <server> <tool>", then replies."""
import json
import os
from pathlib import Path
import subprocess
import sys
import uuid

args = sys.argv[1:]
if args[:2] == ['auth', 'status']:
    print(json.dumps({'loggedIn': True, 'email': 'fixture@example.invalid'}))
    sys.exit(0)
if args[:2] == ['login', 'status']:
    # Codex prints its sign-in status on stderr.
    print('Logged in using ChatGPT', file=sys.stderr)
    sys.exit(0)
if args[:1] == ['exec']:
    prompt = sys.stdin.read()
    _, server, tool = prompt.split()[:3]
    for value in [
        {'type': 'thread.started', 'thread_id': str(uuid.uuid4())},
        {'type': 'turn.started'},
        {'type': 'item.started', 'item': {'id': 'item_0', 'type': 'mcp_tool_call', 'server': server,
                                          'tool': tool, 'status': 'in_progress'}},
        {'type': 'item.completed', 'item': {'id': 'item_1', 'type': 'agent_message', 'text': 'Called it'}},
        {'type': 'turn.completed', 'usage': {'input_tokens': 10, 'output_tokens': 5}},
    ]:
        print(json.dumps(value), flush=True)
    sys.exit(0)
if '--mcp-config' not in args:
    sys.exit(0)
prompt = sys.stdin.read()
log = Path(os.environ['WORKSPACE_TEST_THREAD_DIR'])
config = json.loads(args[args.index('--mcp-config') + 1])['mcpServers']['agent_workspace']


def workspace(method, params):
    request = json.dumps({'jsonrpc': '2.0', 'id': 1, 'method': method, 'params': params})
    done = subprocess.run([config['command'], *config['args']], input=request + '\n',
                          capture_output=True, text=True, timeout=10)
    return json.loads(done.stdout)['result']


tools = [t['name'] for t in workspace('tools/list', {})['tools']]
with open(log / 'turns.jsonl', 'a') as turns:
    turns.write(json.dumps({'args': args, 'cwd': os.getcwd(), 'prompt': prompt, 'tools': tools}) + '\n')


def emit(value):
    print(json.dumps(value), flush=True)


servers = [{'name': 'agent_workspace', 'status': 'connected'}]
if 'offer github' in prompt:
    servers.append({'name': 'github', 'status': 'connected'})
session = args[args.index('--resume') + 1] if '--resume' in args else str(uuid.uuid4())
emit({'type': 'system', 'subtype': 'init', 'session_id': session, 'mcp_servers': servers,
      'tools': ['Read', 'Write', *('mcp__agent_workspace__' + t for t in tools)],
      'permissionMode': 'dontAsk'})
reply = prompt.strip().splitlines()[0]
if prompt.startswith('save '):
    file, title = prompt.strip()[5:].split(' as ', 1)
    Path(file).write_text('# ' + title + '\n')
    result = workspace('tools/call', {'name': 'save_output', 'arguments': {'path': file, 'title': title}})
    reply = result['content'][0]['text']
emit({'type': 'assistant', 'message': {'content': [{'type': 'text', 'text': reply}]}})
emit({'type': 'result', 'is_error': False, 'result': reply, 'usage': {'input_tokens': 10, 'output_tokens': 5},
      'modelUsage': {}})
