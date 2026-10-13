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
def wait_for(gate):
    # Holds a turn or a compaction until the test creates the gate file.
    deadline=time.time()+30
    while not (Path(os.environ['WORKSPACE_TEST_USAGE_DIR'])/gate).exists() and time.time()<deadline:time.sleep(0.02)
if args[:1]==['app-server']:
    # Codex's app-server for one compaction, not a turn: it checks the four requests it reads
    # and replays the server side of a recorded exchange. `codex-compact.json` in the test's
    # scratch directory chooses the recording and what to change in it.
    import tomllib
    scratch=Path(os.environ['WORKSPACE_TEST_USAGE_DIR']);streams=Path(__file__).parent/'streams'
    plan=json.loads((scratch/'codex-compact.json').read_text()) if (scratch/'codex-compact.json').exists() else {}
    overrides=[tomllib.loads(o) for o in args[2::2]]
    assert args[1::2]==['-c']*3 and [list(o)[0] for o in overrides]==['model_reasoning_effort','developer_instructions','mcp_servers'],args
    requests=[json.loads(sys.stdin.readline()) for _ in range(4)]
    assert [(r.get('method'),r.get('id')) for r in requests]==[('initialize',1),('initialized',None),('thread/resume',2),('thread/compact/start',3)],requests
    # A turn's token lets the workspace's tools answer; a compaction's must not.
    helper=overrides[2]['mcp_servers']['agent_workspace']['command']
    call=json.dumps({'jsonrpc':'2.0','id':1,'method':'tools/call','params':{'name':'workspace_context','arguments':{}}})
    answer=json.loads(subprocess.run([helper,'agent-mcp'],input=call+'\n',capture_output=True,text=True,timeout=10).stdout)
    with open(Path(os.environ['WORKSPACE_TEST_PROMPT_DIR'])/'codex-compactions.txt','a') as log:
        log.write(json.dumps({'cwd':os.getcwd(),'effort':overrides[0]['model_reasoning_effort'],'instructions':overrides[1]['developer_instructions'],'tools_refused':'error' in answer or answer['result']['isError'],'requests':requests})+'\n')
    exchange=[json.loads(line) for line in (streams/f"codex-compact-{plan.get('stream','success')}.jsonl").read_text().splitlines()]
    recording=[entry['message'] for entry in exchange if entry['from']=='server']
    # The recording's thread becomes the agent's, and its compaction turn gets an id of its own.
    thread=requests[2]['params']['threadId'];turn=str(uuid.uuid4())
    recorded_thread=next(e['message']['params']['threadId'] for e in exchange if e['message'].get('method')=='thread/resume')
    recorded_turn=next((m['params']['turn']['id'] for m in recording if m.get('method')=='turn/started'),'no turn')
    def own(text):return text.replace(recorded_thread,thread).replace(recorded_turn,turn)
    def write_session_record():
        # What Codex appends to the thread's session record for a compaction, with the usage
        # entry of its one model request left out, repeated or made unreadable when asked.
        entries=[json.loads(own(line)) for line in (streams/'codex-compact-session-record-compacted.jsonl').read_text().splitlines()]
        entries=entries[next(i for i,e in enumerate(entries) if e['payload'].get('type')=='thread_settings_applied'):]
        spent=next(e for e in entries if e['type']=='token_usage_record');at=entries.index(spent)
        match plan.get('usage','one'):
            case 'none':entries.remove(spent)
            case 'two':entries.insert(at,spent)
            case 'no-input':del spent['payload']['usage']['input_tokens']
            case 'more-cached':spent['payload']['usage']['cached_input_tokens']=spent['payload']['usage']['input_tokens']+1
        sessions=Path(os.environ['CODEX_HOME'])/'sessions'
        record=next(sessions.rglob(f'*{thread}.jsonl'),sessions/'2026/10/13'/f'rollout-2026-10-13T17-10-48-{thread}.jsonl')
        record.parent.mkdir(parents=True,exist_ok=True)
        with open(record,'a') as file:file.write(''.join(json.dumps(e)+'\n' for e in entries))
    for message in recording:
        method=message.get('method')
        if method and method==plan.get('stop_before'):break
        if str(message.get('id')) in plan.get('refuse',{}) and not method:message={'error':{'code':-32600,'message':plan['refuse'][str(message['id'])]},'id':message['id']}
        if plan.get('no_figure') and method=='thread/tokenUsage/updated' and message['params']['turnId']==recorded_turn:continue
        if method=='turn/completed':
            if plan.get('gate'):wait_for('gate-compact')
            message['params']['turn'].update(plan.get('turn',{}))
            if message['params']['turn']['status']=='completed':write_session_record()
        print(own(json.dumps(message)),flush=True)
    if 'exit' in plan:
        sys.stderr.write(plan.get('stderr',''));sys.exit(plan['exit'])
    # The real server runs until its input closes. The double logs anything more it is sent and
    # then stays, so only the host's stopping its process group ends it.
    for line in sys.stdin:
        with open(scratch/'codex-compact-extra.txt','a') as log:log.write(line)
    time.sleep(30);sys.exit(0)
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
def model_usage(sid,spent):
    # Claude's modelUsage counts the session's whole life, a cut-off turn's requests included,
    # so the double keeps a running total and adds to it as each request is made.
    path=Path(os.environ['WORKSPACE_TEST_USAGE_DIR'])/f'{sid}.model-usage';total=json.loads(path.read_text()) if path.exists() else {}
    for model,counts in spent.items():
        kept=total.setdefault(model,{})
        for key in ('inputTokens','outputTokens','cacheReadInputTokens','cacheCreationInputTokens'):kept[key]=kept.get(key,0)+counts.get(key,0)
    path.write_text(json.dumps(total));return total
def emit(value):print(json.dumps(value),flush=True)
def pick(ctx,role,provider=None):
    # The first allowed model of the provider, or of the role's first provider.
    ms=ctx['model_selection'];providers=[provider] if provider else ms['role_providers'].get(role) or [p['provider'] for p in ms['providers']]
    access=next(p for p in ms['providers'] if p['provider'] in providers);model=access['models'][0]
    return {'provider':access['provider'],'model':model['model'],'effort':model['effort']}
try:
    rpc('initialize',{'protocolVersion':'2024-11-05','capabilities':{},'clientInfo':{'name':'test','version':'1'}})
    sid=resumed or str(uuid.uuid4())
    prompt=sys.stdin.read()
    if claude and (prompt=='/compact' or prompt.startswith('/compact ')):
        # Claude's compact command, not a turn: a recorded stream chosen by a word in the note.
        note=prompt[len('/compact '):]
        if os.environ.get('WORKSPACE_TEST_PROMPT_DIR'):
            with open(Path(os.environ['WORKSPACE_TEST_PROMPT_DIR'])/'compactions.txt','a') as log:log.write(json.dumps({'input':prompt,'resumed':resumed})+'\n')
        if 'WAIT_FOR_GATE' in note:wait_for('gate-compact')
        if 'STREAM_EVENTS' in note:
            # Claude streams no request events for a compaction; a stream that did must not be counted.
            for event in ({'type':'message_start','message':{'id':str(uuid.uuid4()),'model':'opus','usage':{'input_tokens':7,'cache_read_input_tokens':0,'cache_creation_input_tokens':0,'output_tokens':0}}},{'type':'message_delta','usage':{'output_tokens':3}},{'type':'message_stop'}):emit({'type':'stream_event','event':event})
        stream='compact-nothing' if 'NOTHING' in note else 'compact-failed' if 'FAIL' in note else 'compact-success'
        for line in (Path(__file__).parent/'streams'/f'{stream}.jsonl').read_text().splitlines():
            record=json.loads(line)
            if 'NO_STATUS' in note and record.get('subtype')=='status':continue
            if 'session_id' in record:record['session_id']=sid
            if record['type']=='result':record['modelUsage']=model_usage(sid,record['modelUsage'])
            emit(record)
        sys.exit(0)
    emit({'type':'system','subtype':'init','session_id':sid,'mcp_servers':[{'name':'agent_workspace','status':'connected'}]} if claude else {'type':'thread.started','thread_id':sid})
    counter_path=Path(os.environ['WORKSPACE_TEST_USAGE_DIR']) / sid
    previous=json.loads(counter_path.read_text()) if counter_path.exists() else 0
    request_id=str(uuid.uuid4())
    if claude:
        emit({'type':'stream_event','event':{'type':'message_start','message':{'id':request_id,'model':'opus','usage':{'input_tokens':100,'cache_read_input_tokens':20,'cache_creation_input_tokens':5,'output_tokens':0}}}})
        model_usage(sid,{'claude-opus-5-5':{'inputTokens':100,'cacheReadInputTokens':20,'cacheCreationInputTokens':5}})
        emit({'type':'rate_limit_event','rate_limit_info':{'rateLimitType':'five_hour','status':'allowed','utilization':0.25,'resetsAt':int(time.time())+300}})
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
    # A checklist is a work step: three of six items done.
    if 'SET_PLAN' in prompt:
        assert tool('set_plan',items=[{'text':f'Step {n}','done':n<=3} for n in range(1,7)])=={'done':3,'total':6}
    # A retried turn finishes, so a test can see the redelivered input run.
    if 'HANG_UNTIL_CANCELLED' in prompt and 'Retry of interrupted turn' not in prompt:time.sleep(30)
    if 'WAIT_FOR_GATE' in prompt:wait_for('gate-turn')
    if 'EXIT_WITH_ERROR' in prompt and 'Retry of interrupted turn' not in prompt:sys.exit(3)
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
    emit({'type':'result','is_error':False,'usage':{'input_tokens':50000,'output_tokens':50000},'modelUsage':model_usage(sid,{'claude-opus-5-5':{'outputTokens':30}})} if claude else {'type':'turn.completed','usage':{'input_tokens':100*(previous+1),'cached_input_tokens':20*(previous+1),'output_tokens':30*(previous+1)}})
finally:
    mcp.stdin.close();mcp.wait(timeout=5)
