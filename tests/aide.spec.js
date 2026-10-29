import {test, expect} from '@playwright/test';
import {mkdtempSync, mkdirSync, writeFileSync, readFileSync, existsSync, unlinkSync, realpathSync} from 'node:fs';
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
  await expect(row).toContainText('Fixture conversation',{timeout:25000});await expect(row.locator('.task-status')).toHaveAttribute('title','Conversation idle');
  await expect(page).toHaveTitle('(0·1) Aide de camp');
  const workspace=join(f.root,'repo-tasks','task-1234');expect(readFileSync(join(workspace,'hello.txt'),'utf8')).toBe('trunk\n');
  writeFileSync(join(workspace,'.fixture-status'),JSON.stringify({type:'active',activeFlags:['waitingOnApproval']}));
  await expect(page.getByRole('heading',{name:'Requires attention'})).toBeVisible();await expect(row.locator('.task-status')).toHaveAttribute('title',/Codex needs your input/);
  await expect(page).toHaveTitle('(1·0) Aide de camp');
  await page.getByRole('button',{name:'Cat Herder',exact:true}).click();await expect(row).toBeVisible();
  await expect(page).toHaveTitle('(1·0) Aide de camp');
  // A status transition may remount the row in another group.
  if(!await row.getByRole('button',{name:'Close task'}).isVisible())await row.getByRole('button',{name:/task-1234/}).click();
  page.once('dialog',async d=>{expect(d.message()).toContain('session');await d.dismiss();});await row.getByRole('button',{name:'Close task'}).click();expect(existsSync(workspace)).toBe(true);
  page.once('dialog',d=>d.accept());await row.getByRole('button',{name:'Close task'}).click();await expect(row).toHaveCount(0,{timeout:20000});expect(existsSync(workspace)).toBe(false);
  await expect(page).toHaveTitle('Aide de camp');
  await page.getByLabel('Show completed').check();await expect(row).toBeVisible();await row.getByRole('button',{name:/task-1234/}).click();await expect(row).toContainText('recordings');
  expect(f.run(binary,['task','list','--completed'])).toContain('Completed');
  await expect(page).toHaveTitle('Aide de camp');
 }finally{await stop(server);cleanup(f);}
});

test('startup stays neutral through preparation, backend launch and reload',async({page})=>{
 const f=fixture(),prepare=join(f.dir,'prepare.hold'),startup=join(f.dir,'startup.hold');
 f.env.AIDE_FIXTURE_PREPARE_GATE=prepare;f.env.AIDE_FIXTURE_STARTUP_GATE=startup;
 writeFileSync(prepare,'hold');writeFileSync(startup,'hold');
 const server=await manager(f),statuses=[];
 page.on('websocket',ws=>ws.on('framereceived',({payload})=>{
  const event=JSON.parse(payload.toString());
  if(event.type==='snapshot')for(const project of event.projects)for(const task of project.tasks)statuses.push(task.status);
 }));
 try{
  await page.goto(server.url);
  await page.getByRole('button',{name:'New task'}).click();
  await page.getByLabel('Task ID').fill('starting');
  await page.getByRole('button',{name:'Start and open Codex'}).click();
  const row=taskRow(page,'starting');
  await expect(row.locator('.task-status')).toHaveText('Starting');
  await expect(row.locator('.task-status')).toHaveAttribute('title','Preparing workspace');
  await expect(page).toHaveTitle('Aide de camp');
  await row.getByRole('button',{name:/starting/}).click();
  await expect(row.getByRole('button',{name:'Open terminal',exact:true})).toHaveCount(0);
  await expect(row.getByRole('button',{name:'Recover session'})).toHaveCount(0);
  await expect(row.getByRole('button',{name:'Close task'})).toHaveCount(0);
  await page.reload();
  await expect(row.locator('.task-status')).toHaveText('Starting');
  unlinkSync(prepare);
  await expect.poll(()=>existsSync(join(f.dir,'startup.entered'))).toBe(true);
  await expect(row.locator('.task-status')).toHaveAttribute('title','Starting Codex');
  await expect(page).toHaveTitle('Aide de camp');
  expect(f.run(binary,['task','list'])).toContain('Starting');
  await page.reload();
  await expect(row.locator('.task-status')).toHaveText('Starting');
  unlinkSync(startup);
  await expect(row.locator('.task-status')).toHaveText('Ready');
  await expect(row.getByRole('button',{name:'Open terminal',exact:true})).toBeVisible();
  await expect(page).toHaveTitle('(0·1) Aide de camp');
  expect(statuses).toContain('starting');expect(statuses).not.toContain('attention');
 }finally{for(const gate of [prepare,startup])if(existsSync(gate))unlinkSync(gate);await stop(server);cleanup(f);}
});

test('failed backend startup requires attention and recovers',async({page})=>{
 const f=fixture(),gate=join(f.dir,'startup.hold');
 f.env.AIDE_FIXTURE_STARTUP_GATE=gate;
 writeFileSync(gate,'hold');writeFileSync(join(f.dir,'startup.fail'),'fail');
 const server=await manager(f);
 try{
  await page.goto(server.url);
  await page.getByRole('button',{name:'New task'}).click();
  await page.getByLabel('Task ID').fill('failed-start');
  await page.getByRole('button',{name:'Start and open Codex'}).click();
  const row=taskRow(page,'failed-start');
  await expect(row.locator('.task-status')).toHaveText('Starting');
  unlinkSync(gate);
  await expect(row.locator('.task-status')).toHaveText('Requires attention');
  await expect(row.locator('.task-status')).toHaveAttribute('title',/Startup failed.*Retry Recover session/);
  await expect(page).toHaveTitle('(1·0) Aide de camp');
  await page.reload();
  await expect(row.locator('.task-status')).toHaveText('Requires attention');
  unlinkSync(join(f.dir,'startup.fail'));
  await row.getByRole('button',{name:/failed-start/}).click();
  await row.getByRole('button',{name:'Open terminal',exact:true}).click();
  await expect(row.locator('.task-status')).toHaveText('Ready');
  await expect(page).toHaveTitle('(0·1) Aide de camp');
 }finally{if(existsSync(gate))unlinkSync(gate);await stop(server);cleanup(f);}
});

test('manager interruption during startup requires attention after restart and can recover',async({page})=>{
 const f=fixture(),gate=join(f.dir,'startup.hold');
 f.env.AIDE_FIXTURE_STARTUP_GATE=gate;writeFileSync(gate,'hold');
 let server=await manager(f);
 try{
  await page.goto(server.url);
  await page.getByRole('button',{name:'New task'}).click();
  await page.getByLabel('Task ID').fill('interrupted-start');
  await page.getByRole('button',{name:'Start and open Codex'}).click();
  const row=taskRow(page,'interrupted-start');
  await expect.poll(()=>existsSync(join(f.dir,'startup.entered'))).toBe(true);
  await expect(row.locator('.task-status')).toHaveText('Starting');
  await stop(server,'SIGKILL');
  unlinkSync(gate);
  server=await manager(f);
  await page.goto(server.url);
  await expect(row.locator('.task-status')).toHaveText('Requires attention');
  await expect(row.locator('.task-status')).toHaveAttribute('title',/Startup interrupted.*Recover session/);
  await expect(page).toHaveTitle('(1·0) Aide de camp');
  await row.getByRole('button',{name:'Open terminal',exact:true}).click();
  await expect(row.locator('.task-status')).toHaveText('Ready');
  await expect(page).toHaveTitle('(0·1) Aide de camp');
 }finally{if(existsSync(gate))unlinkSync(gate);await stop(server);cleanup(f);}
});

test('failed preparation remains visible and can retry the same task ID',async({page})=>{
 const f=fixture(),gate=join(f.dir,'prepare.hold');
 f.env.AIDE_FIXTURE_PREPARE_GATE=gate;
 writeFileSync(gate,'hold');writeFileSync(join(f.dir,'prepare.fail'),'fail');
 const server=await manager(f);
 try{
  await page.goto(server.url);
  await page.getByRole('button',{name:'New task'}).click();
  await page.getByLabel('Task ID').fill('retry-start');
  await page.getByRole('button',{name:'Start and open Codex'}).click();
  const row=taskRow(page,'retry-start');
  await expect(row.locator('.task-status')).toHaveText('Starting');
  unlinkSync(gate);
  await expect(row.locator('.task-status')).toHaveText('Requires attention');
  await expect(row.locator('.task-status')).toHaveAttribute('title',/Fixture preparation failure/);
  await page.reload();
  await expect(row.locator('.task-status')).toHaveText('Requires attention');
  unlinkSync(join(f.dir,'prepare.fail'));
  await page.getByRole('button',{name:'New task'}).click();
  await page.getByLabel('Task ID').fill('retry-start');
  await page.getByRole('button',{name:'Start and open Codex'}).click();
  await expect(row.locator('.task-status')).toHaveText('Ready');
 }finally{if(existsSync(gate))unlinkSync(gate);await stop(server);cleanup(f);}
});

test('closing retains its title without attention, and interrupted cleanup can be retried',async({page})=>{
 const f=fixture(),gate=join(f.dir,'cleanup.hold');
 f.env.AIDE_FIXTURE_CLEANUP_GATE=gate;
 const server=await manager(f);
 try{
  f.run(binary,['task','start','closing','--workspace-only']);
  f.run(binary,['task','recover','closing']);
  f.run(binary,['task','attach','closing','--new-window']);
  await page.goto(server.url);
  const row=taskRow(page,'closing');
  await expect(row.locator('.task-summary')).toHaveText('Fixture conversation');
  await row.getByRole('button',{name:/closing/}).click();
  writeFileSync(gate,'hold');writeFileSync(join(f.dir,'cleanup.fail'),'fail');
  page.once('dialog',d=>d.accept());await row.getByRole('button',{name:'Close task'}).click();
  await expect.poll(()=>existsSync(join(f.dir,'cleanup.entered'))).toBe(true);
  await expect(row.locator('.task-status')).toHaveText('Closing');
  await expect(row.locator('.task-summary')).toHaveText('Fixture conversation');
  await expect(page).toHaveTitle('Aide de camp');
  await expect(row.getByRole('button',{name:'Open terminal',exact:true})).toHaveCount(0);
  await page.reload();
  await expect(row.locator('.task-status')).toHaveText('Closing');
  await expect(row.locator('.task-summary')).toHaveText('Fixture conversation');
  unlinkSync(gate);
  await expect(row.locator('.task-status')).toHaveText('Requires attention');
  await expect(row.locator('.task-status')).toHaveAttribute('title',/Cleanup interrupted/);
  await expect(page).toHaveTitle('(1·0) Aide de camp');
  unlinkSync(join(f.dir,'cleanup.fail'));
  await row.getByRole('button',{name:/closing/}).click();
  page.once('dialog',d=>d.accept());await row.getByRole('button',{name:'Close task'}).click();
  await expect(row).toHaveCount(0);
  await page.getByLabel('Show completed').check();
  await expect(row.locator('.task-summary')).toHaveText('Fixture conversation');
  await expect(row.locator('.task-status')).toHaveText('Completed');
 }finally{if(existsSync(gate))unlinkSync(gate);await stop(server);cleanup(f);}
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
  await expect(page.getByLabel('Task ID')).toHaveCount(0);
  await expect(newTask).toHaveAttribute('aria-expanded','false');
  await newTask.click();
  await expect(page.getByLabel('Task ID')).toHaveValue(id);
  await expect(repository).toHaveValue('second');
  await newTask.click();
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

test('start submission closes the form; toasts expire, replace timers and distinguish outcomes',async({page})=>{
 const f=fixture(),server=await manager(f);
 let socket;
 const commands=[];
 try{
  // Keep real project snapshots but control command outcomes and timing.
  await page.routeWebSocket('**/ws',ws=>{
   socket=ws;
   const backend=ws.connectToServer();
   backend.onMessage(message=>ws.send(message));
   ws.onMessage(message=>commands.push(JSON.parse(message)));
  });
  await page.addInitScript(()=>{
   const send=WebSocket.prototype.send;
   WebSocket.prototype.send=function(data){
    if(window.failSend)throw new DOMException('Connection lost','InvalidStateError');
    return send.call(this,data);
   };
  });
  await page.clock.install();
  await page.clock.pauseAt(new Date());
  await page.goto(server.url);
  const newTask=page.getByRole('button',{name:'New task'});
  const toast=page.getByRole('status');
  await expect(toast).toBeHidden();
  await newTask.click();
  await page.getByLabel('Task ID').fill('toast-task');
  await page.evaluate(()=>window.failSend=true);
  await page.getByRole('button',{name:'Start and open Codex'}).click();
  await expect(page.getByLabel('Task ID')).toHaveValue('toast-task');
  await expect(toast).toHaveText('Dashboard disconnected. Reconnect before trying again.');
  await expect(toast).toHaveAttribute('data-kind','error');
  expect(commands).toHaveLength(0);
  await page.evaluate(()=>window.failSend=false);
  await page.getByRole('button',{name:'Start and open Codex'}).click();
  await expect(page.getByLabel('Task ID')).toHaveCount(0);
  await expect(newTask).toHaveAttribute('aria-expanded','false');
  await expect.poll(()=>commands.length).toBe(1);
  expect(commands[0]).toMatchObject({action:'start',id:'toast-task',repository:'repo',open_terminal:true});
  await expect(toast).toHaveText('Working…');
  await expect(toast).toHaveAttribute('data-kind','progress');
  const progressColour=await toast.evaluate(el=>getComputedStyle(el).borderLeftColor);
  await page.clock.fastForward(4000);
  socket.send(JSON.stringify({type:'result',message:'Started toast-task',error:false}));
  await expect(toast).toHaveText('Started toast-task');
  await expect(toast).toHaveAttribute('data-kind','success');
  const successColour=await toast.evaluate(el=>getComputedStyle(el).borderLeftColor);
  // The old progress timeout must not dismiss the success message.
  await page.clock.fastForward(4000);
  await expect(toast).toBeVisible();
  // Repeated identical results also get a fresh five seconds.
  socket.send(JSON.stringify({type:'result',message:'Started toast-task',error:false}));
  await page.waitForTimeout(100);
  await page.clock.fastForward(4000);
  await expect(toast).toBeVisible();
  await page.clock.fastForward(1000);
  await expect(toast).toBeHidden();
  await page.setViewportSize({width:390,height:844});
  socket.send(JSON.stringify({type:'result',message:'Unable to start task: repository is unavailable. Reopen the form to retry.',error:true}));
  await expect(toast).toHaveAttribute('data-kind','error');
  await expect(toast).toBeVisible();
  const style=await toast.evaluate(el=>{
   const css=getComputedStyle(el);
   return {border:css.borderTopWidth,bar:css.borderLeftWidth,colour:css.borderLeftColor};
  });
  expect(style.border).toBe('1px');expect(style.bar).toBe('5px');
  expect(new Set([progressColour,successColour,style.colour]).size).toBe(3);
  const box=await toast.boundingBox();
  expect(box.x).toBeGreaterThanOrEqual(0);expect(box.x+box.width).toBeLessThanOrEqual(390);
  await page.clock.fastForward(5000);
  await expect(toast).toBeHidden();
  await newTask.click();
  await expect(page.getByLabel('Task ID')).toHaveValue('toast-task');
  await expect(page.getByRole('combobox',{name:'Repository'})).toHaveValue('repo');
 }finally{await stop(server);cleanup(f);}
});

test('collapsed tasks show the summary, ID label and coloured status on one line',async({page})=>{
 const f=fixture();const server=await manager(f);
 try{
  const id='task-compact';
  f.run(binary,['task','start',id,'--workspace-only']);
  await page.goto(server.url);
  const row=taskRow(page,id),summary=row.locator('.task-summary'),badge=row.locator('.task-status');
  await expect(summary).toHaveText('Untitled task');
  await expect(row.locator('.task-id')).toHaveText(id);
  await expect(badge).toHaveText('Ready');
  await expect(row.locator('.details')).toHaveCount(0);
  const workspace=join(f.root,'repo-tasks',id);
  const text='Improve search navigation and preserve the selected result when returning from another task';
  writeFileSync(join(workspace,'.fixture-summary'),text);
  f.run(binary,['task','recover',id]);
  f.run(binary,['task','attach',id,'--new-window']);
  await expect(summary).toHaveText(text);
  await expect(summary).toHaveAttribute('title',text);
  const colours=new Set();
  for(const [status,label,value] of [
   ['working','Working',{type:'active',activeFlags:[]}],
   ['attention','Requires attention',{type:'active',activeFlags:['waitingOnApproval']}],
   ['waiting','Ready',{type:'idle'}],
  ]){
   writeFileSync(join(workspace,'.fixture-status'),JSON.stringify(value));
   await expect(badge).toHaveAttribute('data-status',status);
   await expect(badge).toHaveText(label);
   colours.add(await badge.evaluate(el=>getComputedStyle(el).backgroundColor));
   expect((await row.boundingBox()).height).toBeLessThanOrEqual(60);
  }
  expect(colours.size).toBe(3);
  await page.setViewportSize({width:390,height:844});
  expect((await row.boundingBox()).height).toBeLessThanOrEqual(60);
  expect(await summary.evaluate(el=>el.scrollWidth>el.clientWidth)).toBe(true);
  expect(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth)).toBe(true);
  const title=row.getByRole('button',{name:new RegExp(id)});
  await title.click();
  await expect(row.locator('.expanded-summary')).toHaveText(text);
  await expect(row.locator('.reason')).toHaveText('Conversation idle');
  await expect(row.locator('.repo')).toHaveText('repo');
  await title.press('Enter');await expect(row.locator('.details')).toHaveCount(0);
  await title.press('Space');
  page.once('dialog',d=>d.accept());await row.getByRole('button',{name:'Close task'}).click();
  await expect(row).toHaveCount(0);
  await page.getByLabel('Show completed').check();
  await expect(badge).toHaveText('Completed');
  await expect(badge).toHaveAttribute('data-status','completed');
  expect(colours.has(await badge.evaluate(el=>getComputedStyle(el).backgroundColor))).toBe(false);
  await expect(row.getByRole('button',{name:'Open terminal'})).toHaveCount(0);
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
  server=await manager(f);await page.goto(server.url);await expect(row.locator('.task-status')).toHaveAttribute('title',/Recovered conversation/,{timeout:25000});await expect(row).toContainText('Fixture conversation',{timeout:25000});
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

test('terminal windows reuse Alacritty, rediscover stale sockets, and fall back when IPC is unavailable',async({page})=>{
 const f=fixture(),log=join(f.dir,'alacritty.jsonl'),unavailable=join(f.dir,'alacritty-unavailable');
 f.env.AIDE_FIXTURE_ALACRITTY_LOG=log;
 f.env.AIDE_FIXTURE_ALACRITTY_UNAVAILABLE=unavailable;
 delete f.env.ALACRITTY_SOCKET;
 const server=await manager(f);
 const calls=()=>existsSync(log)?readFileSync(log,'utf8').trim().split('\n').filter(Boolean).map(line=>JSON.parse(line)):[];
 try{
  f.run(binary,['task','start','window-test','--workspace-only']);
  f.run(binary,['task','recover','window-test']);
  await page.goto(server.url);
  const row=taskRow(page,'window-test');await expect(row).toContainText('Fixture conversation');
  // The dashboard and CLI use the same launcher; successful IPC must not spawn.
  await row.getByRole('button',{name:'Open terminal'}).click();
  await expect(page.getByRole('status')).toContainText('Opened terminal');
  expect(calls()).toHaveLength(1);
  const session=runtimeTools(f,'window-test').session;
  const windowArgs=['--title','Aide · window-test','--working-directory',realpathSync(join(f.root,'repo-tasks','window-test')),'-e',realpathSync(execFileSync('which',['env'],{encoding:'utf8'}).trim()),'-u','TMUX','-u','TMUX_PANE',realpathSync(execFileSync('which',['tmux'],{encoding:'utf8'}).trim()),'-L',f.env.AIDE_TMUX_SOCKET,'attach-session','-t',session];
  expect(calls()[0].args).toEqual(['msg','create-window',...windowArgs]);

  f.env.TMUX='inherited-client';f.env.TMUX_PANE='%999';f.env.ALACRITTY_SOCKET='/fixture/stale.sock';
  f.run(binary,['task','attach','window-test','--new-window']);
  expect(calls()).toHaveLength(3);
  expect(calls()[1].socket).toBe('/fixture/stale.sock');
  expect(calls()[2].socket).toBeNull();
  expect(calls()[2].args).toEqual(['msg','create-window',...windowArgs]);

  delete f.env.ALACRITTY_SOCKET;
  writeFileSync(unavailable,'no running instance');
  f.run(binary,['task','attach','window-test','--new-window']);
  await expect.poll(()=>calls().length).toBe(5);
  expect(calls()[3].args).toEqual(['msg','create-window',...windowArgs]);
  expect(calls()[4].args).toEqual(windowArgs);
  for(const call of calls()){
   expect(call.tmux).toBeNull();expect(call.pane).toBeNull();
  }
  expect(f.run('tmux',['-L',f.env.AIDE_TMUX_SOCKET,'has-session','-t',session])).toBe('');
 }finally{await stop(server);cleanup(f);}
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

test('terminal activity aggregates detached panes and ignores backend and Codex process trees',async({page})=>{
 test.setTimeout(90000);
 const f=fixture();f.env.AIDE_FIXTURE_UI_CHILD='1';
 const server=await manager(f);
 try{
  f.run(binary,['task','start','activity','--workspace-only']);
  await page.goto(server.url);
  await expect(taskRow(page,'activity').locator('.terminal-activity')).toHaveText('—');
  f.run(binary,['task','recover','activity']);
  f.run(binary,['task','attach','activity','--new-window']);
  const r=runtimeTools(f,'activity'),work=r.pane('work'),backend=r.pane('backend');
  await expect.poll(()=>r.capture(work)).toContain('Fixture Codex conversation ready');
  await page.goto(server.url);
  const row=taskRow(page,'activity'),activity=row.locator('.terminal-activity');
  await expect(row).toContainText('Fixture conversation');
  await expect(activity).toHaveText('Idle');
  await expect(row.locator('.terminal-status')).toHaveAttribute('aria-label','Terminal activity: Idle');
  await expect(row.locator('.terminal-action .terminal-activity')).toHaveCount(0);
  await expect(row.getByRole('button',{name:'Open terminal',exact:true})).toHaveAttribute('title','Open terminal');
  const title=await page.title();
  expect(r.tmux('list-clients','-t',r.session)).toBe('');
  r.tmux('rename-window','-t',backend,'my renamed backend');
  r.tmux('swap-window','-s',backend,'-t',work);
  const tools=r.tmux('new-window','-d','-P','-F','#{pane_id}','-t',r.session,'-n','tools','fish');
  // Only the managed pane is ignored, not every pane sharing its window.
  const split=r.tmux('split-window','-d','-P','-F','#{pane_id}','-t',backend,'fish');
  await expect(activity).toHaveText('Idle');
  await r.send(tools,'sleep 90 &; set -g activity_job $last_pid');
  await expect(activity).toHaveText('Working');
  await expect(row.locator('.terminal-status')).toHaveAttribute('aria-label','Terminal activity: Working');
  await expect(row.locator('.task-status')).toHaveText('Ready');
  await expect(page).toHaveTitle(title);
  await r.send(split,'sleep 90');
  await r.send(tools,'kill $activity_job');
  await expect(activity).toHaveText('Working');
  r.tmux('send-keys','-t',split,'C-c');
  await expect(activity).toHaveText('Idle');
  // Codex resumed in a user-added pane is also excluded, including its child.
  await r.send(tools,`'${binary}' codex resume`);
  await expect.poll(()=>r.capture(tools)).toContain('Fixture Codex conversation ready');
  await expect(activity).toHaveText('Idle');
  // Exiting the managed UI must not hide later work in that same pane.
  r.tmux('send-keys','-t',work,'C-d');
  await expect.poll(()=>r.tmux('display-message','-p','-t',work,'#{pane_current_command}')).toBe('fish');
  await r.send(work,'sleep 90');
  await expect(activity).toHaveText('Working');
  await page.reload();await expect(activity).toHaveText('Working');
  r.tmux('send-keys','-t',work,'C-c');
  await expect(activity).toHaveText('Idle');
  // Neither a retained dead pane nor a removed pane may keep work active.
  r.tmux('set-option','-p','-t',split,'remain-on-exit','on');
  await r.send(split,'exit');
  r.tmux('kill-pane','-t',tools);
  await expect(activity).toHaveText('Idle');
  expect(f.run(binary,['task','list'])).toContain('terminal=Idle');
  await page.setViewportSize({width:390,height:844});
  expect(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth)).toBe(true);
  expect((await row.boundingBox()).height).toBeLessThanOrEqual(60);
 }finally{await stop(server);cleanup(f);}
});

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
  // The newly restored UI can still briefly hold the task lock while resuming.
  await expect.poll(()=>{
   try{f.run(binary,['task','recover','runtime']);return 'unexpected recovery';}
   catch(error){return error.message;}
  }).toMatch(/unresponsive/);
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
  await expect(row.locator('.task-status')).toHaveAttribute('title',/Multiple conversations found/,{timeout:15000});
  await row.getByRole('button',{name:/ambiguous/}).click();
  await expect(row.locator('dd').first()).toHaveText('Not started');
 }finally{await stop(server);cleanup(f);}
});

test('title helper threads do not block binding and resolved monitoring errors clear',async({page})=>{
 const f=fixture();const server=await manager(f);
 try{
  f.run(binary,['task','start','helper','--workspace-only']);
  const workspace=join(f.root,'repo-tasks','helper');
  // Begin genuinely ambiguous, then classify the extra thread as the native
  // title helper. This also exercises recovery from an existing monitor error.
  writeFileSync(join(workspace,'.fixture-other-thread'),'yes');
  f.run(binary,['task','recover','helper']);
  f.run(binary,['task','attach','helper','--new-window']);
  await page.goto(server.url);const row=taskRow(page,'helper');
  await expect(row.locator('.task-status')).toHaveAttribute('title',/Multiple conversations found/);
  writeFileSync(join(workspace,'.fixture-other-thread'),'thread_title');
  await expect(row.locator('.task-summary')).toHaveText('Fixture conversation');
  await expect(row.locator('.task-status')).toHaveText('Ready');
  await row.getByRole('button',{name:/helper/}).click();
  await expect(row.locator('dd').first()).toHaveText(/^fixture-/);
  writeFileSync(join(workspace,'.fixture-status'),JSON.stringify({type:'active',activeFlags:[]}));
  await expect(row.locator('.task-status')).toHaveText('Working');
  await page.reload();await expect(row.locator('.task-summary')).toHaveText('Fixture conversation');
  await expect(row.locator('.task-status')).toHaveText('Working');
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
  const start=async(id,repo)=>{
   if(!await page.getByLabel('Task ID').isVisible())await page.getByRole('button',{name:'New task'}).click();
   await page.getByLabel('Task ID').fill(id);await page.getByRole('combobox').selectOption(repo);await page.getByRole('button',{name:'Start and open Codex'}).click();
  };
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
  await expect(healthy.locator('.task-status')).toHaveAttribute('title',/Closing/);
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
