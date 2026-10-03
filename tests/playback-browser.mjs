import { chromium } from 'playwright';
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { mkdtemp, mkdir, symlink, copyFile, readFile, access } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..');
const temporary=await mkdtemp(path.join(root,'tests/generated/protocol-'));
await mkdir(path.join(temporary,'integrations'));
await mkdir(path.join(temporary,'apps/desktop'),{recursive:true});
await copyFile(path.join(root,'tests/protocol_renderer.py'),path.join(temporary,'integrations/renderer.py'));
await symlink(path.join(root,'apps/desktop/dist'),path.join(temporary,'apps/desktop/dist'));
const log=path.join(temporary,'controls.jsonl');
const environment={...process.env,WALL_IN_ROOT:temporary,WALL_IN_DATA_DIR:path.join(temporary,'data'),WALL_IN_PROTOCOL_LOG:log};
let service;
let browser;
let base;
async function start() {
  service=spawn(path.join(root,'target/debug/wall-in'),['serve','--port','0'],{env:environment,stdio:['ignore','ignore','pipe']});
  base=await new Promise((resolve,reject)=>{
    let output='';
    const timer=setTimeout(()=>reject(new Error('Test service startup timed out: '+output)),30000);
    service.stderr.on('data',chunk=>{
      output+=chunk;
      const match=output.match(/Wall-in listening at (http:\/\/[^\s]+)/);
      if(match){clearTimeout(timer);resolve(match[1]);}
    });
    service.once('error',error=>{clearTimeout(timer);reject(error);});
    service.once('exit',code=>{clearTimeout(timer);reject(new Error('Service exited: '+code+' '+output));});
  });
}
async function stop() {
  if(service && service.exitCode===null){const exited=once(service,'exit');service.kill('SIGTERM');await exited;}
}
try {
  await start();
  browser=await chromium.launch({executablePath:process.env.CHROME_BIN||'/usr/bin/google-chrome',headless:true,args:['--disable-gpu']});
  const page=await browser.newPage({viewport:{width:1440,height:1000}});
  const state=async()=>(await page.request.get(base+'/api/state')).json();
  const nav=name=>page.getByRole('navigation',{name:'Main navigation'}).getByRole('button',{name:name==='Library'?/^Library/:name,exact:name!=='Library'});
  async function change(action,endpoint,method='POST'){
    const pending=page.waitForResponse(response=>new URL(response.url()).pathname===endpoint&&response.request().method()===method);
    await action();
    const response=await pending;
    assert.equal(response.status(),200,await response.text());
    await page.waitForFunction(()=>!document.querySelector('[aria-label="Refresh library"]')?.disabled);
    return response.json();
  }
  await page.goto(base);
  await page.getByRole('button',{name:'Import wallpaper',exact:true}).click();
  const media=path.join(root,'tests/generated/Motion test.mp4');
  await page.getByRole('dialog').getByLabel('Absolute file or project-folder path').fill(media);
  let next=await change(()=>page.getByRole('dialog').getByRole('button',{name:'Import wallpaper',exact:true}).click(),'/api/import');
  await page.getByRole('dialog').waitFor({state:'hidden'});
  const id=next.wallpapers[0].id;
  next=await change(()=>page.getByRole('button',{name:'Apply wallpaper',exact:true}).click(),'/api/playback');
  assert.equal(next.playback.status,'playing');
  await page.getByRole('contentinfo').getByText('Playing on your desktop',{exact:true}).waitFor();
  next=await change(()=>page.getByRole('button',{name:'Pause wallpaper',exact:true}).click(),'/api/playback');
  assert.equal(next.playback.status,'paused');
  await nav('Settings').click();
  next=await change(()=>page.getByRole('combobox',{name:'Wallpaper fit',exact:true}).selectOption('fill'),'/api/settings','PATCH');
  assert.equal(next.playback.status,'paused');
  assert.equal(next.settings.fit,'fill');
  await stop();
  await start();
  await page.goto(base);
  await page.getByRole('contentinfo').getByText('Desktop playback paused',{exact:true}).waitFor();
  assert.equal((await state()).playback.wallpaper_id,id);
  for(const name of ['Favorites','Playlists','Displays','Settings','Compatibility','Library']){
    await nav(name).click();
    assert.equal(await page.getByRole('contentinfo').getByText('Desktop playback paused',{exact:true}).isVisible(),true);
  }
  await change(()=>page.getByRole('button',{name:'Resume wallpaper',exact:true}).click(),'/api/playback');
  await change(()=>page.getByRole('button',{name:'Stop wallpaper',exact:true}).click(),'/api/playback');
  await stop();
  await start();
  await page.goto(base);
  await page.getByRole('contentinfo').getByText('Nothing playing',{exact:true}).waitFor();
  assert.equal((await state()).playback.status,'stopped');
  const controls=()=>readFile(log,'utf8').then(text=>text.trim().split('\n').map(JSON.parse));
  assert.equal((await controls()).filter(c=>c.action==='started').length,2);
  await page.getByRole('button',{name:'Preview Motion test',exact:true}).click();
  await change(()=>page.getByRole('button',{name:'Apply wallpaper',exact:true}).click(),'/api/playback');
  const ownedPid=(await controls()).filter(c=>c.action==='started').at(-1).pid;
  process.kill(ownedPid,'SIGKILL');
  await page.getByRole('button',{name:'Refresh library',exact:true}).click();
  await page.getByText(/Renderer exited unexpectedly/).waitFor();
  assert.equal((await state()).playback.status,'stopped');
  await change(()=>page.getByRole('button',{name:'Apply wallpaper',exact:true}).click(),'/api/playback');
  await page.getByRole('button',{name:'Remove from library',exact:true}).click();
  await change(()=>page.getByRole('dialog').getByRole('button',{name:'Remove wallpaper',exact:true}).click(),'/api/wallpapers/'+id,'DELETE');
  await page.getByRole('contentinfo').getByText('Nothing playing',{exact:true}).waitFor();
  await access(media);
  assert.equal((await state()).wallpapers.length,0);
  const saved=JSON.parse(await readFile(path.join(temporary,'data/library.json'),'utf8'));
  assert.equal(saved.desired_wallpaper_id,null);
  console.log('PASS browser Apply/Pause/Configure/Resume/Stop, restart restoration, cross-page consistency, renderer crash recovery, and active deletion with a test-only renderer protocol fixture.');
  console.log('This verifies service/UI coordination, NOT actual desktop rendering.');
} finally {
  if(browser)await browser.close();
  await stop();
}
