#!/usr/bin/env python3
"""Explicit, bounded live CLI smoke. Uses temporary stores and no product repositories."""
import argparse
import json
from pathlib import Path
import subprocess
import tempfile
import time

parser=argparse.ArgumentParser()
parser.add_argument('--provider', choices=['claude','codex','both'], default='both')
parser.add_argument('--host', default='target/debug/workspace-host')
args=parser.parse_args()
with tempfile.TemporaryDirectory(prefix='workspace-live-smoke-') as home:
    host=subprocess.Popen([str(Path(args.host).resolve()), '--data-dir', home],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
    def request(command):
        host.stdin.write(json.dumps({'version':1,'id':str(time.monotonic_ns()),'command':command})+'\n');host.stdin.flush()
        response=json.loads(host.stdout.readline())
        if response.get('error'): raise RuntimeError(response['error'])
        return response['result']
    try:
        for provider in (['claude','codex'] if args.provider=='both' else [args.provider]):
            profile={'provider':provider,'model':'sonnet' if provider=='claude' else 'gpt-6.1-sol','effort':'low'}
            policy={'role':'project_orchestrator','mode':'fixed','default':profile,'allowed':[profile],'small':profile,'standard':profile,'complex':profile}
            request({'type':'set_policy','policy':policy})
            project=request({'type':'create_project','name':f'{provider} communication smoke'})
            session=next(s for s in request({'type':'snapshot'})['sessions'] if s['project_id']==project['id'])
            request({'type':'configure_session','session_id':session['id'],'profile':profile})
            request({'type':'set_live','project_id':project['id'],'enabled':True})
            start=time.monotonic()
            request({'type':'send','id':f'{provider}-smoke','sender':None,'recipient':session['id'],'body':'This is a bounded integration smoke test. Call workspace_context exactly once, then reply WORKSPACE_CONNECTED followed by your project name from that tool. Do not create teams, ask questions, read other files or execute commands.'})
            while time.monotonic()-start<150:
                snap=request({'type':'snapshot'})
                runtime=next((r for r in snap['runtimes'] if r['session_id']==session['id']),{})
                if runtime.get('last_error'): raise RuntimeError(f'{provider}: {runtime["last_error"]}')
                if runtime.get('last_finished_at'):
                    messages=request({'type':'messages','session_id':session['id'],'before':None,'limit':100})
                    output='\n'.join(m['body'] for m in messages if m['sender']==session['id'])
                    assert 'WORKSPACE_CONNECTED' in output,output
                    print(json.dumps({'provider':provider,'elapsed_seconds':round(time.monotonic()-start,2),'session_resumable':bool(runtime.get('provider_session_id')),'reply':output}),flush=True)
                    break
                time.sleep(.2)
            else: raise TimeoutError(provider+' did not complete in 150 seconds')
            prior_session=runtime['provider_session_id']
            prior_finish=runtime['last_finished_at']
            request({'type':'send','id':f'{provider}-resume','sender':None,'recipient':session['id'],'body':'Resume smoke: call workspace_context once and reply RESUME_CONNECTED with the project name. No report is needed because you are the main coordinator. Do nothing else.'})
            resume_start=time.monotonic()
            while time.monotonic()-resume_start<150:
                runtime=next(r for r in request({'type':'snapshot'})['runtimes'] if r['session_id']==session['id'])
                if runtime.get('last_error'): raise RuntimeError(runtime['last_error'])
                if runtime.get('last_finished_at',0)>prior_finish:
                    assert runtime['provider_session_id']==prior_session,'Provider session changed during resume'
                    messages=request({'type':'messages','session_id':session['id'],'before':None,'limit':100})
                    assert any('RESUME_CONNECTED' in m['body'] and m['sender']==session['id'] for m in messages)
                    print(json.dumps({'provider':provider,'resume_seconds':round(time.monotonic()-resume_start,2),'same_session':True}),flush=True)
                    break
                time.sleep(.2)
            else: raise TimeoutError('Resume did not complete')
            request({'type':'set_live','project_id':project['id'],'enabled':False})
    finally:
        host.stdin.close()
        host.wait(timeout=10)
