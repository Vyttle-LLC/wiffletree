#!/usr/bin/env python3
"""A provider double whose agents never stop working, for the many-agents fixture.

Each turn streams `BUSY_STEPS` narrated commands, `BUSY_STEP_SECONDS` apart, records a checklist
every few steps and reports progress. A worker then reports itself blocked, which wakes its
coordinator; a coordinator messages each of its idle workers, which wakes them again. So the
host keeps emitting state, step and progress events for as long as it runs. No inference.
"""
import json
import os
import subprocess
import sys
import time
import uuid

args = sys.argv[1:]
if args[:2] == ['auth', 'status']:
    print(json.dumps({'loggedIn': True, 'email': 'fixture@example.invalid', 'orgId': 'fixture-org'}))
    sys.exit(0)
# The fixture's agents all run on Claude, so only its stream is spoken.
config = json.loads(args[args.index('--mcp-config') + 1])['mcpServers']['agent_workspace']
resumed = args[args.index('--resume') + 1] if '--resume' in args else None
steps = int(os.environ.get('BUSY_STEPS', '20'))
pause = float(os.environ.get('BUSY_STEP_SECONDS', '0.5'))
mcp = subprocess.Popen([config['command'], 'agent-mcp'], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)


def rpc(method, params):
    mcp.stdin.write(json.dumps({'jsonrpc': '2.0', 'id': 1, 'method': method, 'params': params}) + '\n')
    mcp.stdin.flush()
    return json.loads(mcp.stdout.readline()).get('result', {})


def tool(name, **arguments):
    """Calls a workspace tool; a refusal is ignored, since the load matters, not the answer."""
    result = rpc('tools/call', {'name': name, 'arguments': arguments})
    try:
        return json.loads(result['content'][0]['text'])
    except (KeyError, IndexError, ValueError):
        return None


def emit(value):
    print(json.dumps(value), flush=True)


def narrate(n):
    text = f'Step {n}: reading the next file and running the checks.'
    emit({'type': 'assistant', 'message': {'content': [{'type': 'text', 'text': text}]}})
    emit({'type': 'assistant', 'message': {'content': [{'type': 'tool_use', 'id': f'busy-{n}', 'name': 'Bash', 'input': {'command': f'cargo test step_{n}'}}]}})
    time.sleep(pause)
    emit({'type': 'user', 'message': {'content': [{'type': 'tool_result', 'tool_use_id': f'busy-{n}', 'content': 'ok'}]}})


try:
    rpc('initialize', {'protocolVersion': '2024-11-05', 'capabilities': {}, 'clientInfo': {'name': 'busy', 'version': '1'}})
    sid = resumed or str(uuid.uuid4())
    sys.stdin.read()
    emit({'type': 'system', 'subtype': 'init', 'session_id': sid, 'mcp_servers': [{'name': 'agent_workspace', 'status': 'connected'}]})
    context = tool('workspace_context') or {}
    me = context.get('self', {})
    for n in range(1, steps + 1):
        narrate(n)
        if n % 5 == 0:
            tool('set_plan', items=[{'text': f'Part {k}', 'done': k <= n // 5} for k in range(1, steps // 5 + 2)])
        if n == steps // 2:
            tool('report', message_id=str(uuid.uuid4()), kind='progress', body=f'Halfway through {steps} steps')
    if me.get('role') == 'project_orchestrator':
        for session in tool('workspace_context').get('team', []):
            if session.get('parent_id') == me.get('id') and session.get('status') != 'working' and not session.get('archived'):
                tool('send_message', recipient=session['id'], message_id=str(uuid.uuid4()), body='Carry on with the next part.')
    else:
        tool('report', message_id=str(uuid.uuid4()), kind='blocked', body='Which part next?')
    result = 'Done for now.'
    emit({'type': 'assistant', 'message': {'content': [{'type': 'text', 'text': result}]}})
    emit({'type': 'result', 'is_error': False, 'usage': {'input_tokens': 100, 'output_tokens': 30}})
finally:
    mcp.stdin.close()
    mcp.wait(timeout=5)
