import { test, expect } from '@playwright/test';
import { mkdtempSync, writeFileSync, readFileSync, readdirSync, mkdirSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { spawn, execFileSync } from 'node:child_process';

const binary = resolve('target/debug/aide-review');
function fixture() {
  const dir = mkdtempSync(join(tmpdir(), 'aide-e2e-'));
  execFileSync('git', ['init', '-q', dir]);
  writeFileSync(join(dir, 'calculator.rs'), 'pub fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n');
  return dir;
}
async function start(dir, port = 0, approval = null) {
  const child = spawn(binary, ['--path', dir, ...(approval ? [] : ['--fixture']), '--no-open', '--port', String(port)], {stdio: ['ignore', 'pipe', 'pipe'], env: {...process.env, ...(approval ? {PATH: `${resolve('tests/fixtures')}:${process.env.PATH}`, REVIEW_TEST_APPROVAL: approval} : {})}});
  let log = '';
  const url = await new Promise((resolve, reject) => {
    const timeout = setTimeout(() => reject(new Error(`Server startup timed out: ${log}`)), 15000);
    child.stdout.on('data', data => { log += data; const m = log.match(/Review: (http:\/\/127\.0\.0\.1:\d+)/); if (m) {clearTimeout(timeout); resolve(m[1]);}});
    child.stderr.on('data', data => { log += data; });
    child.on('exit', code => {clearTimeout(timeout); reject(new Error(`Server exited ${code}: ${log}`));});
  });
  return {child, url, port: Number(new URL(url).port), output: () => log};
}
async function stop(server) {if (server.child.exitCode !== null || server.child.signalCode !== null) return;await new Promise(resolve => {server.child.once('exit', resolve); server.child.kill('SIGKILL');});}
async function ready(page, url) {await page.goto(url); await expect(page.locator('#guide .code-line').first()).toBeVisible();}
async function saved(page) {await expect.poll(() => page.evaluate(() => Object.keys(localStorage).filter(k => k.startsWith('review-outbox-')).every(k => JSON.parse(localStorage[k]).length === 0))).toBe(true);}
async function selectProse(page) {
  const p = page.locator('#guide .prose p').first();
  // Real pointer selection across the first line, then verify selected text and active context.
  const box = await p.boundingBox();
  await page.mouse.move(box.x + 1, box.y + 10);
  await page.mouse.down(); await page.mouse.move(box.x + Math.min(250, box.width - 2), box.y + 10, {steps: 12}); await page.mouse.up();
  const quote = await page.evaluate(() => getSelection().toString().trim());
  expect(quote.length).toBeGreaterThan(5);
  await expect(page.getByRole('toolbar')).toHaveAttribute('data-reference', `Guide: ${quote}`);
  return quote;
}

test('syntax colours preserve exact multilingual source, token selection, feedback and restoration', async ({page}) => {
  const samples = JSON.parse(readFileSync('tests/fixtures/syntax.json', 'utf8'));
  const dir = fixture();
  for (const sample of samples) writeFileSync(join(dir, sample.path), sample.source);
  let server = await start(dir);
  try {
    await ready(page, server.url);
    for (const sample of samples) {
      const rows = page.locator(`.code-line[data-file="${sample.path}"] code`);
      expect(await rows.allTextContents()).toEqual(sample.source.trimEnd().split('\n'));
      const tokens = rows.locator(`.syntax-${sample.category}`);
      expect((await tokens.allTextContents()).join(' ')).toContain(sample.token);
      const styles = await tokens.first().evaluate(el => ({ colour: getComputedStyle(el).color, background: getComputedStyle(el).backgroundColor, base: getComputedStyle(el.closest('code')).color }));
      expect(styles.colour).not.toBe(styles.base);
      expect(styles.background).toBe('rgba(0, 0, 0, 0)');
    }
    await expect(page.locator('#guide script')).toHaveCount(0);
    await expect(page.locator('#guide .unmapped')).toHaveCount(0);

    // Measure the rendered palette against actual CSS backgrounds, including the
    // uniform feedback overlay. The probe leaves real source rows and anchors intact.
    const contrast = await page.locator('.code-rows').first().evaluate(container => {
      const rgb = value => value.match(/[\d.]+/g).map(Number);
      const luminance = colour => colour.slice(0, 3).reduce((sum, value, i) => {
        const channel = value / 255;
        return sum + [0.2126, 0.7152, 0.0722][i] * (channel <= 0.04045 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4);
      }, 0);
      const probe = document.createElement('div');
      probe.style.cssText = 'position:absolute;visibility:hidden';
      const code = document.createElement('code');
      const classes = new Set([...document.querySelectorAll('#guide code [class^="syntax-"]')].map(el => el.className));
      for (const name of classes) {
        const span = document.createElement('span'); span.className = name; code.append(span);
      }
      probe.append(code); container.append(probe);
      const results = [];
      const gutters = [];
      try {
        for (const kind of ['context', 'added', 'removed']) for (const selected of [false, true]) for (const feedback of [false, true]) {
          probe.className = `code-line ${kind}`;
          probe.dataset.selected = String(selected); probe.dataset.feedback = String(feedback);
          let element = probe;
          while (getComputedStyle(element).backgroundColor === 'rgba(0, 0, 0, 0)') element = element.parentElement;
          let background = rgb(getComputedStyle(element).backgroundColor);
          const overlay = getComputedStyle(probe).backgroundImage.match(/rgba?\([^)]+\)/);
          if (overlay) {
            const tint = rgb(overlay[0]);
            background = background.map((value, i) => tint[i] * tint[3] + value * (1 - tint[3]));
          }
          const border = getComputedStyle(probe);
          gutters.push({ kind, colour: border.borderLeftColor, width: border.borderLeftWidth,
            ratio: (luminance(background) + 0.05) / (luminance(rgb(border.borderLeftColor)) + 0.05) });
          for (const token of [code, ...code.children]) {
            const foreground = rgb(getComputedStyle(token).color);
            results.push({ token: token.className || 'plain', kind, selected, feedback, ratio: (luminance(background) + 0.05) / (luminance(foreground) + 0.05) });
          }
        }
      } finally { probe.remove(); }
      return { tokens: results, gutters };
    });
    for (const result of contrast.tokens) expect(result.ratio, JSON.stringify(result)).toBeGreaterThanOrEqual(3.5);
    for (const kind of ['added', 'removed']) {
      const gutters = contrast.gutters.filter(gutter => gutter.kind === kind);
      // Diff markers remain visible and unchanged under selection and feedback.
      expect(new Set(gutters.map(gutter => gutter.colour)).size).toBe(1);
      for (const gutter of gutters) {
        expect(gutter.width).toBe('3px');
        expect(gutter.ratio).toBeGreaterThanOrEqual(1.5);
        expect(gutter.colour).not.toBe('rgba(0, 0, 0, 0)');
      }
    }
    expect(contrast.gutters.find(gutter => gutter.kind === 'added').colour)
      .not.toBe(contrast.gutters.find(gutter => gutter.kind === 'removed').colour);

    const token = page.locator('.code-line[data-file="sample.ts"][data-line="2"] .syntax-function').first();
    await token.click();
    await expect(page.getByRole('toolbar')).toHaveAttribute('data-reference', 'sample.ts:2-2 (new)');
    await page.getByRole('button', {name:'Discuss with agent', exact:true}).click();
    await page.getByRole('textbox', {name:'Ask Codex', exact:true}).fill('Explain the generic parameter.');
    await page.getByRole('button', {name:'Send', exact:true}).click();
    await expect(page.locator('.message.user .message-reference')).toContainText('export function greet<T extends string>');
    await token.click();
    await page.getByRole('button', {name:'Add feedback', exact:true}).click();
    await page.getByRole('textbox', {name:'Feedback comment'}).fill('Keep this generic signature.');
    await page.getByRole('button', {name:'Record feedback'}).click();
    await saved(page);
    const port = server.port;
    await stop(server);
    server = await start(dir, port);
    await page.reload();
    await expect(token).toBeVisible();
    await expect(page.locator('.code-line[data-file="sample.ts"][data-line="2"]')).toHaveAttribute('data-feedback', 'true');
    await expect(page.locator('.feedback-card')).toContainText('Keep this generic signature.');
    await page.getByRole('button', {name:'Overall feedback', exact:false}).click();
    await page.getByRole('button', {name:'Submit review'}).click();
    await expect.poll(() => server.child.exitCode).toBe(0);
    const output = readFileSync(join(dir, readdirSync(dir).find(name => name.startsWith('review-feedback-'))), 'utf8');
    expect(output).toContain('sample.ts:2-2');
    expect(output).toContain('Keep this generic signature.');
    expect(output).not.toContain('syntax-');
  } finally { await stop(server); }
});

test('selection actions, separate context, categories, drafts, restart and direct export', async ({page}) => {
  const dir=fixture(); let server=await start(dir);
  try {
    await ready(page,server.url);
    await page.getByRole('button',{name:'Hide chat',exact:true}).click();
    await page.locator('.code-line[data-line="2"]').click();
    await page.getByRole('button',{name:'Discuss with agent',exact:true}).click();
    const chat=page.getByRole('textbox',{name:'Ask Codex',exact:true});
    await expect(chat).toBeFocused();
    await expect(page.locator('.reference')).toContainText('calculator.rs:2-2');
    await page.locator('.code-line[data-line="1"]').click();
    await expect(page.locator('.reference')).toContainText('calculator.rs:2-2');
    await chat.fill('Why addition?'); await chat.press('Enter');
    await expect(page.locator('.message.user .message-reference')).toContainText('a + b');
    await page.locator('.code-line[data-line="2"]').click();
    await page.getByRole('button',{name:'Add feedback',exact:true}).click();
    await expect(page.getByRole('textbox',{name:'Feedback comment'})).toBeFocused();
    await page.getByRole('textbox',{name:'Feedback comment'}).fill('Cover negative inputs.');
    await page.getByRole('button',{name:'Record feedback'}).click();
    const quote=await selectProse(page);
    await page.getByRole('button',{name:'Add feedback',exact:true}).click();
    await page.getByRole('radio',{name:'Question',exact:true}).click();
    await page.getByRole('textbox',{name:'Feedback comment'}).fill('Why this approach?');
    await page.getByRole('button',{name:'Record feedback'}).click();
    await expect(page.locator('.feedback-card')).toHaveCount(2);
    await expect(page.locator('.code-line[data-line="2"]')).toHaveAttribute('data-feedback','true');
    await page.getByRole('button',{name:'Overall feedback',exact:false}).click();
    await page.getByRole('textbox',{name:'Overall feedback',exact:true}).fill('Overall: looks promising.');
    await page.locator('.code-line[data-line="3"]').click();
    await page.getByRole('button',{name:'Add feedback',exact:true}).click();
    await page.getByRole('textbox',{name:'Feedback comment'}).fill('Unfinished feedback');
    await chat.fill('Unfinished question');
    await page.getByRole('button',{name:'Hide chat',exact:true}).click();
    await page.getByRole('button',{name:'Overall feedback',exact:false}).click();
    await saved(page); const port=server.port; await stop(server); server=await start(dir,port);
    await page.reload();
    await expect(page.getByRole('textbox',{name:'Ask Codex',exact:true})).toHaveCount(0);
    await expect(page.getByRole('textbox',{name:'Overall feedback',exact:true})).toHaveCount(0);
    await expect(page.getByRole('textbox',{name:'Feedback comment'})).toHaveValue('Unfinished feedback');
    await expect(page.locator('.feedback-card')).toHaveCount(3);
    await expect(page.locator('.category-badge')).toHaveText(['Change','Question']);
    await page.getByRole('button',{name:'Show chat',exact:true}).click();
    await expect(chat).toHaveValue('Unfinished question');
    await page.getByRole('button',{name:'Overall feedback',exact:false}).click();
    await expect(page.getByRole('textbox',{name:'Overall feedback',exact:true})).toHaveValue('Overall: looks promising.');
    await page.getByRole('button',{name:'Submit review'}).click();
    await expect(page.locator('.completion')).toBeVisible();
    await expect.poll(()=>server.child.exitCode).toBe(0);
    const filename=readdirSync(dir).find(f=>f.startsWith('review-feedback-'));
    const output=readFileSync(join(dir,filename),'utf8');
    expect(output).toContain('# Overall feedback\n\nOverall: looks promising.');
    expect(output).toContain('# Changes'); expect(output).toContain('# Questions');
    expect(output.indexOf('Cover negative inputs.')).toBeLessThan(output.indexOf('# Questions'));
    expect(output.indexOf('Why this approach?')).toBeGreaterThan(output.indexOf('# Questions'));
    expect(output).toContain(`> ${quote}`); expect(output).toContain('calculator.rs:2-2');
    expect(output).not.toContain('Unfinished feedback'); expect(output).not.toContain('Why addition?');
    expect(readdirSync(dir).filter(n=>n.startsWith('.review'))).toEqual([]);
    expect(await page.evaluate(()=>Object.keys(localStorage).filter(k=>k.startsWith('review-outbox-')))).toEqual([]);
    server=await start(dir,port); await ready(page,server.url);
    await expect(page.locator('.feedback-card')).toHaveCount(0); await expect(page.locator('.message')).toHaveCount(0);
    expect(readFileSync(join(dir,filename),'utf8')).toBe(output);
  } finally {await stop(server);}
});

test('manual references, feedback editing, deletion, and changed-code archive', async ({page}) => {
 const dir=fixture(); let server=await start(dir);
 try {
  await ready(page,server.url); await page.getByText('Reference a file range',{exact:true}).click();
  await page.getByLabel('Source file',{exact:true}).fill('calculator.rs');
  await page.getByRole('spinbutton',{name:'Start line'}).fill('1'); await page.getByRole('spinbutton',{name:'End line'}).fill('3');
  await page.getByRole('button',{name:'Use',exact:true}).click(); await expect(page.locator('.reference')).toContainText('calculator.rs:1-3');
  await page.locator('.code-line[data-line="2"]').click(); await page.getByRole('button',{name:'Add feedback',exact:true}).click();
  await page.getByRole('textbox',{name:'Feedback comment'}).fill('Initial feedback'); await page.getByRole('button',{name:'Record feedback'}).click();
  await page.getByRole('button',{name:'Edit',exact:true}).click(); await page.getByRole('textbox',{name:'Feedback comment'}).fill('Revised feedback');
  await page.getByRole('radio',{name:'Question',exact:true}).click(); await page.getByRole('button',{name:'Record feedback'}).click();
  await expect(page.locator('.feedback-card')).toContainText('Revised feedback'); await expect(page.locator('.category-badge')).toHaveText('Question');
  await page.getByRole('button',{name:'Clear reference'}).click(); await expect(page.locator('.reference')).toContainText('WHOLE REVIEW');
  await expect(page.locator('.code-line[data-line="2"]')).toHaveAttribute('data-feedback','true');
  await page.getByRole('button',{name:'Delete',exact:true}).click();
  await expect(page.locator('.code-line[data-line="2"]')).toHaveAttribute('data-feedback','false');
  await page.getByRole('button',{name:'Overall feedback',exact:false}).click();
  writeFileSync(join(dir,'calculator.rs'),'pub fn add(a: i32, b: i32) -> i32 {\n    a + b + 1\n}\n');
  await page.getByRole('textbox',{name:'Overall feedback',exact:true}).fill('Keep the original snapshot'); await saved(page);
  await expect(page.locator('#guide')).not.toContainText('a + b + 1');
  const port=server.port; await stop(server); server=await start(dir,port); await ready(page,server.url);
  await expect(page.getByText('Previous review is archived',{exact:false})).toBeVisible();
  await expect(page.locator('.feedback-card')).toHaveCount(0); await expect(page.locator('#guide')).toContainText('a + b + 1');
 } finally {await stop(server);}
});

test('approval, question, form, interruption and recovered partial response', async ({page}) => {
 const dir=fixture();let server=await start(dir);
 try {
  await ready(page,server.url);
  const ask=async(text)=>{await page.getByRole('textbox',{name:'Ask Codex',exact:true}).fill(text);await page.getByRole('button',{name:'Send',exact:true}).click();};
  await ask('fixture:approval');await expect(page.getByText('Codex needs your approval',{exact:true})).toBeVisible();await page.getByRole('button',{name:'Approve once'}).click();await expect(page.locator('.agent-request')).toHaveCount(0);
  await ask('fixture:question');await page.getByRole('button',{name:'Arithmetic',exact:true}).click();await page.getByRole('button',{name:'Send response',exact:true}).click();await expect(page.locator('.agent-request')).toHaveCount(0);
  await ask('fixture:form');await page.getByLabel('Review label',{exact:true}).fill('First pass');await page.getByRole('button',{name:'Send response',exact:true}).click();await expect(page.locator('.agent-request')).toHaveCount(0);
  await ask('fixture:slow');await expect(page.locator('.message').last()).toContainText('streaming');await expect(page.locator('.message').last()).toContainText('Partial response');await page.getByRole('button',{name:'Stop',exact:true}).click();await expect(page.locator('.message').last()).toContainText('interrupted');
  await ask('fixture:slow');await expect(page.locator('.message').last()).toContainText('streaming');await expect(page.locator('.message').last()).toContainText('Partial response');await saved(page);const port=server.port;await stop(server);server=await start(dir,port);await page.reload();await expect(page.locator('.message').last()).toContainText('interrupted');await expect(page.getByText('Agent work was interrupted',{exact:false})).toBeVisible();
  await ask('Continue after interruption');await expect(page.locator('.message').last()).toContainText('Fixture response to: Continue after interruption');
 } finally {await stop(server);}
});

test('durable command IDs prevent duplicate feedback after an uncertain connection', async ({page}) => {
 const dir=fixture();const server=await start(dir);
 try {
  await ready(page,server.url);
  await page.evaluate(async () => {
   const request = async () => new Promise((resolve,reject) => {
    const ws = new WebSocket(`ws://${location.host}/ws`);let sent=false;
    ws.onerror=reject;ws.onmessage=e=>{const message=JSON.parse(e.data);if(message.type==='snapshot'&&!sent){sent=true;ws.send(JSON.stringify({id:'retry-test',review_id:message.review.id,command:{type:'feedback',id:'comment-test',text:'Only once',category:'change',prose_offset:null,code_block:0,reference:{kind:'code',file:'calculator.rs',start:1,end:1,side:'new'}}}));}if(message.type==='ack'){ws.close();resolve();}};
   });await request();await request();
  });
  await expect(page.locator('.feedback-card')).toHaveCount(1);
 }finally{await stop(server);}
});

test('reading position and source selection survive a fresh browser page', async ({page}) => {
 const dir=fixture();writeFileSync(join(dir,'long.rs'),Array.from({length:120},(_,i)=>`// source line ${i+1}`).join('\n')+'\n');const server=await start(dir);
 try {
  await ready(page,server.url);await page.locator('.code-line[data-file="long.rs"][data-line="80"]').click();await saved(page);
  const scroll=await page.locator('#reading').evaluate(el=>el.scrollTop);expect(scroll).toBeGreaterThan(500);
  await page.reload();await expect(page.getByRole('toolbar')).toHaveAttribute('data-reference',/long.rs:80-80/);
  await expect.poll(()=>page.locator('#reading').evaluate(el=>el.scrollTop)).toBeCloseTo(scroll,0);
  await expect(page.locator('.code-line[data-file="long.rs"][data-line="80"]')).toHaveAttribute('data-selected','true');
 }finally{await stop(server);}
});


test('independent panels, persistent prose feedback, and single overall field', async ({page}) => {
 const dir=fixture(); const server=await start(dir);
 try {
  await ready(page,server.url);
  const panel=await page.locator('aside').boundingBox(), guide=await page.locator('main').boundingBox();
  expect(panel.x+panel.width).toBeLessThanOrEqual(guide.x);
  const title=await page.locator('#guide h1').first().boundingBox(), metadata=await page.locator('.document-heading').boundingBox();
  expect(title.y+title.height).toBeLessThanOrEqual(metadata.y);
  const quote=await selectProse(page);
  await page.getByRole('button',{name:'Add feedback',exact:true}).click();
  await page.getByRole('textbox',{name:'Feedback comment'}).fill('Prose feedback'); await page.getByRole('button',{name:'Record feedback'}).click();
  await page.locator('.code-line').first().click();
  const highlighted=()=>page.evaluate(()=>[...(CSS.highlights.get('review-changes')||[])].map(r=>r.toString()).join(''));
  await expect.poll(highlighted).toBe(quote);
  await saved(page); await page.reload(); await expect.poll(highlighted).toBe(quote);
  await page.getByRole('button',{name:'Hide chat',exact:true}).click();
  const wider=await page.locator('main').boundingBox(); expect(wider.width).toBeGreaterThan(guide.width);
  await page.getByRole('button',{name:'Delete',exact:true}).click(); await expect.poll(highlighted).toBe('');
  await page.getByRole('button',{name:'Overall feedback',exact:false}).click();
  const overall=page.getByRole('textbox',{name:'Overall feedback',exact:true});
  await overall.fill('Initial overall assessment'); await overall.fill('Final overall assessment');
  await page.getByRole('button',{name:'Submit review'}).click(); await expect.poll(()=>server.child.exitCode).toBe(0);
  const output=readFileSync(join(dir,readdirSync(dir).find(n=>n.startsWith('review-feedback-'))),'utf8');
  expect(output).toBe('# Overall feedback\n\nFinal overall assessment\n\n');
 } finally {await stop(server);}
});

test('CLI conclusion exports the saved review even after workspace edits', async ({page}) => {
  const dir=fixture(); const server=await start(dir);
  try {
    await ready(page,server.url);
    await page.getByRole('button',{name:'Overall feedback',exact:false}).click();
    await page.getByRole('textbox',{name:'Overall feedback',exact:true}).fill('Preserve this reviewed snapshot');
    await saved(page); await stop(server);
    writeFileSync(join(dir,'calculator.rs'),'// changes after reviewing\n');
    const path=execFileSync(binary,['--path',dir,'conclude'],{encoding:'utf8'}).trim();
    expect(readFileSync(path,'utf8')).toContain('Preserve this reviewed snapshot');
    expect(readdirSync(dir).filter(name=>name.startsWith('review.sqlite') || name.startsWith('.review'))).toEqual([]);
    expect(()=>execFileSync(binary,['--path',dir,'history'],{stdio:'pipe'})).toThrow();
    expect(readdirSync(dir).filter(name=>name.startsWith('review.sqlite') || name.startsWith('.review'))).toEqual([]);
  } finally { await stop(server); }
});


test('failed export leaves the review usable and does not overwrite a handover', async ({page}) => {
  const dir=fixture(); const server=await start(dir);
  try {
    await ready(page,server.url);
    const id=await page.evaluate(()=>new Promise(resolve=>{
      const ws=new WebSocket(`ws://${location.host}/ws`);
      ws.onmessage=e=>{ const event=JSON.parse(e.data); if(event.type==='snapshot'){ws.close();resolve(event.review.id);} };
    }));
    const path=join(dir,`review-feedback-${id}.md`);
    writeFileSync(path,'An existing handover');
    await page.getByRole('button',{name:'Overall feedback',exact:false}).click();
    await page.getByRole('textbox',{name:'Overall feedback',exact:true}).fill('Recorded assessment');
    await page.getByRole('button',{name:'Submit review'}).click();
    await expect(page.getByRole('alert')).toContainText('already exists with different content');
    await expect(page.locator('.completion')).toHaveCount(0);
    expect(server.child.exitCode).toBeNull();
    expect(readdirSync(join(dir,'.review'))).toContain('review.sqlite');
    expect(readdirSync(join(dir,'.review'))).toContain('lock');
    expect(readFileSync(path,'utf8')).toBe('An existing handover');
    await page.getByRole('textbox',{name:'Overall feedback',exact:true}).fill('Still editable after failed export');
    await saved(page); await page.reload();
    await expect(page.getByRole('textbox',{name:'Overall feedback',exact:true})).toHaveValue('Still editable after failed export');
  } finally { await stop(server); }
});


for (const kind of ['command', 'mcp']) {
  test(`initial preparation exposes ${kind} approval and continues after reply`, async ({page}) => {
    const dir=fixture(); const server=await start(dir,0,kind);
    try {
      await page.setViewportSize({width:1280,height:600});
      await page.goto(server.url);
      const approve=page.getByRole('button',{name:'Approve once',exact:true});
      await expect(approve).toBeInViewport();
      await expect(page.locator('#guide')).toBeEmpty();
      await expect(page.locator('.guide-loading')).toContainText('waiting for your response');
      await expect(page.locator('.message')).toHaveCount(0);
      if (kind === 'command') {
        await page.getByRole('button',{name:'Decline',exact:true}).click();
        await expect(page.locator('.agent-request')).toHaveCount(0);
        await expect(page.locator('#guide')).toBeEmpty();
        await page.getByRole('button',{name:'Retry preparation',exact:true}).click();
        await expect(approve).toBeInViewport();
      }
      // Requests remain discoverable with chat collapsed and after reload.
      await page.getByRole('button',{name:'Hide chat',exact:true}).click();
      await saved(page); await page.reload();
      await page.getByRole('button',{name:'Codex needs your response',exact:true}).click();
      await expect(approve).toBeInViewport();
      await approve.click();
      await expect(page.locator('#guide')).toContainText('Prepared after your approval.');
      await expect(page.locator('.agent-request')).toHaveCount(0);
      await expect(page.locator('.guide-loading')).toHaveCount(0);
      await expect(page.locator('.message')).toHaveCount(0);
    } finally {await stop(server);}
  });
}

test('preparation clock advances across reload, resets on retry and stops on completion', async ({page}) => {
  const dir = fixture(); const server = await start(dir, 0, 'command');
  const seconds = async () => {
    const text = await page.getByRole('timer').textContent();
    const [, minutes, seconds] = text.match(/Elapsed (\d+):(\d+)/);
    return Number(minutes) * 60 + Number(seconds);
  };
  try {
    await page.goto(server.url);
    await expect(page.getByRole('timer')).toContainText('Elapsed');
    await expect.poll(seconds).toBeGreaterThanOrEqual(2);
    const before = await seconds();
    await page.reload();
    await expect.poll(seconds).toBeGreaterThanOrEqual(before);
    await expect.poll(server.output).toContain('Waiting for your response');
    await page.getByRole('button', {name:'Decline', exact:true}).click();
    await expect(page.getByRole('timer')).toHaveCount(0);
    await expect.poll(server.output).toContain('Preparation stopped');
    await page.getByRole('button', {name:'Retry preparation', exact:true}).click();
    await expect(page.getByRole('timer')).toHaveText('Elapsed 0:00');
    await expect.poll(seconds).toBeGreaterThanOrEqual(1);
    await page.getByRole('button', {name:'Approve once', exact:true}).click();
    await expect(page.locator('#guide')).toContainText('Prepared after your approval.');
    await expect(page.getByRole('timer')).toHaveCount(0);
    await expect.poll(server.output).toContain('Walkthrough ready');
  } finally { await stop(server); }
});

async function highlightedProse(page) {
  return page.evaluate(() => [...(CSS.highlights.get('review-reference') || [])].map(range => range.toString()).join(''));
}

test('whole-paragraph selection persists and is quoted in keyboard-submitted chat', async ({page}) => {
  const dir=fixture(); const server=await start(dir);
  try {
    await ready(page,server.url);
    await expect(page.getByRole('heading',{name:'Understand the change'})).toHaveCount(0);
    const paragraph=page.locator('#guide .prose p').first();
    const quote=await paragraph.textContent();
    await paragraph.click({clickCount:3});
    await expect.poll(()=>highlightedProse(page)).toBe(quote);
    const input=page.getByRole('textbox',{name:'Ask Codex',exact:true});
    await page.getByRole('button',{name:'Discuss with agent',exact:true}).click();
    await input.fill('Explain this paragraph');
    await input.dispatchEvent('keydown',{key:'Enter',isComposing:true});
    await expect(page.locator('.message')).toHaveCount(0);
    await expect(input).toHaveValue('Explain this paragraph');
    await expect.poll(()=>highlightedProse(page)).toBe(quote);
    await input.press('Shift+Enter');
    await input.pressSequentially('Especially the tradeoffs');
    await expect(input).toHaveValue('Explain this paragraph\nEspecially the tradeoffs');
    await expect(page.locator('.message')).toHaveCount(0);
    await input.press('Enter');
    await expect(page.locator('.message.user .message-reference')).toContainText(quote);
    await expect(page.locator('.message').last()).toContainText(quote);
    await saved(page); await page.reload();
    await expect(page.locator('.message.user .message-reference')).toContainText(quote);
    await expect.poll(()=>highlightedProse(page)).toBe(quote);
    // Select the full paragraph with a real mouse drag as well.
    await page.getByRole('button',{name:'Clear reference'}).click();
    const box=await paragraph.boundingBox();
    await page.mouse.move(box.x+1,box.y+4);
    await page.mouse.down();
    await page.mouse.move(box.x+box.width-1,box.y+box.height-3,{steps:15});
    await page.mouse.up();
    await expect.poll(()=>highlightedProse(page)).toBe(quote);
    await page.getByRole('button',{name:'Add feedback',exact:true}).click();
    const feedback=page.getByRole('textbox',{name:'Feedback comment'});
    await feedback.fill('Paragraph feedback');
    await feedback.press('Shift+Enter');
    await feedback.pressSequentially('With another line');
    await feedback.press('Enter');
    await expect(page.locator('.feedback-card')).toContainText('Paragraph feedback\nWith another line');
    await expect(page.locator('.feedback-card small')).toContainText(quote);
  } finally {await stop(server);}
});

test('code ranges support dragging, reverse Shift-click, quotation and restoration', async ({page}) => {
  const dir=fixture(); const server=await start(dir);
  try {
    await ready(page,server.url);
    const first=page.locator('.code-line[data-line="1"]');
    const last=page.locator('.code-line[data-line="3"]');
    await first.click();
    await last.click({modifiers:['Shift']});
    await expect(page.getByRole('toolbar')).toHaveAttribute('data-reference', /calculator.rs:1-3/);
    await expect(page.locator('.code-line[data-selected=true]')).toHaveCount(3);
    await saved(page); await page.reload();
    await expect(page.locator('.code-line[data-selected=true]')).toHaveCount(3);
    await last.click();
    await first.click({modifiers:['Shift']});
    await expect(page.getByRole('toolbar')).toHaveAttribute('data-reference', /calculator.rs:1-3/);
    const a=await first.boundingBox(), b=await last.boundingBox();
    await page.mouse.move(a.x+100,a.y+a.height/2); await page.mouse.down();
    await page.mouse.move(b.x+110,b.y+b.height/2,{steps:10}); await page.mouse.up();
    await expect(page.locator('.code-line[data-selected=true]')).toHaveCount(3);
    // Releasing outside the document must end the drag and persist its last range.
    await page.mouse.move(a.x+100,a.y+a.height/2); await page.mouse.down();
    await page.mouse.move(b.x+110,b.y+b.height/2,{steps:10});
    await page.mouse.move(20,20); await page.mouse.up();
    await saved(page); await page.reload();
    await first.hover();
    await expect(page.locator('.code-line[data-selected=true]')).toHaveCount(3);
    const input=page.getByRole('textbox',{name:'Ask Codex',exact:true});
    await page.getByRole('button',{name:'Discuss with agent',exact:true}).click();
    await input.fill('Explain this whole function'); await input.press('Enter');
    await expect(page.locator('.message.user .message-reference')).toContainText('a + b');
    await expect(page.locator('.message').last()).toContainText('a + b');
  } finally {await stop(server);}
});


test('diff range selection retains old or new side through context lines', async ({page}) => {
  const dir=fixture();
  writeFileSync(join(dir,'calculator.rs'),'pub fn add(a: i32, b: i32) -> i32 {\n    a - b\n}\n');
  execFileSync('git',['-C',dir,'add','.']);
  execFileSync('git',['-C',dir,'-c','user.name=Test','-c','user.email=test@example.invalid','commit','-qm','base']);
  writeFileSync(join(dir,'calculator.rs'),'pub fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n');
  const server=await start(dir,0,'diff');
  try {
    await page.goto(server.url); await page.getByRole('button',{name:'Approve once',exact:true}).click();
    await page.locator('.code-line.removed').waitFor();
    await page.locator('.code-line.removed').click();
    await page.locator('.code-line.context[data-line="3"]').click({modifiers:['Shift']});
    await expect(page.getByRole('toolbar')).toHaveAttribute('data-reference', /calculator.rs:2-3 \(old\)/);
    await expect(page.locator('.code-line[data-selected=true]')).toHaveCount(2);
    await expect(page.locator('.code-line.added')).toHaveAttribute('data-selected','false');
    await page.locator('.code-line.added').click();
    await page.locator('.code-line.context[data-line="3"]').click({modifiers:['Shift']});
    await expect(page.getByRole('toolbar')).toHaveAttribute('data-reference', /calculator.rs:2-3 \(new\)/);
    await expect(page.locator('.code-line[data-selected=true]')).toHaveCount(2);
    await expect(page.locator('.code-line.removed')).toHaveAttribute('data-selected','false');
  } finally {await stop(server);}
});

test('current-code excerpts render actual changes and block titles record source feedback', async ({page}) => {
  const dir=fixture();
  writeFileSync(join(dir,'calculator.rs'),'pub fn add(a: i32, b: i32) -> i32 {\n    a - b\n}\n');
  execFileSync('git',['-C',dir,'add','.']);
  execFileSync('git',['-C',dir,'-c','user.name=Test','-c','user.email=test@example.invalid','commit','-qm','base']);
  writeFileSync(join(dir,'calculator.rs'),'pub fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n');
  const server=await start(dir,0,'command');
  try {
    await page.goto(server.url);
    await page.getByRole('button',{name:'Approve once',exact:true}).click();
    await expect(page.locator('.code-line.removed')).toContainText('a - b');
    await expect(page.locator('.code-line.added')).toContainText('a + b');
    await expect(page.locator('.code-line.context')).toHaveCount(2);
    await page.locator('.code-reference').click({clickCount:3});
    await expect(page.getByRole('toolbar')).toHaveAttribute('data-reference', /calculator.rs:1-3 \(new\)/);
    await page.getByRole('button',{name:'Add feedback',exact:true}).click();
    await page.getByRole('textbox',{name:'Feedback comment'}).fill('Feedback on this block');
    await page.getByRole('button',{name:'Record feedback',exact:true}).click();
    await expect(page.locator('.feedback-card')).toContainText('calculator.rs:1-3 (new)');
    await expect(page.getByRole('alert')).toHaveCount(0);
    await page.getByRole('button',{name:'Submit review',exact:true}).click();
    await expect.poll(()=>server.child.exitCode).toBe(0);
    expect(readdirSync(dir).filter(name=>name.startsWith('.review') || name.startsWith('review.sqlite'))).toEqual([]);
  } finally {await stop(server);}
});

test('review resumes drafts and feedback after interruption, then cleans the entire folder', async ({page}) => {
  const dir=fixture(); let server=await start(dir);
  try {
    await ready(page,server.url);
    await page.locator('.code-line[data-line="2"]').click();
    await page.getByRole('button',{name:'Add feedback',exact:true}).click();
    await page.getByRole('textbox',{name:'Feedback comment'}).fill('Keep this feedback');
    await page.getByRole('button',{name:'Record feedback',exact:true}).click();
    await expect(page.locator('.feedback-card').filter({hasText:'Keep this feedback'})).toBeVisible();
    await page.locator('.code-line[data-line="3"]').click();
    await page.getByRole('button',{name:'Add feedback',exact:true}).click();
    await page.getByRole('textbox',{name:'Feedback comment'}).fill('Unfinished draft');
    await saved(page); const port=server.port; await stop(server);
    server=await start(dir,port); await ready(page,server.url);
    await expect(page.locator('.feedback-card').filter({hasText:'Keep this feedback'})).toBeVisible();
    await expect(page.getByRole('textbox',{name:'Feedback comment'})).toHaveValue('Unfinished draft');
    expect(readdirSync(dir).filter(name=>name.startsWith('review.sqlite'))).toEqual([]);
    expect(readdirSync(join(dir,'.review'))).toContain('review.sqlite');
    mkdirSync(join(dir,'.review','diagrams'));
    writeFileSync(join(dir,'.review','diagrams','temporary.svg'),'<svg/>');
    await page.getByRole('button',{name:'Submit review',exact:true}).click();
    await expect.poll(()=>server.child.exitCode).toBe(0);
    expect(readdirSync(dir).filter(name=>name.startsWith('.review'))).toEqual([]);
    const output=readFileSync(join(dir,readdirSync(dir).find(name=>name.startsWith('review-feedback-'))),'utf8');
    expect(output).toContain('Keep this feedback');
    expect(output).not.toContain('Unfinished draft');
  } finally {await stop(server);}
});

test('overlapping comments stay separate, align without overlap, and focus from their highlight', async ({page}) => {
 const dir=fixture(); const server=await start(dir);
 try {
  await ready(page,server.url);
  const row=page.locator('.code-line[data-line="2"]');
  for (const text of ['First comment','Second comment']) {
   await row.click(); await page.getByRole('button',{name:'Add feedback',exact:true}).click();
   await page.getByRole('textbox',{name:'Feedback comment'}).fill(text);
   await page.getByRole('button',{name:'Record feedback'}).click();
  }
  const cards=page.locator('.feedback-card'); await expect(cards).toHaveCount(2);
  const a=await cards.nth(0).boundingBox(), b=await cards.nth(1).boundingBox(), line=await row.boundingBox();
  expect(a.x).toBeGreaterThan(line.x+line.width); expect(Math.abs(a.y-line.y)).toBeLessThan(3);
  expect(b.y).toBeGreaterThanOrEqual(a.y+a.height);
  await row.click(); await expect(cards.nth(0)).toHaveClass(/focused/); await expect(cards.nth(0)).toBeFocused();
  await cards.nth(0).getByRole('button',{name:'Delete',exact:true}).click();
  await expect(row).toHaveAttribute('data-feedback','true');
  await expect(cards).toHaveCount(1);
  await cards.getByRole('button',{name:'Edit',exact:true}).click();
  await page.getByRole('textbox',{name:'Feedback comment'}).fill('Unfinished edit');
  await page.locator('.code-line[data-line="3"]').click(); await page.getByRole('button',{name:'Add feedback',exact:true}).click();
  await page.getByRole('textbox',{name:'Feedback comment'}).last().fill('Separate draft');
  await saved(page); await page.reload();
  await expect(page.getByRole('textbox',{name:'Feedback comment'}).nth(0)).toHaveValue('Unfinished edit');
  await expect(page.getByRole('textbox',{name:'Feedback comment'}).nth(1)).toHaveValue('Separate draft');
  await page.getByRole('button',{name:'Cancel',exact:true}).last().click();
  await page.getByRole('button',{name:'Cancel',exact:true}).click();
  await expect(cards).toContainText('Second comment');
  await cards.getByRole('button',{name:'Delete',exact:true}).click();
  await expect(row).toHaveAttribute('data-feedback','false');
 } finally {await stop(server);}
});

test('repeated prose quotes preserve their exact feedback anchors across reload', async ({page}) => {
 const dir=fixture(); writeFileSync(join(dir,'other.rs'),'// another file\n'); const server=await start(dir);
 try {
  await ready(page,server.url);
  const paragraphs=page.locator('#guide .prose p').filter({hasText:'Review how this file contributes'});
  await expect(paragraphs).toHaveCount(2);
  await paragraphs.nth(1).click({clickCount:3});
  await page.getByRole('button',{name:'Add feedback',exact:true}).click();
  await page.getByRole('textbox',{name:'Feedback comment'}).fill('About the second occurrence');
  await page.getByRole('button',{name:'Record feedback'}).click();
  const anchoredToSecond=()=>page.evaluate(()=>{
    const range=[...(CSS.highlights.get('review-changes')||[])][0];
    const paragraphs=[...document.querySelectorAll('#guide .prose p')].filter(p=>p.textContent.includes('Review how this file contributes'));
    return !!range && paragraphs[1].contains(range.startContainer);
  });
  await expect.poll(anchoredToSecond).toBe(true); await saved(page); await page.reload();
  await expect.poll(anchoredToSecond).toBe(true);
  await paragraphs.nth(1).click({position:{x:20,y:10}}); await expect(page.locator('.feedback-card')).toBeFocused();
 } finally {await stop(server);}
});

test('chat follows streaming text, pauses for reading history, and resumes on demand', async ({page}) => {
 const dir=fixture(); const server=await start(dir);
 try {
  await ready(page,server.url);
  const input=page.getByRole('textbox',{name:'Ask Codex',exact:true});
  await input.fill('fixture:stream'); await input.press('Enter');
  await expect(page.locator('.message').last()).toContainText('Streaming paragraph 10');
  const gap=()=>page.locator('#conversation').evaluate(el=>el.scrollHeight-el.clientHeight-el.scrollTop);
  await expect.poll(gap).toBeLessThan(45);
  await page.locator('#conversation').evaluate(el=>el.scrollTop=0);
  await expect(page.getByRole('button',{name:'Jump to latest',exact:true})).toBeVisible();
  await expect(page.locator('.message').last()).toContainText('Streaming paragraph 20');
  expect(await page.locator('#conversation').evaluate(el=>el.scrollTop)).toBe(0);
  await page.getByRole('button',{name:'Hide chat',exact:true}).click(); await saved(page); await page.reload();
  await page.getByRole('button',{name:'Show chat',exact:true}).click();
  await expect(page.getByRole('button',{name:'Jump to latest',exact:true})).toBeVisible();
  expect(await page.locator('#conversation').evaluate(el=>el.scrollTop)).toBe(0);
  await page.getByRole('button',{name:'Jump to latest',exact:true}).click();
  await expect.poll(gap).toBeLessThan(45);
  await expect(page.locator('.message').last()).toContainText('Streaming paragraph 39');
  await expect.poll(gap).toBeLessThan(45);
 } finally {await stop(server);}
});

test('feedback stays on the selected occurrence of a repeated code excerpt', async ({page}) => {
 const dir=fixture(); const server=await start(dir,0,'repeated-code');
 try {
  await page.goto(server.url); await page.getByRole('button',{name:'Approve once',exact:true}).click();
  const rows=page.locator('.code-line[data-line="2"]'); await expect(rows).toHaveCount(2);
  await rows.nth(1).click(); await page.getByRole('button',{name:'Add feedback',exact:true}).click();
  await page.getByRole('textbox',{name:'Feedback comment'}).fill('Comment on the second excerpt');
  await page.getByRole('button',{name:'Record feedback'}).click();
  await expect(rows.nth(0)).toHaveAttribute('data-feedback','false'); await expect(rows.nth(1)).toHaveAttribute('data-feedback','true');
  await saved(page); await page.reload();
  await expect(rows.nth(0)).toHaveAttribute('data-feedback','false'); await expect(rows.nth(1)).toHaveAttribute('data-feedback','true');
  await rows.nth(1).scrollIntoViewIfNeeded();
  const line=await rows.nth(1).boundingBox(), card=await page.locator('.feedback-card').boundingBox();
  expect(Math.abs(line.y-card.y)).toBeLessThan(3);
 } finally {await stop(server);}
});

test('chat toggle stays beside the brand and feedback fills the right margin', async ({page}) => {
 const dir=fixture(); const server=await start(dir);
 try {
  await ready(page,server.url);
  const toggle=page.getByRole('button',{name:'Hide chat',exact:true});
  const brand=await page.getByText('Aide',{exact:true}).boundingBox();
  const control=await toggle.boundingBox();
  expect(control.x+control.width).toBeLessThan(brand.x);
  await expect(toggle).toHaveAttribute('title','Hide chat');
  await page.locator('.code-line[data-line="2"]').click();
  await page.getByRole('button',{name:'Add feedback',exact:true}).click();
  const input=page.getByRole('textbox',{name:'Feedback comment'});
  await input.fill('The margin should give this comment room to breathe.');
  const assertMargin=async()=>{
   const pane=await page.locator('#reading').boundingBox();
   const card=await page.locator('.feedback-card').boundingBox();
   const editor=await input.boundingBox();
   expect(Math.abs(card.x+card.width-(pane.x+pane.width))).toBeLessThan(2);
   expect(card.width).toBeGreaterThanOrEqual(340);
   expect(editor.width).toBeGreaterThanOrEqual(card.width-40);
   expect(editor.height).toBeGreaterThanOrEqual(150);
   const style=await page.locator('.feedback-card').evaluate(el=>{
    const s=getComputedStyle(el);return {border:s.borderTopWidth,radius:s.borderTopLeftRadius,shadow:s.boxShadow};
   });
   expect(style).toEqual({border:'0px',radius:'0px',shadow:'none'});
  };
  await assertMargin();
  await toggle.click(); await assertMargin();
  await saved(page); await page.reload();
  await expect(input).toHaveValue('The margin should give this comment room to breathe.');
  await expect(page.getByRole('button',{name:'Show chat',exact:true})).toBeVisible();
  await assertMargin();
  await page.getByRole('button',{name:'Record feedback',exact:true}).click();
  await expect(page.locator('.feedback-card')).toContainText('The margin should give this comment room to breathe.');
 } finally {await stop(server);}
});

test('adding inline feedback keeps the reading position and uses a white editor', async ({page}) => {
 const dir=fixture(); writeFileSync(join(dir,'long.rs'),Array.from({length:140},(_,i)=>`// source line ${i+1}`).join('\n')+'\n');
 const server=await start(dir);
 try {
  await ready(page,server.url);
  const row=page.locator('.code-line[data-file="long.rs"][data-line="100"]');
  await row.evaluate(el=>{
   const reading=document.getElementById('reading');
   reading.scrollTop+=el.getBoundingClientRect().top-reading.getBoundingClientRect().top-reading.clientHeight+80;
  });
  await row.click(); await saved(page);
  const before=await page.locator('#reading').evaluate(el=>el.scrollTop);
  expect(before).toBeGreaterThan(1000);
  await page.getByRole('button',{name:'Add feedback',exact:true}).click();
  const input=page.getByRole('textbox',{name:'Feedback comment'});
  await expect(input).toBeFocused(); await saved(page);
  expect(await page.locator('#reading').evaluate(el=>el.scrollTop)).toBe(before);
  await expect(input).toHaveCSS('background-color','rgb(255, 255, 255)');
  await page.reload();
  await expect.poll(()=>page.locator('#reading').evaluate(el=>el.scrollTop)).toBe(before);
 } finally {await stop(server);}
});

for (const method of ['browser','CLI']) {
 test(`${method} conclusion without recorded feedback reports completion and creates no export`, async ({page}) => {
  const dir=fixture(); const server=await start(dir);
  try {
   await ready(page,server.url);
   await page.getByRole('button',{name:'Overall feedback',exact:false}).click();
   await page.getByRole('textbox',{name:'Overall feedback',exact:true}).fill(' \n\t');
   await page.locator('.code-line[data-line="2"]').click();
   await page.getByRole('button',{name:'Add feedback',exact:true}).click();
   await page.getByRole('textbox',{name:'Feedback comment'}).fill('Temporary recorded comment');
   await page.getByRole('button',{name:'Record feedback',exact:true}).click();
   await page.getByRole('button',{name:'Delete',exact:true}).click();
   await page.locator('.code-line[data-line="3"]').click();
   await page.getByRole('button',{name:'Add feedback',exact:true}).click();
   await page.getByRole('textbox',{name:'Feedback comment'}).fill('An unfinished draft is not feedback');
   await saved(page);
   if (method === 'browser') {
    await page.getByRole('button',{name:'Submit review',exact:true}).click();
    await expect(page.locator('.completion')).toContainText('Review concluded. No feedback recorded.');
    await expect(page.locator('.completion code')).toHaveCount(0);
    await expect.poll(()=>server.child.exitCode).toBe(0);
    expect(server.output()).toContain('Review concluded. No feedback recorded.');
    expect(await page.evaluate(()=>Object.keys(localStorage).filter(k=>k.startsWith('review-outbox-')))).toEqual([]);
   } else {
    await stop(server);
    const output=execFileSync(binary,['--path',dir,'conclude'],{encoding:'utf8'}).trim();
    expect(output).toBe('Review concluded. No feedback recorded.');
   }
   expect(readdirSync(dir).filter(n=>n.startsWith('.review') || n.startsWith('review-feedback-'))).toEqual([]);
  } finally {await stop(server);}
 });
}
