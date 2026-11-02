import {test,expect} from '@playwright/test';
import {writeFileSync,readFileSync,existsSync} from 'node:fs';
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

test('N PR links, independent roles, attention, unlink, history and restart',async({page,context})=>{
 const f=githubFixture({prs:{1:{status:'waiting'},100:{status:'rerequest'}}});let server=await manager(f);
 const errors=[];page.on('pageerror',e=>errors.push(e.message));
 try{
  await page.goto(server.url);const row=taskRow(page,'one');await row.locator('.task-title').click();
  await expect(row.getByText('No linked PRs')).toBeVisible();await add(row,1);await add(row,100);
  await expect(row.locator('.task-heading .pr-tag')).toHaveCount(2);await expect(page).toHaveTitle('(1·0) Aide de camp');
  await expect(row.locator('.task-status')).toHaveText('Ready');
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
  await expect(row.locator('.task-heading .pr-tag')).toContainText('Merged');expect(f.queries().length).toBeGreaterThan(before);await expect(page).toHaveTitle('(0·1) Aide de camp');
  await row.locator('.task-title').click();page.once('dialog',d=>d.accept());await row.getByRole('button',{name:'Close task',exact:true}).click();await expect(row).toHaveCount(0);
  await page.getByLabel('Show completed').check();await expect(row).toBeVisible();await expect(row.locator('.task-heading .pr-tag')).toContainText('Historical');
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
  await expect(row.locator('.task-heading .pr-tag')).toContainText('Stale');await expect(row.locator('.task-heading .pr-tag')).toHaveAttribute('data-color','grey');await expect(page).toHaveTitle('(1·0) Aide de camp');
  await row.getByRole('button',{name:'Link PR',exact:true}).click();await row.getByLabel('PR URL').fill(url(2));await row.getByRole('button',{name:'Add',exact:true}).click();await expect(row.locator('.pr-message')).toContainText('paused');await expect(row.getByLabel('PR URL')).toHaveValue(url(2));
  // Cleanup and unlink do not depend on remote access.
  await row.getByRole('button',{name:'Cancel',exact:true}).click();await row.getByRole('button',{name:'Unlink example/repo #1',exact:true}).click();await expect(row.locator('.task-heading .pr-tag')).toHaveCount(0);await expect(page).toHaveTitle('(0·1) Aide de camp');
  f.state({prs:{1:{status:'failed'}}});await stop(server);server=await manager(f);await page.goto(server.url);await row.locator('.task-title').click();await add(row,1);
  await expect(row.locator('.task-heading .pr-tag')).toHaveAttribute('data-color','yellow');await expect(row.locator('.pr-reasons')).toContainText('CI failed');
 }finally{await stop(server);cleanup(f);}
});
