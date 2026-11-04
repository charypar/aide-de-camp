import {test,expect} from '@playwright/test';
import {writeFileSync,readFileSync,existsSync,unlinkSync} from 'node:fs';
import {join,resolve} from 'node:path';
import {fixture,manager,stop,taskRow,cleanup,binary} from './aide-fixture.js';

function githubFixture(state={}) {
 const f=fixture();f.env.AIDE_GH=resolve('tests/aide-fixtures/gh');f.env.AIDE_GITHUB_FIXTURE=join(f.dir,'github.json');
 f.state=value=>writeFileSync(f.env.AIDE_GITHUB_FIXTURE,JSON.stringify(value));f.state(state);
 f.queries=()=>existsSync(f.env.AIDE_GITHUB_FIXTURE+'.log')?readFileSync(f.env.AIDE_GITHUB_FIXTURE+'.log','utf8').trim().split('\n').map(JSON.parse):[];
 f.run(binary,['task','start','one','--workspace-only']);
 return f;
}
const url=n=>`https://github.com/example/repo/pull/${n}`;
async function add(row,n){await row.getByRole('button',{name:'Link PR',exact:true}).click();await row.getByLabel('PR URL').fill(url(n));await row.getByRole('button',{name:'Add',exact:true}).click();await expect(row.locator('.pr-message')).toHaveText(/PR linked|PR already linked/);}
async function refresh(row){await row.getByRole('button',{name:'Refresh PRs'}).click();await expect(row.locator('.pr-message')).toHaveText('PR statuses refreshed');}

test('startup polls without a browser; an in-flight PR refresh cannot revive a closed task',async({page})=>{
 const f=githubFixture();let server=await manager(f);const gate=join(f.dir,'github.hold');
 try{
  f.run(binary,['task','recover','one']);f.run(binary,['task','attach','one','--new-window']);
  f.run(binary,['task','start','two','--workspace-only']);
  f.run(binary,['task','pr','one','link',url(1)]);f.run(binary,['task','pr','two','link',url(2)]);
  await stop(server);
  const before=f.queries().length;
  writeFileSync(gate,'hold');f.state({gate,prs:{1:{status:'merged'},2:{status:'merged'}}});
  server=await manager(f);
  // Fresh cached links must still be polled on restart, before any browser connects.
  await expect.poll(()=>f.queries().length).toBeGreaterThan(before);
  await page.goto(server.url);const one=taskRow(page,'one'),two=taskRow(page,'two');
  await expect(one.locator('.task-heading .pr-tag')).toHaveAttribute('title',/Stale/);
  await one.locator('.task-title').click();
  page.once('dialog',d=>d.accept());await one.getByRole('button',{name:'Close task',exact:true}).click();
  await expect(one).toHaveCount(0);
  expect(existsSync(join(f.root,'repo-tasks','one'))).toBe(false);
  unlinkSync(gate);
  await expect(two.locator('.task-heading .pr-tag')).toHaveAttribute('title',/Merged/);
  await expect(page).toHaveTitle('(0·1) Aide de camp');
  await page.reload();await expect(one).toHaveCount(0);
  await page.getByLabel('Show completed').check();
  await expect(one.locator('.task-status')).toHaveText('Completed');
  await expect(one.locator('.task-heading .pr-tag')).toHaveAttribute('title',/Ready to merge.*Historical/);
  await expect(one.getByRole('button',{name:'Open terminal',exact:true})).toHaveCount(0);
 }finally{if(existsSync(gate))unlinkSync(gate);await stop(server);cleanup(f);}
});

test('compact PR tags align with task labels and stay clear of controls on phones',async({page})=>{
 const f=githubFixture();const server=await manager(f);
 try{
  await page.goto(server.url);const row=taskRow(page,'one');await row.locator('.task-title').click();
  for(const number of [12345,23456,34567])await add(row,number);
  await row.locator('.task-title').click();
  const tags=row.locator('.task-heading .pr-tag');await expect(tags).toHaveText(['#12345','#23456','#34567']);
  const centers=await row.locator('.task-id, .task-status, .terminal-status, .task-heading .pr-tag').evaluateAll(elements=>elements.map(el=>{const r=el.getBoundingClientRect();return r.y+r.height/2;}));
  expect(Math.max(...centers)-Math.min(...centers)).toBeLessThan(2);
  await page.setViewportSize({width:390,height:844});
  const title=await row.locator('.task-title').boundingBox();const summary=await row.locator('.task-summary').boundingBox();
  expect(summary.width).toBeGreaterThan(30);
  for(const tag of await tags.all()){
   const box=await tag.boundingBox();expect(box.y).toBeGreaterThanOrEqual(title.y+title.height);expect(box.x+box.width).toBeLessThan(390);
  }
  await expect(row.getByRole('button',{name:'Open terminal',exact:true})).toBeVisible();
  expect(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth)).toBe(true);
 }finally{await stop(server);cleanup(f);}
});

test('N PR links, independent roles, attention, unlink, history and restart',async({page,context})=>{
 const f=githubFixture({prs:{1:{status:'waiting'},100:{status:'rerequest'}}});let server=await manager(f);
 const errors=[];page.on('pageerror',e=>errors.push(e.message));
 try{
  await page.goto(server.url);const row=taskRow(page,'one');await row.locator('.task-title').click();
  await expect(row.getByText('No linked PRs')).toBeVisible();await add(row,1);await add(row,100);
  await expect(row.locator('.task-heading .pr-tag')).toHaveCount(2);await expect(page).toHaveTitle('(1·0) Aide de camp');
  await expect(row.locator('.task-status')).toHaveText('Ready');
  await expect(row.locator('.task-heading .pr-tag')).toHaveText(['#1','#100']);
  await expect(row.locator('.task-heading .pr-tag').first()).toHaveAttribute('title',/example\/repo #1.*Waiting for review/);
  await expect(row.locator('.task-heading .pr-tag').last()).toHaveAccessibleName(/example\/repo #100.*Review requested again/);
  const centers=await row.locator('.task-id, .task-status, .terminal-status, .task-heading .pr-tag').evaluateAll(elements=>elements.map(el=>{const r=el.getBoundingClientRect();return r.y+r.height/2;}));
  expect(Math.max(...centers)-Math.min(...centers)).toBeLessThan(2);

  await expect(row.getByText(/Reviewing · @colleague/)).toBeVisible();
  const review=row.locator('.pr-entry').filter({hasText:'Example PR 100'});await expect(review.locator('.pr-checks')).toHaveCount(0);
  await add(row,1);await expect(row.locator('.task-heading .pr-tag')).toHaveCount(2);
  // Input and expansion survive snapshots and regrouping.
  await row.getByRole('button',{name:'Link PR',exact:true}).click();await row.getByLabel('PR URL').fill(url(2));
  f.state({prs:{1:{status:'ready'},100:{status:'waiting'}}});await refresh(row);
  await expect(row.getByLabel('PR URL')).toHaveValue(url(2));await expect(row.locator('.task-heading .pr-tag').first()).toHaveAttribute('data-color','green');
  await page.waitForTimeout(2200);await expect(row.getByLabel('PR URL')).toHaveValue(url(2));
  await row.getByRole('button',{name:'Cancel',exact:true}).click();
  // No nested interactive control; keyboard navigation opens the PR independently.
  const badge=row.locator('.task-heading .pr-tag').first();await expect(badge).toHaveAttribute('href',url(1));
  await expect(badge).toHaveAttribute('target','_blank');expect(await badge.evaluate(el=>el.closest('button'))).toBeNull();
  await context.route('https://github.com/**',route=>route.fulfill({body:'Fixture PR'}));
  const popupPromise=page.waitForEvent('popup');await badge.focus();await page.keyboard.press('Enter');const popup=await popupPromise;await popup.close();await expect(row.locator('.task-title')).toHaveAttribute('aria-expanded','true');
  // A second browser does not start another observer.
  const count=f.queries().length;const second=await context.newPage();await second.goto(server.url);await expect(second.locator('.pr-tag').first()).toBeVisible();await second.close();expect(f.queries()).toHaveLength(count);
  await page.setViewportSize({width:390,height:844});expect(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth)).toBe(true);await expect(row.getByRole('button',{name:'Open terminal',exact:true})).toBeVisible();
  // Unlink the sole actionable PR; native state and the other association remain.
  await row.getByRole('button',{name:'Unlink example/repo #1',exact:true}).click();await expect(row.locator('.task-heading .pr-tag')).toHaveCount(1);await expect(page).toHaveTitle('(0·1) Aide de camp');
  await expect(row.locator('.task-title')).toHaveAttribute('aria-expanded','true');
  // CLI uses the same durable link table and protocol.
  f.run(binary,['task','pr','one','link',url(1)]);expect(f.run(binary,['task','pr','one','list'])).toContain('Ready to merge');
  f.run(binary,['task','pr','one','unlink',url(1)]);await expect(row.locator('.task-heading .pr-tag')).toHaveCount(1);
  f.state({prs:{100:{status:'merged'}}});const before=f.queries().length;await stop(server);server=await manager(f);await page.goto(server.url);
  await expect(row.locator('.task-heading .pr-tag')).toHaveAttribute('title',/Merged/);expect(f.queries().length).toBeGreaterThan(before);await expect(page).toHaveTitle('(0·1) Aide de camp');
  await row.locator('.task-title').click();page.once('dialog',d=>d.accept());await row.getByRole('button',{name:'Close task',exact:true}).click();await expect(row).toHaveCount(0);
  await page.getByLabel('Show completed').check();await expect(row).toBeVisible();await expect(row.locator('.task-heading .pr-tag')).toHaveAttribute('title',/Historical/);
  if(await row.locator('.task-title').getAttribute('aria-expanded')==='false')await row.locator('.task-title').click();
  await expect(row.getByRole('button',{name:'Link PR',exact:true})).toHaveCount(0);await expect(row.getByRole('button',{name:/Unlink/})).toHaveCount(0);
  expect(errors).toEqual([]);
 }finally{await stop(server);cleanup(f);}
});

test('inline errors retain input; failed refresh is stale and restart recovers',async({page})=>{
 const f=githubFixture();let server=await manager(f);
 try{
  await page.goto(server.url);const row=taskRow(page,'one');await row.locator('.task-title').click();
  await row.getByRole('button',{name:'Link PR',exact:true}).click();await row.getByLabel('PR URL').fill('https://github.com/example/repo/issues/1');await row.getByRole('button',{name:'Add',exact:true}).click();
  await expect(row.locator('.pr-message')).toContainText('pull request URL');await expect(row.getByLabel('PR URL')).toHaveValue(/issues/);
  expect(f.queries()).toHaveLength(0);
  await row.getByLabel('PR URL').fill(url(1));await row.getByRole('button',{name:'Add',exact:true}).click();await expect(row.locator('.pr-message')).toHaveText('PR linked');
  await expect(row.locator('.task-heading .pr-tag')).toHaveAttribute('data-color','green');
  f.state({error:401});await row.getByRole('button',{name:'Refresh PRs'}).click();await expect(row.locator('.pr-message')).toContainText('authentication unavailable');
  await expect(row.locator('.task-heading .pr-tag')).toHaveAttribute('title',/Stale/);await expect(row.locator('.task-heading .pr-tag')).toHaveAttribute('data-color','grey');await expect(page).toHaveTitle('(1·0) Aide de camp');
  await row.getByRole('button',{name:'Link PR',exact:true}).click();await row.getByLabel('PR URL').fill(url(2));await row.getByRole('button',{name:'Add',exact:true}).click();await expect(row.locator('.pr-message')).toContainText('paused');await expect(row.getByLabel('PR URL')).toHaveValue(url(2));
  // Cleanup and unlink do not depend on remote access.
  await row.getByRole('button',{name:'Cancel',exact:true}).click();await row.getByRole('button',{name:'Unlink example/repo #1',exact:true}).click();await expect(row.locator('.task-heading .pr-tag')).toHaveCount(0);await expect(page).toHaveTitle('(0·1) Aide de camp');
  f.state({prs:{1:{status:'failed'}}});await stop(server);server=await manager(f);await page.goto(server.url);await row.locator('.task-title').click();await add(row,1);
  await expect(row.locator('.task-heading .pr-tag')).toHaveAttribute('data-color','yellow');await expect(row.locator('.pr-reasons')).toContainText('CI failed');
 }finally{await stop(server);cleanup(f);}
});

test('test-merge checks control readiness and head checks are the fallback',async({page})=>{
 const f=githubFixture({prs:{1:{status:'failed',merge_check:'SUCCESS'}}});const server=await manager(f);
 try{
  await page.goto(server.url);const row=taskRow(page,'one');await row.locator('.task-title').click();await add(row,1);
  const badge=row.locator('.task-heading .pr-tag');await expect(badge).toHaveAttribute('title',/Ready to merge/);await expect(badge).toHaveAttribute('data-color','green');
  await row.locator('.pr-checks summary').click();await expect(row.locator('.pr-checks')).toContainText('merge-build: SUCCESS');await expect(row.locator('.pr-checks')).not.toContainText('FAILURE');
  f.state({prs:{1:{status:'ready',merge_check:'FAILURE'}}});await refresh(row);await expect(badge).toHaveAttribute('title',/Needs attention/);await expect(badge).toHaveAttribute('data-color','yellow');await expect(row.locator('.pr-reasons')).toContainText('CI failed');
  f.state({prs:{1:{status:'running',merge_check:'NONE',merge_state:'BLOCKED'}}});await refresh(row);await expect(badge).toHaveAttribute('title',/CI running/);await expect(page).toHaveTitle('(0·1) Aide de camp');
  await expect(badge).toHaveAttribute('data-color','grey');
  await expect(row.locator('.pr-entry .pr-tag')).toHaveAttribute('data-color','grey');
 }finally{await stop(server);cleanup(f);}
});

test('damaged records and failed saves leave other PRs fresh and report recovery',async({page})=>{
 const f=githubFixture({prs:{1:{status:'waiting'},2:{status:'waiting'}}});f.run(binary,['task','start','two','--workspace-only']);let server=await manager(f);
 const sql=query=>f.run('python3',['-c','import sqlite3,sys; db=sqlite3.connect(sys.argv[1]); db.executescript(sys.argv[2]); db.close()',join(f.root,'.aide','state.sqlite'),query]);
 try{
  f.run(binary,['task','pr','one','link',url(1)]);f.run(binary,['task','pr','two','link',url(2)]);await stop(server);
  sql("UPDATE github_links SET body='broken' WHERE task_id='two'; INSERT INTO github_links VALUES('missing','invalid-url','{}'); CREATE TRIGGER fail_status BEFORE UPDATE ON github_links WHEN OLD.task_id='one' BEGIN SELECT RAISE(FAIL,'fixture write failure'); END;");
  f.state({});server=await manager(f);await page.goto(server.url);
  const one=taskRow(page,'one'),two=taskRow(page,'two'),errors=page.locator('.project > p.error');
  await expect(two.locator('.task-heading .pr-tag')).toHaveAttribute('title',/Ready to merge/);await expect(two.locator('.task-heading .pr-tag')).toHaveAttribute('data-color','green');
  await expect(one.locator('.task-heading .pr-tag')).toHaveAttribute('title',/Stale/);await expect(one.locator('.task-heading .pr-tag')).toHaveAttribute('data-color','grey');
  await expect(errors).toContainText('Invalid stored PR URL');await expect(errors).toContainText('Could not save PR status');
  await one.locator('.task-title').click();await expect(one.locator('.pr-entry .error')).toContainText('fixture write failure');
  sql("DROP TRIGGER fail_status; DELETE FROM github_links WHERE url='invalid-url';");await refresh(one);
  await expect(one.locator('.task-heading .pr-tag')).toHaveAttribute('data-color','green');await expect(errors).toBeEmpty();await expect(one.locator('.pr-entry .error')).toBeEmpty();
 }finally{await stop(server);cleanup(f);}
});

test('parking overrides existing PR attention until a new attention transition',async({page})=>{
 const f=githubFixture({prs:{1:{status:'ready'}}});let server=await manager(f);
 try{
  await page.goto(server.url);let row=taskRow(page,'one');await row.locator('.task-title').click();await add(row,1);
  await expect(page).toHaveTitle('(1·0) Aide de camp');
  await row.getByRole('button',{name:'Park task',exact:true}).click();
  await expect(row.locator('.task-status')).toHaveText('Parked');await expect(page).toHaveTitle('Aide de camp');
  await stop(server);server=await manager(f);await page.goto(server.url);
  await expect(row.locator('.task-status')).toHaveText('Parked');
  expect(f.run(binary,['task','list'])).toContain('Parked');
  await row.locator('.task-title').click();await refresh(row);
  await expect(row.locator('.task-status')).toHaveText('Parked');
  f.state({prs:{1:{status:'waiting'}}});await refresh(row);
  await expect(row.locator('.task-heading .pr-tag')).toHaveAttribute('title',/Waiting for review/);
  await expect(row.locator('.task-status')).toHaveText('Parked');
  f.state({prs:{1:{status:'ready'}}});await refresh(row);
  await expect(page).toHaveTitle('(1·0) Aide de camp');
  await expect(row.locator('.task-status')).not.toHaveText('Parked');
  f.run(binary,['task','park','one']);await expect(row.locator('.task-status')).toHaveText('Parked');
  f.run(binary,['task','unpark','one']);await expect(page).toHaveTitle('(1·0) Aide de camp');
 }finally{await stop(server);cleanup(f);}
});
