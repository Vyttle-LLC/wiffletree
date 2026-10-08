#!/usr/bin/env python3
"""Deterministic end-to-end transport, scheduling, permission and recovery checks. No inference."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time

root=Path(__file__).resolve().parents[1]
fake=root/'crates/workspace-host/tests/fixtures/provider.py'
with tempfile.TemporaryDirectory(prefix='workspace-protocol-test-') as tmp:
    tmp=Path(tmp);repo=tmp/'repo';repo.mkdir()
    for args in [['init'],['-c','user.name=Workspace Test','-c','user.email=test@example.invalid','commit','--allow-empty','-m','Fixture']]:
        subprocess.run(['git',*args],cwd=repo,check=True,capture_output=True)
    (tmp/'usage').mkdir()
    env={**os.environ,'WORKSPACE_TEST_USAGE_DIR':str(tmp/'usage'),'WORKSPACE_CODEX_BIN':str(fake),'WORKSPACE_CLAUDE_BIN':str(fake)}
    def start():return subprocess.Popen([str(root/'target/debug/workspace-host'),'--data-dir',str(tmp/'home')],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True,env=env)
    host=start()
    def request(command):
        host.stdin.write(json.dumps({'version':1,'id':'test','command':command})+'\n');host.stdin.flush()
        value=json.loads(host.stdout.readline())
        assert not value.get('error'),value
        return value['result']
    def snapshot():return request({'type':'snapshot'})
    def until(predicate,seconds=10):
        deadline=time.monotonic()+seconds
        while time.monotonic()<deadline:
            value=snapshot()
            if predicate(value):return value
            time.sleep(.02)
        raise AssertionError(value)
    try:
        profile={'provider':'codex','model':'gpt-6.1-sol','effort':'medium'}
        for role in ['implementer']:
            request({'type':'set_policy','policy':{'role':role,'mode':'fixed','default':profile,'allowed':[profile],'small':profile,'standard':profile,'complex':profile}})
        profile={'provider':'claude','model':'sonnet','effort':'low'}
        request({'type':'set_policy','policy':{'role':'tester','mode':'fixed','default':profile,'allowed':[profile],'small':profile,'standard':profile,'complex':profile}})
        request({'type':'set_workspaces_dir','path':str(tmp/'workspaces')})
        project=request({'type':'create_project','name':'Protocol fixture'})
        main=snapshot()['sessions'][0]
        request({'type':'attach_repository','project_id':project['id'],'path':str(repo),'base':'HEAD'})
        assert project['id'] not in snapshot()['live_projects']
        start_time=time.monotonic()
        request({'type':'send','id':'start','sender':None,'recipient':main['id'],'body':'START_HANDOFF'})
        assert project['id'] in snapshot()['live_projects'],'A human message starts its project'
        # Acceptance removes the worktree once the tester's turn ends.
        done=until(lambda s:any(t['state']=='accepted' and not Path(t['worktree']).exists() for t in s['tickets']))
        ticket=done['tickets'][0]
        assert subprocess.run(['git','show',f"{ticket['branch']}:result.txt"],cwd=repo,check=True,capture_output=True,text=True).stdout=='verified','The branch keeps the work'
        assert not (repo/'result.txt').exists()
        assert any(s['provider']=='claude' and s['role']=='tester' for s in done['sessions'])
        print(f'Cross-provider protocol handoff passed in {time.monotonic()-start_time:.3f}s',flush=True)
        until(lambda s:all(x['status']!='working' for x in s['sessions']))
        def steps(session,after=0,run=None):return request({'type':'steps','session_id':session,'run_id':run,'after':after})
        transcript=request({'type':'messages','session_id':main['id'],'before':None,'limit':100})
        assert any(m['body']=='MAIN_READY' for m in transcript),transcript
        assert not any('Checking the workspace' in m['body'] for m in transcript),'Narration stays out of the chat'
        page=steps(main['id'])
        assert [(s['kind'],s['state'],s['title']) for s in page['steps']][:2]==[('narration','succeeded','Checking the workspace.'),('command','succeeded','ls')],page
        assert not page['running'] and not steps(main['id'],page['revision'])['steps']
        print('Work steps stream beside the chat, which receives only final replies',flush=True)
        usage_command={'type':'usage','days':7,'project_id':None,'provider':None,'timezone':'America/New_York'}
        usage=request(usage_command)
        assert usage['totals']['requests']>=4,usage
        assert usage['totals']['output']==30*usage['totals']['requests'],usage
        assert usage['totals']['partial_requests']==0,usage
        assert sum(c['own']['input']+c['own']['output']+c['workers']['input']+c['workers']['output'] for c in usage['coordinators'])==usage['totals']['input']+usage['totals']['output']
        for provider,input_per_request in [('claude',125),('codex',100)]:
            selected=request({**usage_command,'provider':provider})
            assert selected['totals']['input']==input_per_request*selected['totals']['requests'],selected
            if provider=='codex': assert selected['totals']['requests']>=1,selected
        quota=next(q for q in request({'type':'quotas'}) if q['provider']=='claude')
        assert quota['windows'][0]['used_percent']==25,quota
        print('Live request ledger and streamed quota reporting reconcile across coordinators',flush=True)
        before=next(r['provider_session_id'] for r in snapshot()['runtimes'] if r['session_id']==main['id'])
        override={'provider':'claude','model':'sonnet','effort':'high'}
        request({'type':'configure_session','session_id':main['id'],'profile':override})
        assert next(r['profile'] for r in snapshot()['runtimes'] if r['session_id']==main['id'])==override
        request({'type':'send','id':'permission','sender':None,'recipient':main['id'],'body':'REQUEST_PERMISSION'})
        attention=until(lambda s:len(s['attention'])==1)['attention'][0]
        assert 'python3 -m unittest' in attention['prompt']
        request({'type':'resolve_attention','id':attention['id'],'answer':'approved'})
        until(lambda s:all(x['status']!='working' for x in s['sessions']))
        messages=request({'type':'messages','session_id':main['id'],'before':None,'limit':100})
        assert any(m['body']=='PERMISSION_allow' for m in messages)
        assert next(r['provider_session_id'] for r in snapshot()['runtimes'] if r['session_id']==main['id'])==before
        def settled():return until(lambda s:all(x['status']!='working' for x in s['sessions']))
        request({'type':'send','id':'ask','sender':None,'recipient':main['id'],'body':'ASK_USER'})
        until(lambda s:len(s['attention'])==1);settled()
        request({'type':'send','id':'continue','sender':None,'recipient':main['id'],'body':'CONTINUE'})
        until(lambda s:not s['attention']);settled()
        request({'type':'send','id':'ask-again','sender':None,'recipient':main['id'],'body':'ASK_USER'})
        question=until(lambda s:len(s['attention'])==1)['attention'][0];settled()
        delivered=len(request({'type':'messages','session_id':main['id'],'before':None,'limit':100}))
        request({'type':'dismiss_attention','id':question['id']})
        assert not snapshot()['attention']
        time.sleep(.3)
        assert len(request({'type':'messages','session_id':main['id'],'before':None,'limit':100}))==delivered,'Dismissing does not wake the coordinator'
        print('Coordinator closes settled questions; the human can dismiss one without waking it',flush=True)
        request({'type':'send','id':'timer','sender':None,'recipient':main['id'],'body':'SCHEDULE_TIMER'})
        timer=until(lambda s:len(s['schedules'])==1)['schedules'][0]
        until(lambda s:not s['schedules']);settled()
        until(lambda _:any(m['body']=='TIMER_FIRED' for m in request({'type':'messages','session_id':main['id'],'before':None,'limit':100})))
        fire=next(m for m in request({'type':'messages','session_id':main['id'],'before':None,'limit':100}) if m['id'].startswith(f"timer:{timer['id']}:"))
        assert fire['sender']==main['id'] and fire['receipt']=='completed',fire
        request({'type':'send','id':'monitor','sender':None,'recipient':main['id'],'body':'SCHEDULE_MONITOR'})
        monitor=until(lambda s:len(s['schedules'])==1)['schedules'][0];settled()
        assert monitor['every_ms']==30*60_000 and monitor['until']-monitor['first_at']==150*60_000,monitor
        print('A coordinator timer fires between turns as its own message',flush=True)
        request({'type':'send','id':'hang','sender':None,'recipient':main['id'],'body':'HANG_UNTIL_CANCELLED'})
        until(lambda s:any(x['id']==main['id'] and x['status']=='working' for x in s['sessions']))
        until(lambda _:('command','running') in [(s['kind'],s['state']) for s in steps(main['id'])['steps']])
        request({'type':'set_live','project_id':project['id'],'enabled':False})
        until(lambda s:any(r['session_id']==main['id'] and r.get('last_error') for r in s['runtimes']))
        messages=request({'type':'messages','session_id':main['id'],'before':None,'limit':100})
        assert next(m for m in messages if m['id']=='hang')['receipt']=='held'
        hung=steps(main['id'])
        assert ('command','interrupted') in [(s['kind'],s['state']) for s in hung['steps']],hung
        saved_usage=request(usage_command)['totals']
        host.stdin.close();host.wait(timeout=5);host=start()
        assert request(usage_command)['totals']==saved_usage
        assert not snapshot()['live_projects']
        assert next(r['profile'] for r in snapshot()['runtimes'] if r['session_id']==main['id'])==override
        assert steps(main['id'],run=hung['run_id'])['steps']==hung['steps'],'Steps survive a restart'
        assert snapshot()['schedules']==[monitor],'Timers survive a restart'
        request({'type':'reconcile_session','session_id':main['id'],'retry':False})
        messages=request({'type':'messages','session_id':main['id'],'before':None,'limit':100})
        assert next(m for m in messages if m['id']=='hang')['receipt']=='cancelled'
        print('Permission response, exact session resume, cancellation and restart reconciliation passed',flush=True)
    finally:
        host.stdin.close();host.wait(timeout=5)
