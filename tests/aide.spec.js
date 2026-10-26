import {test, expect} from '@playwright/test';
import {mkdtempSync, mkdirSync, writeFileSync, readFileSync, existsSync, unlinkSync} from 'node:fs';
import {join, resolve} from 'node:path';
import {spawn, execFileSync} from 'node:child_process';

const binary=resolve('target/debug/aide');
function fixture(){
 const dir=mkdtempSync('/tmp/aide-e2e-'),root=join(dir,'Cat Herder'),home=join(dir,'home'),source=join(dir,'source');
 mkdirSync(root);mkdirSync(source);
 const env={...process.env,AIDE_HOME:home,AIDE_FIXTURE_REAL_JJ:execFileSync('which',['jj'],{encoding:'utf8'}).trim(),AIDE_TMUX_SOCKET:`aide-test-${dir.split('-').at(-1)}`,AIDE_CODEX:resolve('tests/aide-fixtures/codex'),AIDE_FIXTURE_RECORDINGS:join(dir,'native'),PATH:`${resolve('tests/aide-fixtures')}:${process.env.PATH}`};
 const run=(program,args,cwd=root)=>execFileSync(program,args,{cwd,env,encoding:'utf8',stdio:['ignore','pipe','pipe']});
 run('git',['init','-b','main'],source);writeFileSync(join(source,'hello.txt'),'trunk\n');run('git',['add','.'],source);run('git',['-c','user.name=Test','-c','user.email=test@example.com','commit','-m','initial'],source);
 run('git',['clone',source,'repo']);run(binary,['init','.','--repo','./repo']);
 return {dir,root,home,env,run};
}
async function manager(f){
 const child=spawn(binary,['--no-open'],{cwd:f.root,env:f.env,stdio:['ignore','pipe','pipe']});let log='';
 const url=await new Promise((resolve,reject)=>{
  const timer=setTimeout(()=>reject(new Error(log)),15000);
  child.stdout.on('data',s=>{log+=s;const m=log.match(/Aide: (http:\/\/127\.0\.0\.1:\d+)/);if(m){clearTimeout(timer);resolve(m[1]);}});
  child.stderr.on('data',s=>log+=s);child.on('exit',c=>{clearTimeout(timer);reject(new Error(`exit ${c}: ${log}`));});
 });return {child,url};
}
async function stop(server,signal='SIGINT'){if(server.child.exitCode!==null||server.child.signalCode!==null)return;await new Promise(r=>{server.child.once('exit',r);server.child.kill(signal);});}
function taskRow(page,id){return page.locator('.task').filter({has:page.getByRole('button',{name:new RegExp(id)})});}
function cleanup(f){try{f.run('tmux',['-L',f.env.AIDE_TMUX_SOCKET,'kill-server']);}catch{}}

test('task creation, native status, focus, confirmation and retained history',async({page})=>{
 const f=fixture();const server=await manager(f);
 try{
  await page.goto(server.url);await expect(page.getByRole('button',{name:'Cat Herder',exact:true})).toBeVisible();
  await page.getByRole('button',{name:'New task'}).click();await expect(page.getByRole('combobox',{name:'Repository'})).toHaveValue('repo');await page.getByLabel('Task ID').fill('bad task');expect(await page.getByLabel('Task ID').evaluate(el=>el.checkValidity())).toBe(false);await page.getByLabel('Task ID').fill('task-1234');await page.getByRole('button',{name:'Start and open Codex'}).click();
  const row=taskRow(page,'task-1234');await expect(row).toBeVisible();await row.getByRole('button',{name:/task-1234/}).click();
  await expect(row).toContainText('Fixture conversation',{timeout:25000});await expect(row).toContainText('Conversation idle');
  const workspace=join(f.root,'repo-tasks','task-1234');expect(readFileSync(join(workspace,'hello.txt'),'utf8')).toBe('trunk\n');
  writeFileSync(join(workspace,'.fixture-status'),JSON.stringify({type:'active',activeFlags:['waitingOnApproval']}));
  await expect(page.getByRole('heading',{name:'Requires attention'})).toBeVisible();await expect(row).toContainText('Codex needs your input');
  await page.getByRole('button',{name:'Cat Herder',exact:true}).click();await expect(row).toBeVisible();
  // A status transition may remount the row in another group.
  if(!await row.getByRole('button',{name:'Close task'}).isVisible())await row.getByRole('button',{name:/task-1234/}).click();
  page.once('dialog',async d=>{expect(d.message()).toContain('session');await d.dismiss();});await row.getByRole('button',{name:'Close task'}).click();expect(existsSync(workspace)).toBe(true);
  page.once('dialog',d=>d.accept());await row.getByRole('button',{name:'Close task'}).click();await expect(row).toHaveCount(0,{timeout:20000});expect(existsSync(workspace)).toBe(false);
  await page.getByLabel('Show completed').check();await expect(row).toBeVisible();await row.getByRole('button',{name:/task-1234/}).click();await expect(row).toContainText('recordings');
  expect(f.run(binary,['task','list','--completed'])).toContain('Completed');
 }finally{await stop(server);cleanup(f);}
});

test('repository defaults and selection survive refreshes; the full task title toggles accessibly',async({page})=>{
 const f=fixture();
 f.run('git',['clone',join(f.dir,'source'),'second']);
 f.run(binary,['init','.','--repo','./second']);
 const server=await manager(f);
 try{
  await page.goto(server.url);
  const newTask=page.getByRole('button',{name:'New task'});
  await newTask.click();
  const repository=page.getByRole('combobox',{name:'Repository'});
  await expect(repository).toHaveValue('repo');
  await repository.selectOption('second');
  // Periodic snapshots must not overwrite an explicit selection.
  await page.waitForTimeout(2500);
  await expect(repository).toHaveValue('second');
  await newTask.click();await newTask.click();
  await expect(repository).toHaveValue('second');
  const id='a-task-with-a-long-title-to-check-narrow-screen-layout';
  await page.getByLabel('Task ID').fill(id);
  await page.getByRole('button',{name:'Start and open Codex'}).click();
  const row=taskRow(page,id),title=row.getByRole('button',{name:new RegExp(id)});
  await expect(row).toContainText('Fixture conversation',{timeout:25000});
  expect(existsSync(join(f.root,'second-tasks',id,'hello.txt'))).toBe(true);
  await expect(title).toHaveAttribute('aria-expanded','false');
  const titleBox=await title.boundingBox(),headingBox=await row.locator('.task-heading').boundingBox();
  expect(titleBox.height).toBeGreaterThanOrEqual(44);
  expect(titleBox.width).toBeGreaterThan(headingBox.width*.65);
  // Click the far edge, away from the chevron and task ID.
  await title.click({position:{x:titleBox.width-12,y:titleBox.height/2}});
  await expect(title).toHaveAttribute('aria-expanded','true');
  await expect(row.getByRole('button',{name:'Copy path'})).toBeVisible();
  await title.press('Enter');await expect(title).toHaveAttribute('aria-expanded','false');
  await title.press('Space');await expect(title).toHaveAttribute('aria-expanded','true');
  await row.getByRole('button',{name:'Open terminal'}).click();
  await expect(page.getByRole('status')).toContainText('Opened terminal');
  await expect(title).toHaveAttribute('aria-expanded','true');
  await page.setViewportSize({width:390,height:844});
  await expect(row.getByRole('button',{name:'Close task'})).toBeVisible();
  expect(await page.evaluate(()=>document.documentElement.scrollWidth<=window.innerWidth)).toBe(true);
  await title.click();await expect(title).toHaveAttribute('aria-expanded','false');
  await title.click();await expect(title).toHaveAttribute('aria-expanded','true');
 }finally{await stop(server);cleanup(f);}
});

test('dashboard loss preserves terminals; reboot-equivalent recovery resumes same conversation idle',async({page})=>{
 const f=fixture();let server=await manager(f);
 try{
  f.run(binary,['task','start','restore-me','--workspace-only']);
  f.run(binary,['task','recover','restore-me']);
  await page.goto(server.url);const row=taskRow(page,'restore-me');await expect(row).toContainText('Fixture conversation',{timeout:25000});
  await row.getByRole('button',{name:/restore-me/}).click();
  const thread=await row.locator('dd').first().textContent();
  const session=f.run('tmux',['-L',f.env.AIDE_TMUX_SOCKET,'list-sessions','-F','#{session_name}']).trim();
  const pid=f.run('tmux',['-L',f.env.AIDE_TMUX_SOCKET,'display-message','-p','-t',session,'#{pane_pid}']).trim();
  await stop(server,'SIGKILL');expect(f.run('tmux',['-L',f.env.AIDE_TMUX_SOCKET,'display-message','-p','-t',session,'#{pane_pid}']).trim()).toBe(pid);
  server=await manager(f);await page.goto(server.url);await expect(row).toContainText('Fixture conversation');
  expect(f.run('tmux',['-L',f.env.AIDE_TMUX_SOCKET,'display-message','-p','-t',session,'#{pane_pid}']).trim()).toBe(pid);
  await stop(server);f.run('tmux',['-L',f.env.AIDE_TMUX_SOCKET,'kill-session','-t',session]);
  server=await manager(f);await page.goto(server.url);await expect(row).toContainText('Recovered conversation',{timeout:25000});await expect(row).toContainText('Fixture conversation',{timeout:25000});
  await row.getByRole('button',{name:/restore-me/}).click();await expect(row.locator('dd').first()).toHaveText(thread);
 }finally{await stop(server);cleanup(f);}
});

test('review entry points forward options and task workspace without a manager',async()=>{
 const f=fixture();
 try{
  f.run(binary,['task','start','review-me','--workspace-only']);
  const output=f.run(binary,['task','review','review-me','--help']);expect(output).toContain('guided review');
  expect(f.run(binary,['review','--help'])).toContain('guided review');
  expect(()=>f.run(binary,['task','review','review-me','--path','/tmp'])).toThrow();
  expect(()=>f.run(binary,['task','cleanup','review-me'])).toThrow();
  expect(existsSync(join(f.root,'repo-tasks','review-me'))).toBe(true);
 }finally{cleanup(f);}
});

async function closeInTerminal(f,id,answer){
 const child=spawn('python3',[resolve('tests/aide-pty.py'),binary,'task','cleanup',id],{cwd:f.root,env:f.env,stdio:['pipe','pipe','pipe']});
 let output='',sent=false;
 return await new Promise((resolve,reject)=>{
  const timer=setTimeout(()=>{child.kill();reject(new Error(output));},15000);
  child.stdout.on('data',data=>{output+=data;if(!sent&&output.includes('to confirm:')){sent=true;child.stdin.write(answer+'\n');}});
  child.stderr.on('data',data=>output+=data);
  child.on('exit',()=>{clearTimeout(timer);resolve(output);});
 });
}

test('CLI closure confirms before deletion and preserves completed history',async()=>{
 const f=fixture();
 try{
  f.run(binary,['task','start','close-me','--workspace-only']);
  const path=join(f.root,'repo-tasks','close-me');
  expect(await closeInTerminal(f,'close-me','no')).toContain('Cancelled');expect(existsSync(path)).toBe(true);
  expect(await closeInTerminal(f,'close-me','close-me')).toContain('Closed close-me');expect(existsSync(path)).toBe(false);
  expect(f.run(binary,['task','list','--completed'])).toContain('Completed');
 }finally{cleanup(f);}
});

function runtimeTools(f,id){
 const tmux=(...args)=>f.run('tmux',['-L',f.env.AIDE_TMUX_SOCKET,...args]).trim();
 const session=tmux('list-sessions','-F','#{session_name}').split('\n')[0];
 const pane=role=>tmux('show-options','-qv','-t',session,`@aide-${role}`);
 const capture=target=>tmux('capture-pane','-p','-t',target);
 const send=async(target,text)=>{tmux('send-keys','-t',target,'-l',text);await new Promise(r=>setTimeout(r,300));tmux('send-keys','-t',target,'Enter');};
 const backend=()=>JSON.parse(readFileSync(join(f.root,'repo-tasks',id,'.fixture-backend.json'),'utf8'));
 return {tmux,session,pane,capture,send,backend};
}

test('independent backend, shell environment repair, context checks and managed-window recovery',async({page})=>{
 test.setTimeout(90000);
 const f=fixture();f.env.AIDE_TEST_CHANGED='old';f.env.AIDE_TEST_REMOVED='stale';
 let server=await manager(f);
 try{
  f.run(binary,['task','start','runtime','--workspace-only']);f.run(binary,['task','recover','runtime']);
  const r=runtimeTools(f,'runtime'),work=r.pane('work'),backendPane=r.pane('backend');
  await expect.poll(()=>r.capture(work)).toContain('Fixture Codex conversation ready');
  const original=r.backend().pid;
  r.tmux('rename-window','-t',backendPane,'renamed backend');
  const extra=r.tmux('new-window','-d','-P','-F','#{pane_id}','-t',r.session,'-n','my shell','fish');
  const extraPid=r.tmux('display-message','-p','-t',extra,'#{pane_pid}');
  const workspace=join(f.root,'repo-tasks','runtime');
  await page.goto(server.url);const row=taskRow(page,'runtime');await expect(row).toContainText('Fixture conversation');
  await stop(server);
  r.tmux('send-keys','-t',work,'C-d');
  await expect.poll(()=>r.tmux('display-message','-p','-t',work,'#{pane_current_command}')).toBe('fish');
  expect(r.backend().pid).toBe(original);process.kill(original,0);
  // Same backend and same conversation, even from another window/outside cwd.
  await r.send(extra,`cd /tmp; set -e AIDE_HOME; '${binary}' codex resume`);
  await expect.poll(()=>r.capture(extra)).toContain('Fixture Codex conversation ready');
  expect(r.backend().pid).toBe(original);
  r.tmux('send-keys','-t',extra,'C-d');
  await expect.poll(()=>r.tmux('display-message','-p','-t',extra,'#{pane_current_command}')).toBe('fish');
  expect(()=>execFileSync(binary,['codex','resume'],{cwd:workspace,env:{...f.env,TMUX_PANE:''},stdio:'pipe'})).toThrow();
  f.run(binary,['task','start','other','--workspace-only']);
  await r.send(extra,`cd '${join(f.root,'repo-tasks','other')}'; '${binary}' --home '${f.home}' codex resume`);
  await expect.poll(()=>r.capture(extra)).toContain('Current directory belongs to task other');
  r.tmux('send-keys','-t',backendPane,'C-c');
  await expect.poll(()=>r.tmux('display-message','-p','-t',backendPane,'#{pane_dead}')).toBe('1');
  await r.send(work,`'${binary}' --home '${f.home}' codex resume`);
  await expect.poll(()=>r.capture(work)).toContain('Start aide dashboard mode first');
  expect(r.tmux('display-message','-p','-t',backendPane,'#{pane_dead}')).toBe('1');
  server=await manager(f);
  expect(r.tmux('display-message','-p','-t',backendPane,'#{pane_dead}')).toBe('1');
  await r.send(work,`set -gx AIDE_TEST_CHANGED new; set -e AIDE_TEST_REMOVED; set -gx AIDE_TEST_ADDED added; '${binary}' --home '${f.home}' codex resume`);
  await expect.poll(()=>r.backend().pid).not.toBe(original);
  expect(r.backend().environment).toEqual({AIDE_TEST_CHANGED:'new',AIDE_TEST_ADDED:'added'});
  // Removing the managed backend window is repaired without touching user work.
  r.tmux('kill-window','-t',backendPane);
  f.run(binary,['task','attach','runtime','--new-window']);
  expect(r.pane('backend')).not.toBe(backendPane);
  expect(r.tmux('display-message','-p','-t',extra,'#{pane_pid}')).toBe(extraPid);
  expect(r.pane('work')).toBe(work);
  const backendPid=r.backend().pid;
  r.tmux('set-option','-p','-t',work,'remain-on-exit','on');
  r.tmux('respawn-pane','-k','-t',work,'true');
  await expect.poll(()=>r.tmux('display-message','-p','-t',work,'#{pane_dead}')).toBe('1');
  f.run(binary,['task','attach','runtime','--new-window']);
  await expect.poll(()=>r.tmux('display-message','-p','-t',work,'#{pane_dead}')).toBe('0');
  expect(r.pane('work')).toBe(work);expect(r.backend().pid).toBe(backendPid);
  expect(r.tmux('display-message','-p','-t',extra,'#{pane_pid}')).toBe(extraPid);
  // Recovery refuses to replace an unrelated foreground program in a managed pane.
  const replacement=r.pane('backend');
  r.tmux('send-keys','-t',replacement,'C-c');
  await expect.poll(()=>r.tmux('display-message','-p','-t',replacement,'#{pane_dead}')).toBe('1');
  r.tmux('respawn-pane','-t',replacement,'sleep','120');
  expect(()=>f.run(binary,['task','recover','runtime'])).toThrow(/unresponsive/);
  expect(r.tmux('display-message','-p','-t',replacement,'#{pane_current_command}')).toBe('sleep');
 }finally{await stop(server);cleanup(f);}
});

test('stored conversation remains authoritative when another thread appears in the same workspace',async({page})=>{
 const f=fixture();let server=await manager(f);
 try{
  f.run(binary,['task','start','bound','--workspace-only']);f.run(binary,['task','recover','bound']);
  await page.goto(server.url);const row=taskRow(page,'bound');await expect(row).toContainText('Fixture conversation');
  await row.getByRole('button',{name:/bound/}).click();
  const identity=await row.locator('dd').first().textContent();
  const workspace=join(f.root,'repo-tasks','bound');
  writeFileSync(join(workspace,'.fixture-other-thread'),'yes');
  // Allow multiple observation cycles. The old implementation silently rebound
  // to whichever same-directory conversation appeared first in the loaded list.
  await page.waitForTimeout(2500);
  await expect(row).toContainText('Fixture conversation');
  await expect(row.locator('dd').first()).toHaveText(identity);
  const r=runtimeTools(f,'bound');r.tmux('send-keys','-t',r.pane('work'),'C-d');
  await r.send(r.pane('work'),`'${binary}' codex resume`);
  await page.waitForTimeout(1500);
  await expect(row.locator('dd').first()).toHaveText(identity);
  await stop(server);r.tmux('kill-session','-t',r.session);
  server=await manager(f);await page.goto(server.url);await expect(row).toContainText('Fixture conversation');
  await row.getByRole('button',{name:/bound/}).click();await expect(row.locator('dd').first()).toHaveText(identity);
 }finally{await stop(server);cleanup(f);}
});

test('ambiguous initial conversations require attention instead of choosing one',async({page})=>{
 const f=fixture();const server=await manager(f);
 try{
  f.run(binary,['task','start','ambiguous','--workspace-only']);
  writeFileSync(join(f.root,'repo-tasks','ambiguous','.fixture-other-thread'),'yes');
  f.run(binary,['task','recover','ambiguous']);
  await page.goto(server.url);const row=taskRow(page,'ambiguous');
  await expect(row).toContainText('Multiple conversations found',{timeout:15000});
  await row.getByRole('button',{name:/ambiguous/}).click();
  await expect(row.locator('dd').first()).toHaveText('Not started');
 }finally{await stop(server);cleanup(f);}
});

test('slow repository work leaves one dashboard responsive; shared repo and task transitions stay locked',async({page})=>{
 test.setTimeout(90000);
 const f=fixture(),gates=join(f.dir,'gates');mkdirSync(gates);f.env.AIDE_FIXTURE_FETCH_GATES=gates;
 f.run('git',['clone',join(f.dir,'source'),'second']);f.run(binary,['init','.','--repo','./second']);
 f.run(binary,['task','start','healthy','--repo','repo','--workspace-only']);
 const server=await manager(f);
 try{
  f.run(binary,['task','recover','healthy']);
  await page.goto(server.url);const healthy=taskRow(page,'healthy');await expect(healthy).toContainText('Fixture conversation');
  await page.getByRole('button',{name:'New task'}).click();
  const start=async(id,repo)=>{await page.getByLabel('Task ID').fill(id);await page.getByRole('combobox').selectOption(repo);await page.getByRole('button',{name:'Start and open Codex'}).click();};
  writeFileSync(join(gates,'repo.hold'),'hold');
  await start('slow-first','repo');await expect.poll(()=>existsSync(join(gates,'repo.entered'))).toBe(true);
  // The CLI shares the same task lock, even before task creation reaches SQLite.
  expect(()=>f.run(binary,['task','start','slow-first','--repo','repo','--workspace-only'])).toThrow(/busy/);
  // Another command on this very browser connection must complete before release.
  await start('fast','second');await expect(taskRow(page,'fast')).toContainText('Fixture conversation',{timeout:15000});
  await healthy.getByRole('button',{name:'Open terminal'}).click();await expect(page.getByRole('status')).toContainText('Opened terminal');
  await start('slow-second','repo');
  await expect.poll(()=>existsSync(join(f.root,'.aide/task-locks/slow-second.lock'))).toBe(true);
  expect(readFileSync(join(gates,'repo.entered'),'utf8').trim().split('\n')).toHaveLength(1);
  // Cleanup holds its task lock while waiting for shared VCS access.
  await healthy.getByRole('button',{name:/healthy/}).click();
  page.once('dialog',d=>d.accept());await healthy.getByRole('button',{name:'Close task'}).click();
  await expect(healthy).toContainText('Closing');
  expect(()=>f.run(binary,['task','attach','healthy','--new-window'])).toThrow(/busy/);
  await page.reload();await expect(taskRow(page,'fast')).toContainText('Fixture conversation');
  unlinkSync(join(gates,'repo.hold'));
  await expect(taskRow(page,'slow-first')).toContainText('Fixture conversation',{timeout:25000});
  await expect(taskRow(page,'slow-second')).toContainText('Fixture conversation',{timeout:25000});
  await expect(healthy).toHaveCount(0,{timeout:25000});
  expect(existsSync(join(f.root,'repo-tasks','healthy'))).toBe(false);
  await page.getByLabel('Show completed').check();await expect(healthy).toContainText('Completed');
  expect(()=>f.run(binary,['task','recover','healthy'])).toThrow(/open/);
 }finally{if(existsSync(join(gates,'repo.hold')))unlinkSync(join(gates,'repo.hold'));await stop(server);cleanup(f);}
});
