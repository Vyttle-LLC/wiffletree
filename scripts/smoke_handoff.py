#!/usr/bin/env python3
"""Bounded live Claude/Codex ticket handoff in a disposable local repository; no push/deploy."""
import argparse
import json
import subprocess
import tempfile
import time
from pathlib import Path

parser=argparse.ArgumentParser()
for role in ['coordinator','implementer','tester']:
    parser.add_argument('--'+role, choices=['codex','claude'], default='codex')
args=parser.parse_args()
root=Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory(prefix='workspace-handoff-') as directory:
    directory=Path(directory);repo=directory/'fixture';repo.mkdir()
    (repo/'calc.py').write_text('def add(a, b):\n    return a - b\n')
    (repo/'test_calc.py').write_text('import unittest\nfrom calc import add\nclass Addition(unittest.TestCase):\n    def test_add(self):\n        self.assertEqual(add(2, 3), 5)\n        self.assertEqual(add(-2, 3), 1)\nif __name__ == "__main__": unittest.main()\n')
    (repo/'AGENTS.md').write_text('This is an isolated integration-test fixture. Change only calc.py for the assigned bug and commit it on the ticket branch; verification needs a clean, committed worktree. Run exactly python3 -m unittest (optionally -v) as a standalone Bash command. Inspect source with your available file-reading tool, or a standalone read-only shell command if your provider has no file-reading tool. Do not combine shell commands. Do not push, deploy, contact external services, update external brains, or spawn provider-native agents. Coordinate only through the supplied agent_workspace tools.\n')
    for git_args in [['init'],['add','.'],['-c','user.name=Workspace Test','-c','user.email=test@example.invalid','commit','-m','Fixture']]:
        subprocess.run(['git',*git_args],cwd=repo,check=True,capture_output=True)
    process=subprocess.Popen([str(root/'target/debug/workspace-host'),'--data-dir',str(directory/'host')],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
    def request(command):
        process.stdin.write(json.dumps({'version':1,'id':str(time.monotonic_ns()),'command':command})+'\n');process.stdin.flush()
        response=json.loads(process.stdout.readline())
        if response.get('error'):raise RuntimeError(response['error'])
        return response['result']
    project=None
    try:
        for role, provider in [('project_orchestrator',args.coordinator),('implementer',args.implementer),('tester',args.tester)]:
            profile={'provider':provider,'model':'sonnet' if provider=='claude' else 'gpt-6.1-sol','effort':'low'}
            request({'type':'set_policy','policy':{'role':role,'mode':'fixed','default':profile,'allowed':[profile],'small':profile,'standard':profile,'complex':profile}})
        project=request({'type':'create_project','name':'Addition integration smoke'})
        main=next(s for s in request({'type':'snapshot'})['sessions'] if s['project_id']==project['id'])
        request({'type':'set_verification','verification':{'verifiers':[{'role':'tester','focus':'Tests','provider':args.tester}],'max_rounds':2}})
        request({'type':'attach_repository','project_id':project['id'],'path':str(repo),'base':'HEAD'})
        request({'type':'send','id':'brief','sender':None,'recipient':main['id'],'body':f'Authorized execution smoke. Fix calc.py add(a,b), which incorrectly subtracts. Create exactly one ticket in the fixture repository with acceptance criterion python3 -m unittest passes. Assign one {args.implementer} implementer. On ready_for_testing, call verify_ticket. When verification passes, accept the ticket and reply with its branch. Do not merge, push, deploy, edit external files, create any other tickets, or spawn native subagents. End your turn after delegating; the host resumes you on reports. If a ticket is already accepted, do not repeat work.'})
        start=time.monotonic();last=None
        while time.monotonic()-start<360:
            snap=request({'type':'snapshot'})
            status=[(s['name'],s['status']) for s in snap['sessions']]
            if status!=last:print(json.dumps({'elapsed':round(time.monotonic()-start,1),'sessions':status}),flush=True);last=status
            for attention in snap['attention']:
                if attention['operation_id'].startswith('permission:'):
                    heading, _, raw_input = attention['prompt'].partition('\n')
                    tool_input = json.loads(raw_input)
                    allowed_commands={
                        'python3 -m unittest',
                        'python3 -m unittest -v',
                        'git status --porcelain && echo --- && git diff && echo --- && python3 -m unittest -v',
                    }
                    if not heading.endswith(' requests Bash') or tool_input.get('command') not in allowed_commands:
                        raise RuntimeError('Smoke will not auto-approve unexpected operation: '+attention['prompt'])
                    request({'type':'resolve_attention','id':attention['id'],'answer':'approved'})
                    print(json.dumps({'permission':'approved exact fixture check','command':tool_input['command']}),flush=True)
                else:
                    raise RuntimeError('Smoke needs a human decision: '+attention['prompt'])
            errors=[r['last_error'] for r in snap['runtimes'] if r.get('last_error')]
            if errors:raise RuntimeError(errors)
            if any(t['state']=='accepted' for t in snap['tickets']):
                if any(s['status']=='working' for s in snap['sessions']):
                    time.sleep(.3)
                    continue
                ticket=snap['tickets'][0]
                workers=[s for s in snap['sessions'] if s['role'] in ['implementer','tester']]
                assert len(workers)==2,workers
                assert next(s['provider'] for s in workers if s['role']=='implementer')==args.implementer
                assert next(s['provider'] for s in workers if s['role']=='tester')==args.tester
                # Acceptance removed the worktree; the branch keeps the verified fix.
                fixed=subprocess.run(['git','show',f"{ticket['branch']}:calc.py"],cwd=repo,check=True,text=True,capture_output=True).stdout
                assert 'a + b' in fixed,fixed
                assert (repo/'calc.py').read_text()=='def add(a, b):\n    return a - b\n','Original checkout was changed'
                activity=request({'type':'activity','project_id':project['id'],'before':None,'limit':100})
                timings=[a['detail'] for a in activity if a['kind']=='turn_scheduled']
                print(json.dumps({'result':'verified and accepted by the coordinator','providers':vars(args),'elapsed_seconds':round(time.monotonic()-start,2),'ticket':ticket,'handoff_timings':timings}),flush=True)
                break
            time.sleep(.3)
        else:
            for session in snap['sessions']:
                print(json.dumps({'session':session['name'],'messages':request({'type':'messages','session_id':session['id'],'before':None,'limit':30})}),flush=True)
            raise TimeoutError('Ticket did not complete in six minutes')
    finally:
        if project:request({'type':'set_live','project_id':project['id'],'enabled':False})
        process.stdin.close();process.wait(timeout=10)
