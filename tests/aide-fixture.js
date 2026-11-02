import {mkdtempSync, mkdirSync, writeFileSync, readFileSync, existsSync, unlinkSync, realpathSync} from 'node:fs';
import {join, resolve} from 'node:path';
import {spawn, execFileSync} from 'node:child_process';

export const binary=resolve('target/debug/aide');
export function fixture(){
 const dir=mkdtempSync('/tmp/aide-e2e-'),root=join(dir,'Cat Herder'),home=join(dir,'home'),source=join(dir,'source');
 mkdirSync(root);mkdirSync(source);
 const env={...process.env,AIDE_HOME:home,AIDE_FIXTURE_REAL_JJ:execFileSync('which',['jj'],{encoding:'utf8'}).trim(),AIDE_TMUX_SOCKET:`aide-test-${dir.split('-').at(-1)}`,AIDE_CODEX:resolve('tests/aide-fixtures/codex'),AIDE_FIXTURE_RECORDINGS:join(dir,'native'),PATH:`${resolve('tests/aide-fixtures')}:${process.env.PATH}`};
 const run=(program,args,cwd=root)=>execFileSync(program,args,{cwd,env,encoding:'utf8',stdio:['ignore','pipe','pipe']});
 run('git',['init','-b','main'],source);writeFileSync(join(source,'hello.txt'),'trunk\n');run('git',['add','.'],source);run('git',['-c','user.name=Test','-c','user.email=test@example.com','commit','-m','initial'],source);
 run('git',['clone',source,'repo']);run(binary,['init','.','--repo','./repo']);
 return {dir,root,home,env,run};
}
export async function manager(f){
 const child=spawn(binary,['--no-open'],{cwd:f.root,env:f.env,stdio:['ignore','pipe','pipe']});let log='';
 const url=await new Promise((resolve,reject)=>{
  const timer=setTimeout(()=>reject(new Error(log)),15000);
  child.stdout.on('data',s=>{log+=s;const m=log.match(/Aide: (http:\/\/127\.0\.0\.1:\d+)/);if(m){clearTimeout(timer);resolve(m[1]);}});
  child.stderr.on('data',s=>log+=s);child.on('exit',c=>{clearTimeout(timer);reject(new Error(`exit ${c}: ${log}`));});
 });return {child,url};
}
export async function stop(server,signal='SIGINT'){if(server.child.exitCode!==null||server.child.signalCode!==null)return;await new Promise(r=>{server.child.once('exit',r);server.child.kill(signal);});}
export function taskRow(page,id){return page.locator('.task').filter({has:page.getByRole('button',{name:new RegExp(id)})});}
export function cleanup(f){try{f.run('tmux',['-L',f.env.AIDE_TMUX_SOCKET,'kill-server']);}catch{}}

