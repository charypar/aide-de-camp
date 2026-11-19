import {test, expect} from '@playwright/test';
import {mkdirSync, writeFileSync, readFileSync, existsSync, unlinkSync, chmodSync, realpathSync} from 'node:fs';
import {join} from 'node:path';
import {fixture, manager, stop, cleanup} from './aide-fixture.js';

let f, server, root, launchLog;
test.beforeEach(async () => {
  f = fixture();
  root = join(realpathSync(f.home), 'agent-config');
  mkdirSync(join(root, 'skills', 'example'), {recursive:true});
  writeFileSync(join(root, 'methodology.md'), '# Working rules\nUse meaningful tests.\n');
  writeFileSync(join(root, 'projects.json'), 'malformed mappings remain readable');
  writeFileSync(join(root, 'skills', 'example', 'SKILL.md'), '# Example skill\nSupporting workflow');
  const bin = join(f.dir, 'editor-bin');
  mkdirSync(bin);
  launchLog = join(f.dir, 'zed-args.json');
  const zed = join(bin, 'zed');
  writeFileSync(zed, '#!/usr/bin/env node\nrequire("node:fs").writeFileSync(process.env.AIDE_EDITOR_LOG, JSON.stringify(process.argv.slice(2)));\nif (require("node:fs").existsSync(process.env.AIDE_EDITOR_LOG + ".fail")) process.exit(1);\n');
  chmodSync(zed, 0o755);
  f.env.PATH = `${bin}:${f.env.PATH}`;
  f.env.AIDE_EDITOR_LOG = launchLog;
  server = await manager(f);
});
test.afterEach(async () => {if(server) await stop(server); if(f) cleanup(f);});

async function config(page) {
  await page.goto(server.url);
  await expect(page.locator('[data-machine=local]')).toHaveAttribute('title', /Connected/);
  await page.getByRole('link', {name:'AI configuration', exact:true}).click();
  await expect(page.getByRole('link', {name:'methodology.md', exact:true})).toBeVisible();
}

test('find, view, copy line references and launch external editor without writes', async ({page, context}) => {
  await context.grantPermissions(['clipboard-read','clipboard-write']);
  await config(page);
  await page.getByLabel('Find files').fill('SKILL');
  await expect(page.getByRole('link', {name:'methodology.md', exact:true})).toHaveCount(0);
  await page.getByRole('link', {name:'skills/example/SKILL.md'}).click();
  await expect(page.getByLabel('File contents')).toContainText('Supporting workflow');
  await page.getByLabel('Find files').fill('');
  await page.getByRole('link', {name:'methodology.md', exact:true}).click();
  await page.getByRole('button', {name:'Select line 2', exact:true}).click();
  await page.getByRole('button', {name:'Copy reference', exact:true}).click();
  await expect.poll(() => page.evaluate(() => navigator.clipboard.readText())).toBe(`${root}/methodology.md:2`);
  await page.getByRole('button', {name:'Open in Zed', exact:true}).click();
  await expect.poll(() => existsSync(launchLog) ? JSON.parse(readFileSync(launchLog,'utf8')) : []).toEqual([`${root}/methodology.md:2`]);
  await page.getByRole('button', {name:'Open folder in Zed'}).click();
  await expect.poll(() => JSON.parse(readFileSync(launchLog,'utf8'))).toEqual([root]);
  expect(readFileSync(join(root,'methodology.md'),'utf8')).toBe('# Working rules\nUse meaningful tests.\n');
  writeFileSync(`${launchLog}.fail`, '');
  await page.getByRole('button', {name:'Open in Zed', exact:true}).click();
  await expect(page.getByRole('status')).toContainText('Zed failed');
  await page.setViewportSize({width:390,height:844});
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
});

test('navigation retains task view and supports history, reload and external refresh', async ({page}) => {
  await page.goto(server.url);
  await page.getByRole('checkbox', {name:'Show completed'}).check();
  await page.getByRole('button', {name:'Cat Herder', exact:true}).click();
  await page.getByRole('link', {name:'AI configuration', exact:true}).click();
  await page.getByRole('link', {name:'methodology.md', exact:true}).click();
  await expect(page.getByLabel('File contents')).toContainText('Use meaningful tests.');
  await page.getByRole('link', {name:'projects.json', exact:true}).click();
  await expect(page.getByLabel('File contents')).toContainText('malformed mappings');
  await page.goBack();
  await expect(page.getByLabel('File contents')).toContainText('Use meaningful tests.');
  await page.goForward();
  await expect(page.getByLabel('File contents')).toContainText('malformed mappings');
  await page.getByRole('link', {name:'Tasks', exact:true}).click();
  await expect(page.getByRole('checkbox', {name:'Show completed'})).toBeChecked();
  await expect(page.getByRole('button', {name:'All projects'})).toHaveAttribute('aria-pressed', 'false');
  await page.goBack();
  await page.reload();
  await expect(page.getByLabel('File contents')).toContainText('malformed mappings');
  writeFileSync(join(root,'projects.json'), 'External change');
  await page.getByRole('button', {name:'Refresh', exact:true}).click();
  await expect(page.getByLabel('File contents')).toContainText('External change');
  unlinkSync(join(root,'projects.json'));
  await page.getByRole('button', {name:'Refresh', exact:true}).click();
  await expect(page.getByRole('alert')).toContainText('Cannot access projects.json');
  await expect(page.getByRole('link', {name:'projects.json', exact:true})).toHaveCount(0);
});

test('protocol confinement, correlated errors and absent configuration', async ({page, request}) => {
  const denied = await request.get(`${server.url}/ws`, {headers:{Origin:'https://untrusted.example',Connection:'Upgrade',Upgrade:'websocket','Sec-WebSocket-Version':'13','Sec-WebSocket-Key':'dGhlIHNhbXBsZSBub25jZQ=='}});
  expect(denied.status()).toBe(403);
  await config(page);
  const responses = await page.evaluate(async () => {
    const ws = new WebSocket(`ws://${location.host}/ws`);
    await new Promise(resolve => ws.addEventListener('open', resolve, {once:true}));
    const messages = [
      {request_id:'missing', operation:{operation:'read',path:'skills/missing.md'}},
      {request_id:'escape', operation:{operation:'read',path:'../manager.json'}},
      {request_id:'listing', operation:{operation:'list'}},
    ];
    return await new Promise(resolve => {
      const results = {};
      ws.onmessage = event => {const value=JSON.parse(event.data); if(value.type==='configuration') {results[value.request_id]=value.result; if(Object.keys(results).length===3) {ws.close();resolve(results);}}};
      for (const message of messages) ws.send(JSON.stringify({action:'configuration',...message}));
    });
  });
  expect(responses.escape.Err).toContain('Unsupported configuration path');
  expect(responses.missing.Err).toContain('Cannot access');
  expect(responses.listing.Ok.files).toContain('methodology.md');
  await stop(server);
  f.env.AIDE_HOME = join(f.dir, 'empty-home');
  server = await manager(f);
  await page.goto(`${server.url}/#/configuration`);
  await expect(page.getByText('The configuration folder does not exist yet.', {exact:false})).toBeVisible();
  expect(existsSync(join(f.env.AIDE_HOME,'agent-config'))).toBe(false);
  await expect(page.getByRole('button', {name:'Open folder in Zed'})).toBeDisabled();
});


test('ignores late file responses', async ({page}) => {
  await page.routeWebSocket('**/ws', socket => {
    const upstream = socket.connectToServer();
    upstream.onMessage(message => {
      const event = JSON.parse(message);
      if(event.type === 'configuration' && event.result.Ok?.kind === 'document' && event.result.Ok.path === 'methodology.md') {
        setTimeout(() => socket.send(message), 700);
      } else socket.send(message);
    });
  });
  await config(page);
  await page.getByRole('link', {name:'methodology.md', exact:true}).click();
  await page.getByRole('link', {name:'projects.json', exact:true}).click();
  await expect(page.getByLabel('File contents')).toContainText('malformed mappings');
  await page.waitForTimeout(900);
  await expect(page.getByLabel('File contents')).toContainText('malformed mappings');
});

test('reconnection reloads external edits and viewer errors recover', async ({page}) => {
  await page.addInitScript(() => {
    const NativeWebSocket = window.WebSocket;
    window.WebSocket = class extends NativeWebSocket {
      constructor(...args) {super(...args); window.testSocket = this;}
    };
  });
  await config(page);
  await page.getByRole('link', {name:'projects.json', exact:true}).click();
  await expect(page.getByLabel('File contents')).toContainText('malformed mappings');
  writeFileSync(join(root,'projects.json'), 'Changed while disconnected');
  await page.evaluate(() => window.testSocket.close());
  await expect(page.getByLabel('File contents')).toContainText('Changed while disconnected');
  writeFileSync(join(root,'projects.json'), Buffer.from([255]));
  await page.getByRole('button', {name:'Refresh', exact:true}).click();
  await expect(page.locator('.config-viewer [role=alert]')).toContainText('not UTF-8');
  writeFileSync(join(root,'projects.json'), 'Recovered');
  await page.getByRole('button', {name:'Refresh', exact:true}).click();
  await expect(page.getByLabel('File contents')).toContainText('Recovered');
});

test('configuration cog is keyboard accessible and long source lines wrap without changing references', async ({page, context}) => {
  const prose = '  Keep this indented instruction readable. '.repeat(12);
  const token = 'https://example.com/' + 'long-path-segment'.repeat(30);
  writeFileSync(join(root, 'methodology.md'), `${prose}\n${token}\nLast line`);
  await context.grantPermissions(['clipboard-read','clipboard-write']);
  await page.goto(server.url);
  const cog = page.getByRole('link', {name:'AI configuration', exact:true});
  await expect(cog).toHaveAttribute('title', 'AI configuration');
  await expect(cog.locator('svg')).toBeVisible();
  await cog.focus();
  await page.keyboard.press('Enter');
  await expect(cog).toHaveAttribute('aria-current', 'page');
  await page.getByRole('link', {name:'methodology.md', exact:true}).click();
  const source = page.getByLabel('File contents');
  await expect(source.locator('.config-line')).toHaveCount(3);
  for (const width of [1440, 390]) {
    await page.setViewportSize({width, height:900});
    await expect.poll(() => source.evaluate(element => element.scrollWidth <= element.clientWidth)).toBe(true);
    const wrapped = await source.locator('code').first().evaluate(element => ({height:element.getBoundingClientRect().height, lineHeight:parseFloat(getComputedStyle(element).lineHeight), text:element.textContent}));
    expect(wrapped.height).toBeGreaterThan(wrapped.lineHeight * 2);
    expect(wrapped.text).toBe(prose);
    await page.getByRole('button', {name:'Select line 2', exact:true}).click();
    await page.getByRole('button', {name:'Copy reference', exact:true}).click();
    await expect.poll(() => page.evaluate(() => navigator.clipboard.readText())).toBe(`${root}/methodology.md:2`);
  }
});

test('headless configuration keeps browsing and copying without desktop actions', async ({page, context}) => {
  await stop(server);
  f.env.AIDE_DESKTOP = '0';
  server = await manager(f);
  await context.grantPermissions(['clipboard-read','clipboard-write']);
  await config(page);
  await expect(page.getByRole('button', {name:/Zed/})).toHaveCount(0);
  await expect(page.getByText('Edit files on this machine using SSH or your preferred remote editor, then refresh.', {exact:true})).toBeVisible();
  await page.getByRole('button', {name:'Copy folder path',exact:true}).click();
  await expect.poll(() => page.evaluate(() => navigator.clipboard.readText())).toBe(root);
  await page.getByRole('link', {name:'methodology.md',exact:true}).click();
  await expect(page.getByLabel('File contents')).toContainText('Use meaningful tests.');
  await page.getByRole('button', {name:'Select line 2',exact:true}).click();
  await page.getByRole('button', {name:'Copy reference',exact:true}).click();
  await expect.poll(() => page.evaluate(() => navigator.clipboard.readText())).toBe(`${root}/methodology.md:2`);
  writeFileSync(join(root,'methodology.md'), 'Updated remotely');
  await page.getByRole('button', {name:'Refresh',exact:true}).click();
  await expect(page.getByLabel('File contents')).toContainText('Updated remotely');
  await page.reload();
  await expect(page.getByLabel('File contents')).toContainText('Updated remotely');
  await expect(page.getByRole('button', {name:/Zed/})).toHaveCount(0);
});

test('headless API rejects file and folder editor launches with correlated errors', async ({page}) => {
  await stop(server);
  f.env.AIDE_DESKTOP = '0';
  server = await manager(f);
  await config(page);
  const results = await page.evaluate(async () => {
    const ws = new WebSocket(`ws://${location.host}/ws`);
    await new Promise(resolve => ws.addEventListener('open',resolve,{once:true}));
    return new Promise(resolve => {
      const results = {};
      ws.onmessage = event => {
        const value = JSON.parse(event.data);
        if (value.type !== 'configuration') return;
        results[value.request_id] = value.result;
        if (Object.keys(results).length === 2) {ws.close();resolve(results);}
      };
      for (const [id,path] of [['folder',null],['file','methodology.md']]) {
        ws.send(JSON.stringify({action:'configuration',request_id:id,operation:{operation:'open',path,line:2}}));
      }
    });
  });
  expect(results.folder.Err).toContain('cannot launch desktop applications');
  expect(results.file.Err).toContain('cannot launch desktop applications');
  expect(existsSync(launchLog)).toBe(false);
});
