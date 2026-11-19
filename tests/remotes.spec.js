import {realpathSync} from 'node:fs';
import {test, expect} from '@playwright/test';
import {binary, fixture, manager, stop, cleanup, taskRow} from './aide-fixture.js';

test('prefixed dashboard supports actions and removes stale data on disconnect', async ({page}) => {
 const f=fixture(); let server;
 try {
  f.run(binary,['task','start','prefix-task','--workspace-only']);
  server=await manager(f);
  await page.goto(`${server.url}/aide/`);
  await expect(taskRow(page,'prefix-task')).toBeVisible();
  await page.locator('[data-machine=local]').click();
  await taskRow(page,'prefix-task').getByRole('button').first().click();
  await page.getByRole('button',{name:'Park task',exact:true}).click();
  await expect(taskRow(page,'prefix-task').getByRole('button').first()).toHaveAttribute('aria-expanded','false');
  await taskRow(page,'prefix-task').getByRole('button').first().click();
  await expect(page.getByRole('button',{name:'Unpark task',exact:true})).toBeVisible();
  await page.reload();
  await expect(taskRow(page,'prefix-task')).toBeVisible();
  await stop(server);
  await expect(page.locator('[data-machine=local]')).toHaveAttribute('title', /Disconnected/);
  await expect(page.locator('[data-machine=local]')).not.toContainText('(0.');
  await expect(page.locator('.task')).toHaveCount(0);
  await expect(page.getByText('Make room for your next task')).toHaveCount(0);
 } finally {if(server) await stop(server);cleanup(f);}
});

test('headless dashboard creates tasks without launching desktop applications', async ({page}) => {
 const f=fixture(); f.env.AIDE_DESKTOP='0'; let server;
 try {
  server=await manager(f);
  await page.goto(server.url);
  await page.getByRole('button',{name:'New task',exact:true}).click();
  await page.getByLabel('Task ID',{exact:true}).fill('headless-task');
  await page.getByRole('button',{name:'Start task',exact:true}).click();
  await expect(taskRow(page,'headless-task')).toBeVisible();
  await expect(page.getByRole('button',{name:'Terminal',exact:true})).toHaveCount(0);
  await taskRow(page,'headless-task').getByRole('button').first().click();
  await expect(page.getByRole('button',{name:'Zed',exact:true})).toHaveCount(0);
 } finally {if(server) await stop(server);cleanup(f);}
});

test('remote machine selection survives snapshots and disconnected tasks disappear', async ({page}) => {
 const f=fixture();let server;let disconnected=false;let snapshots=0;
 try {
  f.run(binary,['task','start','same-name','--workspace-only']);
  server=await manager(f);
  await page.routeWebSocket('**/ws',socket=>{
   const backend=socket.connectToServer();
   backend.onMessage(data=>{
    const event=JSON.parse(data.toString());
    if(event.type==='hosts') event.hosts=[{identity:{...event.local,instance:'fixture-remote',name:'Other host'},error:disconnected?'Disconnected':null}];
    if(event.type==='snapshot') snapshots++;
    if(event.type==='snapshot'&&!disconnected) event.projects.push(...event.projects.map(project=>({...project,name:`Other host · ${project.name}`,root:`peer:fixture-remote:${project.root}`,source:{instance:'fixture-remote',name:'Other host',root:project.root}})));
    socket.send(JSON.stringify(event));
   });
  });
  await page.goto(server.url);
  await expect(page.locator('.task')).toHaveCount(2);
  await page.locator('[data-machine=fixture-remote]').click();
  await expect(page.locator('.task')).toHaveCount(1);
  await expect(page.locator('.project-name')).toHaveText('Other host · Cat Herder');
  const previous=snapshots;
  await expect.poll(()=>snapshots).toBeGreaterThan(previous);
  await expect(page.locator('[data-machine=fixture-remote]')).toHaveAttribute('aria-pressed','true');
  await expect(page.locator('[data-machine=fixture-remote]')).toContainText('(0.1)');
  await page.locator('[data-machine=fixture-remote]').click();
  await expect(page.locator('.task')).toHaveCount(2);
  await page.locator('[data-machine=fixture-remote]').click();
  disconnected=true;
  await expect(page.locator('[data-machine=fixture-remote]')).toHaveAttribute('title', /Disconnected/);
  await expect(page.locator('[data-machine=fixture-remote]')).not.toContainText('(0.1)');
  await expect(page.locator('.task')).toHaveCount(0);
  await expect(page.getByText('Other host: Disconnected — overview incomplete')).toBeVisible();
  disconnected=false;
  await expect(page.locator('.task')).toHaveCount(1);
  await expect(page.locator('[data-machine=fixture-remote]')).toContainText('(0.1)');
  await stop(server);
  await expect(page.locator('.machine.offline')).toHaveCount(2);
  await expect(page.locator('.machine-counts')).toHaveText(['—','—']);
  await expect(page.locator('.task')).toHaveCount(0);
 } finally {if(server) await stop(server);cleanup(f);}
});

 test('header retains Tasks and configuration with five compact machines', async ({page}) => {
 const f=fixture(); let server;
 try {
  server=await manager(f);
  await page.routeWebSocket('**/ws', socket=>{
   const backend=socket.connectToServer();
   backend.onMessage(data=>{
    const event=JSON.parse(data.toString());
    if(event.type==='hosts') {
     event.local.name='macbook-air';
     event.hosts=Array.from({length:4},(_,i)=>({identity:{...event.local,instance:`vm-${i}`,name:`linux-vm-${i}`},error:null}));
    }
    socket.send(JSON.stringify(event));
   });
  });
  await page.goto(`${server.url}/aide/`);
  await expect(page.locator('.machine')).toHaveCount(5);
  await expect(page.getByRole('link',{name:'Tasks',exact:true})).toBeVisible();
  const settings=page.getByRole('link',{name:'AI configuration',exact:true});
  const last=await page.locator('.machine').last().boundingBox();
  const cog=await settings.boundingBox();
  expect(cog.x).toBeGreaterThan(last.x+last.width);
  await expect(page.locator('.machine').first()).toContainText('(0.0)');
  await settings.click();
  await expect(page.getByRole('heading',{name:'AI configuration',exact:true})).toBeVisible();
  await page.getByRole('link',{name:'Tasks',exact:true}).click();
  await expect(page.getByRole('button',{name:'New task',exact:true})).toBeVisible();
 } finally {if(server) await stop(server);cleanup(f);}
});


test('remote Zed shortcut targets the owner and reports launch errors without expanding', async ({page}) => {
 const f=fixture();let server;const commands=[];let fail=true;
 try {
  f.run(binary,['task','start','remote-editor','--workspace-only']);
  server=await manager(f);
  await page.routeWebSocket('**/ws',socket=>{
   const backend=socket.connectToServer();
   backend.onMessage(data=>{
    const event=JSON.parse(data.toString());
    if(event.type==='hosts') event.hosts=[{identity:{...event.local,instance:'fixture-remote',name:'Linux VM',desktop:false},error:null}];
    if(event.type==='snapshot') event.projects=event.projects.map(project=>({...project,root:`peer:fixture-remote:${project.root}`,source:{instance:'fixture-remote',name:'Linux VM',root:project.root}}));
    socket.send(JSON.stringify(event));
   });
   socket.onMessage(data=>{
    const request=JSON.parse(data.toString());commands.push(request.command);
    socket.send(JSON.stringify({type:'action_result',request_id:request.request_id,error:fail,message:fail?'Zed launcher failed':'Requested Zed on Mac'}));
   });
  });
  await page.goto(server.url);
  const row=taskRow(page,'remote-editor');
  for(const width of [1440,390]){
   await page.setViewportSize({width,height:900});
   await expect(row.getByRole('button',{name:'Zed',exact:true})).toBeVisible();
   await expect(row.getByRole('button',{name:'Terminal',exact:true})).toBeVisible();
  }
  await row.getByRole('button',{name:'Zed',exact:true}).click();
  await expect(page.getByRole('status')).toContainText('Zed launcher failed');
  expect(commands[0]).toMatchObject({action:'editor',id:'remote-editor',project:`peer:fixture-remote:${realpathSync(f.root)}`});
  await expect(row.locator('.task-title')).toHaveAttribute('aria-expanded','false');
  fail=false;
  await row.getByRole('button',{name:'Zed',exact:true}).click();
  await expect(page.getByRole('status')).toContainText('Requested Zed on Mac');
  await page.reload();
  await expect(row.getByRole('button',{name:'Zed',exact:true})).toBeVisible();
  await row.locator('.task-title').click();
  await expect(row.getByRole('button',{name:'Review',exact:true})).toHaveCount(0);
 } finally {if(server)await stop(server);cleanup(f);}
});
