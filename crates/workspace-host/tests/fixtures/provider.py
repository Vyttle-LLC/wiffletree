#!/usr/bin/env python3
"""Deterministic provider double. Only used via WORKSPACE_*_BIN in tests."""
import json
import os
from pathlib import Path
import subprocess
import sys
import time
import tomllib
import uuid

args=sys.argv[1:]
if args[:2]==['auth','status']:
    print(json.dumps({'loggedIn':True,'email':'fixture@example.invalid','orgId':'fixture-org'}))
    sys.exit(0)
if '/context' in args:
    report="""## Context Usage

**Model:** claude-opus-5-5  
**Tokens:** 90k / 1m (9%)

### Estimated usage by category

| Category | Tokens | Percentage |
|----------|--------|------------|
| System prompt | 4.2k | 0.4% |
| System tools | 17k | 1.7% |
| MCP tools | 52k | 5.2% |
| System tools (deferred) | 16.6k | 1.7% |
| Custom agents | 3.4k | 0.3% |
| Memory files | 8.6k | 0.9% |
| Skills | 5.1k | 0.5% |
| Messages | 0 | 0.0% |
| Free space | 897k | 89.7% |
| Autocompact buffer | 13k | 1.3% |
"""
    print(json.dumps({'is_error':False,'num_turns':0,'result':report}))
    sys.exit(0)
claude='--output-format' in args
if claude:
    assert '--dangerously-skip-permissions' in args, args
    assert '--include-partial-messages' in args, args
    assert '--permission-prompt-tool' not in args, args
    assert args[args.index('--tools')+1] == 'default', args
    config=json.loads(args[args.index('--mcp-config')+1])['mcpServers']['agent_workspace']
    resumed=args[args.index('--resume')+1] if '--resume' in args else None
else:
    assert '--dangerously-bypass-approvals-and-sandbox' in args, args
    assert not any('sandbox_mode=' in a for a in args), args
    config=next(tomllib.loads(args[i+1])['mcp_servers']['agent_workspace'] for i,a in enumerate(args[:-1]) if a=='-c' and args[i+1].startswith('mcp_servers.'))
    resumed=args[args.index('resume')+1] if 'resume' in args else None
mcp=subprocess.Popen([config['command'],'agent-mcp'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
def rpc(method,params):
    mcp.stdin.write(json.dumps({'jsonrpc':'2.0','id':1,'method':method,'params':params})+'\n');mcp.stdin.flush()
    value=json.loads(mcp.stdout.readline())
    assert 'error' not in value,value
    result=value['result']
    if method=='tools/call':
        assert not result.get('isError'),result
        return json.loads(result['content'][0]['text'])
    return result
def tool(name,**arguments):return rpc('tools/call',{'name':name,'arguments':arguments})
def emit(value):print(json.dumps(value),flush=True)
try:
    rpc('initialize',{'protocolVersion':'2024-11-05','capabilities':{},'clientInfo':{'name':'test','version':'1'}})
    sid=resumed or str(uuid.uuid4())
    emit({'type':'system','subtype':'init','session_id':sid,'mcp_servers':[{'name':'agent_workspace','status':'connected'}]} if claude else {'type':'thread.started','thread_id':sid})
    counter_path=Path(os.environ['WORKSPACE_TEST_USAGE_DIR']) / sid
    previous=json.loads(counter_path.read_text()) if counter_path.exists() else 0
    request_id=str(uuid.uuid4())
    if claude:
        emit({'type':'stream_event','event':{'type':'message_start','message':{'id':request_id,'model':'opus','usage':{'input_tokens':100,'cache_read_input_tokens':20,'cache_creation_input_tokens':5,'output_tokens':0}}}})
        emit({'type':'rate_limit_event','rate_limit_info':{'rateLimitType':'five_hour','status':'allowed','utilization':0.25,'resetsAt':int(time.time())+300}})
    prompt=sys.stdin.read()
    ctx=tool('workspace_context');me=ctx['self'];role=me['role']
    # Every turn narrates and runs one command, so work steps stay out of the transcript.
    if claude:
        emit({'type':'stream_event','event':{'type':'content_block_start','index':0,'content_block':{'type':'text','text':''}}})
        emit({'type':'stream_event','event':{'type':'content_block_delta','index':0,'delta':{'type':'text_delta','text':'Checking the workspace.'}}})
        emit({'type':'assistant','message':{'content':[{'type':'text','text':'Checking the workspace.'}]}})
        emit({'type':'stream_event','event':{'type':'content_block_stop','index':0}})
        emit({'type':'assistant','message':{'content':[{'type':'tool_use','id':'fixture-ls','name':'Bash','input':{'command':'ls'}}]}})
    else:
        emit({'type':'item.completed','item':{'id':'fixture-say','type':'agent_message','text':'Checking the workspace.'}})
        emit({'type':'item.started','item':{'id':'fixture-ls','type':'command_execution','command':'/bin/zsh -lc ls','aggregated_output':'','exit_code':None,'status':'in_progress'}})
    if 'HANG_UNTIL_CANCELLED' in prompt:time.sleep(30)
    if claude: emit({'type':'user','message':{'content':[{'type':'tool_result','tool_use_id':'fixture-ls','content':'result.txt'}]}})
    else: emit({'type':'item.completed','item':{'id':'fixture-ls','type':'command_execution','command':'/bin/zsh -lc ls','aggregated_output':'result.txt\n','exit_code':0,'status':'completed'}})
    if 'REQUEST_PERMISSION' in prompt:
        permission=tool('request_permission',tool_name='Bash',input={'command':'python3 -m unittest'},tool_use_id='test-permission')
        result='PERMISSION_'+permission['behavior']
    elif role=='project_orchestrator':
        if 'ASK_USER' in prompt:
            tool('ask_user',request_id=str(uuid.uuid4()),question='Add the API repository, then tell me to continue.')
        if 'CONTINUE' in prompt:
            # A real coordinator decides from the turn's open-question reminder; the double trusts it.
            for question in ctx['open_questions']:
                assert question['operation_id'] in prompt and 'close_question' in prompt, prompt
                tool('close_question',request_id=question['operation_id'],resolution='Repository attached')
        if 'START_HANDOFF' in prompt:
            team=tool('create_repo_coordinator',repository_id=ctx['repositories'][0]['id'],provider='codex')
            tool('send_message',recipient=team['id'],message_id='start',body='Run the fixture ticket.')
        result='MAIN_READY'
    elif role=='task_orchestrator':
        tickets=ctx['tickets']
        if not tickets:
            ticket=tool('create_ticket',title='Fixture ticket',brief='Create result.txt containing verified')
            tool('assign_ticket',ticket_id=ticket['id'],role='implementer',instruction='Implement fixture')
        elif tickets[0]['state']=='ready_for_testing':
            tool('assign_ticket',ticket_id=tickets[0]['id'],role='tester',instruction='Verify fixture')
        elif tickets[0]['state']=='passed':
            tool('accept_ticket',ticket_id=tickets[0]['id'])
            tool('report',message_id='accepted',kind='completed',body='Independent verification passed')
        result='COORDINATED'
    elif role=='implementer':
        Path('result.txt').write_text('verified')
        # Acceptance removes the worktree and refuses uncommitted work, so the double commits like a real implementer.
        for git in [['add','result.txt'],['-c','user.name=Fixture','-c','user.email=fixture@example.invalid','commit','-q','-m','Write result']]:
            subprocess.run(['git',*git],check=True,capture_output=True)
        tool('report',message_id='ready',kind='ready_for_testing',body='result.txt committed')
        result='IMPLEMENTED'
    else:
        assert Path('result.txt').read_text()=='verified'
        tool('report',message_id='passed',kind='passed',body='Read result.txt: verified')
        result='TESTED'
    emit({'type':'assistant','message':{'content':[{'type':'text','text':result}]}} if claude else {'type':'item.completed','item':{'type':'agent_message','text':result}})
    if claude:
        emit({'type':'stream_event','event':{'type':'message_delta','usage':{'output_tokens':30}}})
        emit({'type':'stream_event','event':{'type':'message_stop'}})
    # A deliberately cumulative Claude result must not replace request-level counters.
    if not claude: counter_path.write_text(json.dumps(previous+1))
    emit({'type':'result','is_error':False,'usage':{'input_tokens':50000,'output_tokens':50000}} if claude else {'type':'turn.completed','usage':{'input_tokens':100*(previous+1),'cached_input_tokens':20*(previous+1),'output_tokens':30*(previous+1)}})
finally:
    mcp.stdin.close();mcp.wait(timeout=5)
