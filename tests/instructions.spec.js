import {test, expect} from '@playwright/test';
import {mkdirSync, writeFileSync, readFileSync, unlinkSync, realpathSync, existsSync} from 'node:fs';
import {join} from 'node:path';
import {fixture, manager, stop, cleanup, binary, taskRow} from './aide-fixture.js';

function configure(f) {
 const config=join(f.home,'agent-config');
 mkdirSync(join(config,'projects','personal'),{recursive:true});
 writeFileSync(join(config,'methodology.md'),'Global test methodology');
 writeFileSync(join(config,'projects.json'),JSON.stringify({[realpathSync(f.root)]:'personal'}));
 writeFileSync(join(config,'projects/personal/instructions.md'),'Private project conventions');
 return config;
}
function instructions(f,id) {
 const path=join(f.root,'repo-tasks',id,'.fixture-instructions');
 return existsSync(path)?readFileSync(path,'utf8'):'';
}

function task(f,id) {
 return JSON.parse(f.run('python3',['-c',
  'import sqlite3,sys; db=sqlite3.connect(".aide/state.sqlite"); print(db.execute("SELECT body FROM tasks WHERE id=?",(sys.argv[1],)).fetchone()[0])',id]));
}

test('private instructions reach task conversations and survive source changes and recovery',async({page})=>{
 const f=fixture(),config=configure(f);let server=await manager(f);
 try {
  f.run(binary,['task','start','configured','--new-window']);
  await expect.poll(()=>instructions(f,'configured')).toContain('Private project conventions');
  const original=instructions(f,'configured');
  expect(original).toContain('Existing native preferences');
  expect(original).toContain('Global test methodology');
  await page.goto(server.url);
  const row=taskRow(page,'configured');
  await expect(row.locator('.task-summary')).toHaveText('Fixture conversation');
  await expect(row.locator('.task-status')).toHaveText('Ready');

  await expect.poll(()=>task(f,'configured').thread_id).toBeTruthy();
  const identity=task(f,'configured').thread_id;
  writeFileSync(join(config,'methodology.md'),'Revised global methodology');
  unlinkSync(join(config,'projects/personal/instructions.md'));
  await stop(server);
  cleanup(f);
  unlinkSync(join(f.root,'repo-tasks/configured/.fixture-instructions'));
  server=await manager(f);
  await expect.poll(()=>instructions(f,'configured'),{timeout:15000}).toBe(original);
  expect(task(f,'configured').thread_id).toBe(identity);
  await page.goto(server.url);
  await expect(row.locator('.task-summary')).toHaveText('Fixture conversation');
  await page.reload();
  await expect(row.locator('.task-summary')).toHaveText('Fixture conversation');

  writeFileSync(join(config,'projects/personal/instructions.md'),'Revised private project');
  f.run(binary,['task','start','revised','--new-window']);
  await expect.poll(()=>instructions(f,'revised')).toContain('Revised private project');
  expect(instructions(f,'revised')).toContain('Revised global methodology');
  expect(instructions(f,'configured')).toBe(original);
 } finally {await stop(server);cleanup(f);}
});

test('invalid private project mapping fails visibly and a corrected configuration recovers',async({page})=>{
 const f=fixture(),config=configure(f),server=await manager(f);
 try {
  writeFileSync(join(config,'projects.json'),'not JSON');
  f.run(binary,['task','start','broken','--workspace-only']);
  expect(()=>f.run(binary,['task','recover','broken'])).toThrow();
  await page.goto(server.url);
  writeFileSync(join(config,'projects.json'),JSON.stringify({[realpathSync(f.root)]:'personal'}));
  const failure=join(f.root,'repo-tasks/broken/.fixture-config-error');
  writeFileSync(failure,'fail configuration probe');
  expect(()=>f.run(binary,['task','recover','broken'])).toThrow();
  expect(instructions(f,'broken')).toBe('');
  unlinkSync(failure);
  f.run(binary,['task','recover','broken']);
  await expect.poll(()=>instructions(f,'broken')).toContain('Private project conventions');
  await expect(taskRow(page,'broken').locator('.task-summary')).toHaveText('Fixture conversation');
 } finally {await stop(server);cleanup(f);}
});
