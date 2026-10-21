import { test, expect } from '@playwright/test';
import { mkdtempSync, writeFileSync, readFileSync, readdirSync, mkdirSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { spawn, execFileSync } from 'node:child_process';

const binary = resolve('target/debug/review');
function fixture() {
  const dir = mkdtempSync(join(tmpdir(), 'devdeck-e2e-'));
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
  await expect(page.locator('.reference')).toContainText(quote);
  return quote;
}

test('selection, chat, recorded feedback, drafts, server restart and direct export', async ({page}) => {
  const dir = fixture(); let server = await start(dir);
  try {
    await ready(page, server.url);
    await page.locator('.code-line[data-line="2"]').click();
    await expect(page.locator('.reference')).toContainText('calculator.rs:2-2 (new)');
    await page.getByRole('textbox', {name:'Ask Codex', exact:true}).fill('Why addition?');
    await page.getByRole('button', {name:'Send', exact:true}).click();
    await expect(page.locator('.message').last()).toContainText('Selected review reference:');
    await page.getByRole('button', {name:'Add feedback', exact:true}).click();
    await page.getByRole('textbox', {name:'Feedback comment'}).fill('Cover negative inputs.');
    await page.getByRole('button', {name:'Record feedback'}).click();
    await expect(page.locator('.feedback-card')).toContainText('Cover negative inputs.');
    const quote = await selectProse(page);
    await page.getByRole('textbox', {name:'Feedback comment'}).fill('Explain this motivation.');
    await page.getByRole('button', {name:'Record feedback'}).click();
    await expect(page.locator('.feedback-card')).toHaveCount(2);
    await page.getByRole('button', {name:'Clear reference'}).click();
    await page.getByRole('textbox', {name:'Feedback comment'}).fill('Overall: looks promising.');
    await page.getByRole('button', {name:'Record feedback'}).click();
    await expect(page.locator('.feedback-card')).toHaveCount(3);
    await page.getByRole('textbox', {name:'Feedback comment'}).fill('Unfinished feedback');
    await page.getByRole('button', {name:'Conversation', exact:true}).click();
    await page.getByRole('textbox', {name:'Ask Codex', exact:true}).fill('Unfinished question');
    await saved(page);
    const port = server.port; await stop(server); server = await start(dir, port);
    await page.reload(); await expect(page.getByRole('textbox', {name:'Ask Codex', exact:true})).toHaveValue('Unfinished question');
    await expect(page.locator('.reference')).toContainText('WHOLE REVIEW');
    await page.getByRole('button', {name:'Feedback (3)', exact:true}).click();
    await expect(page.getByRole('textbox', {name:'Feedback comment'})).toHaveValue('Unfinished feedback');
    await expect(page.locator('.feedback-card')).toHaveCount(3);
    await page.getByRole('button', {name:'Submit review'}).click();
    await expect(page.getByText('Review submitted. Feedback saved to', {exact:false})).toBeVisible();
    const filename = readdirSync(dir).find(f => f.startsWith('review-feedback-'));
    const output = readFileSync(join(dir, filename), 'utf8');
    expect(output).toContain('calculator.rs:2-2 (new)');expect(output).toContain(`> ${quote}`);
    expect(output).toContain('Overall: looks promising.');expect(output).not.toContain('Why addition?');expect(output).not.toContain('Unfinished feedback');
    await expect.poll(() => server.child.exitCode).toBe(0);
    await expect(page.locator('.connection')).toHaveText('Submitted');
    await expect(page.getByRole('button', {name:'Submit review'})).toBeDisabled();
    expect(readdirSync(dir).filter(name => name.startsWith('review.sqlite') || name.startsWith('.review'))).toEqual([]);
    expect(output.indexOf('Overall: looks promising.')).toBeLessThan(output.indexOf('calculator.rs:2-2'));
    expect(await page.evaluate(() => Object.keys(localStorage).filter(key => key.startsWith('review-outbox-')))).toEqual([]);
    server = await start(dir, port);
    await ready(page,server.url);
    await expect(page.getByRole('button', {name:'Submit review'})).toBeEnabled();
    await expect(page.getByRole('button', {name:'Feedback (0)',exact:true})).toBeVisible();
    await expect(page.locator('.message')).toHaveCount(0);
    expect(readFileSync(join(dir, filename),'utf8')).toBe(output);
  } finally { await stop(server); }
});

test('manual references, deselection, feedback editing, and changed-code archive', async ({page}) => {
 const dir=fixture();let server=await start(dir);
 try{
  await ready(page,server.url);await page.getByText('Reference a file range',{exact:true}).click();
  await page.getByLabel('Source file', {exact:true}).fill('calculator.rs');await page.getByRole('spinbutton',{name:'Start line'}).fill('1');await page.getByRole('spinbutton',{name:'End line'}).fill('3');await page.getByRole('button',{name:'Use',exact:true}).click();
  await expect(page.locator('.reference')).toContainText('calculator.rs:1-3');
  await page.getByRole('button',{name:'Add feedback',exact:true}).click();await page.getByRole('textbox',{name:'Feedback comment'}).fill('Initial feedback');await page.getByRole('button',{name:'Record feedback'}).click();
  await page.getByRole('button',{name:'Edit',exact:true}).click();await page.getByRole('textbox',{name:'Feedback comment'}).fill('Revised feedback');await page.getByRole('button',{name:'Record feedback'}).click();await expect(page.locator('.feedback-card')).toContainText('Revised feedback');await expect(page.locator('.feedback-card')).toHaveCount(1);
  await page.getByRole('button',{name:'Clear reference'}).click();await expect(page.locator('.reference')).toContainText('Select guide text');
  await saved(page);writeFileSync(join(dir,'calculator.rs'),'pub fn add(a: i32, b: i32) -> i32 {\n    a + b + 1\n}\n');
  // No polling and no command-triggered capture: feedback still belongs to the original snapshot.
  await page.getByRole('textbox',{name:'Feedback comment'}).fill('Keep the original snapshot');
  await page.getByRole('button',{name:'Record feedback'}).click();
  await expect(page.locator('.feedback-card')).toHaveCount(2);
  await expect(page.locator('#guide')).not.toContainText('a + b + 1');
  const port=server.port; await stop(server); server=await start(dir,port); await ready(page,server.url);
  await expect(page.getByText('Previous review is archived',{exact:false})).toBeVisible();
  await expect(page.getByRole('button',{name:'Feedback (0)',exact:true})).toBeVisible();
  await expect(page.locator('#guide')).toContainText('a + b + 1');
 }finally{await stop(server);}
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
    ws.onerror=reject;ws.onmessage=e=>{const message=JSON.parse(e.data);if(message.type==='snapshot'&&!sent){sent=true;ws.send(JSON.stringify({id:'retry-test',review_id:message.review.id,command:{type:'feedback',id:'comment-test',text:'Only once',reference:null}}));}if(message.type==='ack'){ws.close();resolve();}};
   });await request();await request();
  });
  await page.getByRole('button',{name:'Feedback (1)',exact:true}).click();await expect(page.locator('.feedback-card')).toHaveCount(1);
 }finally{await stop(server);}
});

test('reading position and source selection survive a fresh browser page', async ({page}) => {
 const dir=fixture();writeFileSync(join(dir,'long.rs'),Array.from({length:120},(_,i)=>`// source line ${i+1}`).join('\n')+'\n');const server=await start(dir);
 try {
  await ready(page,server.url);await page.locator('.code-line[data-file="long.rs"][data-line="80"]').click();await saved(page);
  const scroll=await page.locator('#reading').evaluate(el=>el.scrollTop);expect(scroll).toBeGreaterThan(500);
  await page.reload();await expect(page.locator('.reference')).toContainText('long.rs:80-80');
  await expect.poll(()=>page.locator('#reading').evaluate(el=>el.scrollTop)).toBeCloseTo(scroll,0);
  await expect(page.locator('.code-line[data-file="long.rs"][data-line="80"]')).toHaveAttribute('data-selected','true');
 }finally{await stop(server);}
});


test('left panel, persistent prose highlight, implicit general feedback and one-off lifecycle', async ({page}) => {
  const dir=fixture(); const server=await start(dir);
  try {
    await ready(page,server.url);
    await expect(page.getByRole('button',{name:'New review',exact:true})).toHaveCount(0);
    await expect(page.getByRole('button',{name:'Prepare / regenerate guide'})).toHaveCount(0);
    await expect(page.locator('.message')).toHaveCount(0);
    const panel=await page.locator('aside').boundingBox();
    const guide=await page.locator('main').boundingBox();
    expect(panel.x + panel.width).toBeLessThanOrEqual(guide.x);
    const title=await page.locator('#guide h1').first().boundingBox();
    const metadata=await page.locator('.document-heading').boundingBox();
    const firstParagraph=await page.locator('#guide .prose p').first().boundingBox();
    expect(title.y + title.height).toBeLessThanOrEqual(metadata.y);
    expect(metadata.y + metadata.height).toBeLessThanOrEqual(firstParagraph.y);

    const quote=await selectProse(page);
    await page.getByRole('textbox',{name:'Ask Codex',exact:true}).fill('Question about this passage');
    const highlighted=()=>page.evaluate(()=>{
      const highlight=CSS.highlights.get('review-reference');
      return highlight ? [...highlight].map(range=>range.toString()).join('') : '';
    });
    await expect.poll(highlighted).toBe(quote);
    const ref=await page.locator('.reference').boundingBox();
    const input=await page.getByRole('textbox',{name:'Ask Codex',exact:true}).boundingBox();
    expect(input.y - (ref.y+ref.height)).toBeLessThan(80);
    await saved(page); await page.reload();
    await expect.poll(highlighted).toBe(quote);
    await page.getByRole('button',{name:'Clear reference'}).click();
    await expect.poll(highlighted).toBe('');
    await page.getByRole('button',{name:'Feedback (0)',exact:true}).click();
    for (const text of ['General comment one','General comment two']) {
      await page.getByRole('textbox',{name:'Feedback comment'}).fill(text);
      await page.getByRole('button',{name:'Record feedback'}).click();
    }
    await expect(page.locator('.feedback-card')).toHaveCount(2);
    await expect(page.locator('.feedback-card small')).toHaveText(['Overall feedback','Overall feedback']);
    await page.getByRole('button',{name:'Submit review'}).click();
    await expect(page.locator('.completion')).toBeVisible();
    await expect.poll(()=>server.child.exitCode).toBe(0);
    const exported=readFileSync(join(dir,readdirSync(dir).find(name=>name.startsWith('review-feedback-'))),'utf8');
    expect(exported).toBe('# Overall feedback\n\nGeneral comment one\n\nGeneral comment two\n\n');
  } finally { await stop(server); }
});

test('CLI conclusion exports the saved review even after workspace edits', async ({page}) => {
  const dir=fixture(); const server=await start(dir);
  try {
    await ready(page,server.url);
    await page.getByRole('button',{name:'Feedback (0)',exact:true}).click();
    await page.getByRole('textbox',{name:'Feedback comment'}).fill('Preserve this reviewed snapshot');
    await page.getByRole('button',{name:'Record feedback'}).click();
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
    await page.getByRole('button',{name:'Submit review'}).click();
    await expect(page.getByRole('alert')).toContainText('already exists with different content');
    await expect(page.locator('.completion')).toHaveCount(0);
    expect(server.child.exitCode).toBeNull();
    expect(readdirSync(join(dir,'.review'))).toContain('review.sqlite');
    expect(readdirSync(join(dir,'.review'))).toContain('lock');
    expect(readFileSync(path,'utf8')).toBe('An existing handover');
    await page.getByRole('button',{name:'Feedback (0)',exact:true}).click();
    await page.getByRole('textbox',{name:'Feedback comment'}).fill('Still editable after failed export');
    await page.getByRole('button',{name:'Record feedback'}).click();
    await expect(page.locator('.feedback-card')).toContainText('Still editable after failed export');
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
      // Pending preparation requests survive reload and remain available on either tab.
      await page.getByRole('button',{name:'Feedback (0)',exact:true}).click();
      await saved(page); await page.reload();
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
    await expect(page.locator('.reference')).toContainText('calculator.rs:1-3');
    await expect(page.locator('.code-line[data-selected=true]')).toHaveCount(3);
    await saved(page); await page.reload();
    await expect(page.locator('.code-line[data-selected=true]')).toHaveCount(3);
    await last.click();
    await first.click({modifiers:['Shift']});
    await expect(page.locator('.reference')).toContainText('calculator.rs:1-3');
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
    await expect(page.locator('.reference')).toContainText('calculator.rs:2-3 (old)');
    await expect(page.locator('.code-line[data-selected=true]')).toHaveCount(2);
    await expect(page.locator('.code-line.added')).toHaveAttribute('data-selected','false');
    await page.locator('.code-line.added').click();
    await page.locator('.code-line.context[data-line="3"]').click({modifiers:['Shift']});
    await expect(page.locator('.reference')).toContainText('calculator.rs:2-3 (new)');
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
    await expect(page.locator('.reference')).toContainText('calculator.rs:1-3 (new)');
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
    await page.getByRole('button',{name:'Feedback (0)',exact:true}).click();
    await page.getByRole('textbox',{name:'Feedback comment'}).fill('Keep this feedback');
    await page.getByRole('button',{name:'Record feedback',exact:true}).click();
    await expect(page.locator('.feedback-card')).toContainText('Keep this feedback');
    await page.getByRole('textbox',{name:'Feedback comment'}).fill('Unfinished draft');
    await saved(page); const port=server.port; await stop(server);
    server=await start(dir,port); await ready(page,server.url);
    await expect(page.locator('.feedback-card')).toContainText('Keep this feedback');
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
