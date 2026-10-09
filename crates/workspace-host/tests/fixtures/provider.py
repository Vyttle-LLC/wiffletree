#!/usr/bin/env python3
"""Deterministic provider double. Only used via WORKSPACE_*_BIN in tests."""
import json
import os
from pathlib import Path
import subprocess
import sys
import time
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
    import tomllib
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
def pick(ctx,role,provider=None):
    # The first allowed model of the provider, or of the role's first provider.
    ms=ctx['model_selection'];providers=[provider] if provider else ms['role_providers'].get(role) or [p['provider'] for p in ms['providers']]
    access=next(p for p in ms['providers'] if p['provider'] in providers);model=access['models'][0]
    return {'provider':access['provider'],'model':model['model'],'effort':model['effort']}
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
    prompts=os.environ.get('WORKSPACE_TEST_PROMPT_DIR')
    if prompts:
        with open(Path(prompts)/f"{me['id']}.txt",'a') as log:log.write(prompt+'\n=====\n')
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
    # A retried turn finishes, so a test can see the redelivered input run.
    if 'HANG_UNTIL_CANCELLED' in prompt and 'Retry of interrupted turn' not in prompt:time.sleep(30)
    if claude: emit({'type':'user','message':{'content':[{'type':'tool_result','tool_use_id':'fixture-ls','content':'result.txt'}]}})
    else: emit({'type':'item.completed','item':{'id':'fixture-ls','type':'command_execution','command':'/bin/zsh -lc ls','aggregated_output':'result.txt\n','exit_code':0,'status':'completed'}})
    if 'JUST_REPLY' in prompt:
        result='NOTED'
    elif 'REQUEST_PERMISSION' in prompt:
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
            # FAIL_CODEX_ROUND_1 makes the Codex verifier fail round 1, so the implementer fixes it.
            brief='Create result.txt containing verified'+(' FAIL_CODEX_ROUND_1' if 'FAIL_CODEX_ROUND_1' in prompt else '')
            ticket=tool('create_ticket',repository_id=ctx['repositories'][0]['id'],title='Fixture ticket',brief=brief)
            tool('assign_ticket',ticket_id=ticket['id'],role='implementer',instruction='Implement fixture',profile=pick(ctx,'implementer'),reason='Fixture implementer')
        for ticket in ctx['tickets']:
            if ticket['state']=='ready_for_testing':
                assert 'Fixture planned' in prompt and 'Fixture written' in prompt,'Progress rides along with the ready report'
                verifiers=[{'focus':v['focus'],'profile':pick(ctx,v['role'],v.get('provider')),'reason':'Fixture verifier'} for v in ctx['model_selection']['verifiers']]
                tool('verify_ticket',ticket_id=ticket['id'],verifiers=verifiers)
            elif ticket['state']=='passed':
                tool('accept_ticket',ticket_id=ticket['id'])
        # Coordinator control: the tool results come back as the reply, for the test to read.
        children=[s['id'] for s in ctx['team'] if s['parent_id']==me['id'] and not s['archived']]
        control=None
        if 'STOP_CHILDREN' in prompt:
            control=tool('stop_agents',reason='Wrong approach')
        if 'MESSAGE_CHILD' in prompt:
            control=tool('send_message',recipient=children[0],message_id=str(uuid.uuid4()),body='JUST_REPLY queued while stopped')
        if 'RESUME_CHILD' in prompt:
            held={'held':'retry'} if 'HELD_RETRY' in prompt else {}
            message={'message':'JUST_REPLY carry on'} if 'WITH_MESSAGE' in prompt else {}
            control=tool('resume_agent',session_id=children[0],**held,**message)
        if 'SCHEDULE_TIMER' in prompt:
            tool('schedule',label='Fixture check',prompt='TIMER_CHECK',at='+2s')
        if 'SCHEDULE_MONITOR' in prompt:
            tool('schedule',label='Service check',prompt='CHECK_SERVICE',every='30m',until='+3h')
        timer_fire='Sender: your timer' in prompt and '[timer] Fixture check' in prompt and 'TIMER_CHECK' in prompt
        # A coordinator's own read-only check: its final message is the result in the human's chat.
        check='Sender: your timer' in prompt and 'CHECK_SERVICE' in prompt
        result='CONTROL '+json.dumps(control) if control else 'TIMER_FIRED' if timer_fire else 'SERVICE_HEALTHY' if check else 'MAIN_READY'
    elif role=='implementer' and 'Verification round' in prompt:
        Path('fix.txt').write_text('fixed')
        for git in [['add','fix.txt'],['-c','user.name=Fixture','-c','user.email=fixture@example.invalid','commit','-q','-m','Fix verification failure']]:
            subprocess.run(['git',*git],check=True,capture_output=True)
        tool('report',message_id='fixed',kind='ready_for_testing',body='fix.txt committed')
        result='FIXED'
    elif role=='implementer':
        tool('report',message_id='planned',kind='progress',body='Fixture planned')
        Path('result.txt').write_text('verified')
        tool('report',message_id='written',kind='progress',body='Fixture written')
        # Acceptance removes the worktree and refuses uncommitted work, so the double commits like a real implementer.
        for git in [['add','result.txt'],['-c','user.name=Fixture','-c','user.email=fixture@example.invalid','commit','-q','-m','Write result']]:
            subprocess.run(['git',*git],check=True,capture_output=True)
        tool('report',message_id='ready',kind='ready_for_testing',body='result.txt committed')
        result='IMPLEMENTED'
    else:
        assert Path('result.txt').read_text()=='verified'
        # Verifiers of one round overlap; a short check keeps that visible in the timestamps.
        if 'Verify ticket' in prompt: time.sleep(0.3)
        if 'FAIL_CODEX_ROUND_1' in prompt and 'Focus: Codex' in prompt and ': round 1 of' in prompt:
            tool('report',message_id='failed',kind='failed',body='fix.txt is missing',findings=[{'severity':'blocking','location':'result.txt:1','summary':'fix.txt is missing','trigger':'Verify round 1','evidence':'ls shows no fix.txt'}])
        else:
            tool('report',message_id=str(uuid.uuid4()),kind='passed',body='Read result.txt: verified')
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
