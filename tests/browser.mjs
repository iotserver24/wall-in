import { chromium } from 'playwright';
import assert from 'node:assert/strict';
import { mkdir, rename, access } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const fixtures = path.join(root, 'tests/generated');
const base = process.env.WALL_IN_TEST_URL || 'http://127.0.0.1:3977';
const browser = await chromium.launch({executablePath: process.env.CHROME_BIN || '/usr/bin/google-chrome', headless: true, args: ['--disable-gpu']});
const page = await browser.newPage({viewport: {width: 1440, height: 1000}});
const exceptions = [];
page.on('pageerror', error => exceptions.push(error.message));
await mkdir(path.join(root, 'artifacts'), {recursive: true});
const nav = name => page.getByRole('navigation', {name: 'Main navigation'}).getByRole('button', {name: name === 'Library' ? /^Library/ : name, exact: name !== 'Library'});
const state = async () => (await page.request.get(base + '/api/state')).json();
const check = (name) => console.log('PASS ' + name);
async function mutation(action, endpoint, method='POST', expected=200) {
  const responsePromise = page.waitForResponse(response => new URL(response.url()).pathname === endpoint && response.request().method() === method);
  await action();
  const response = await responsePromise;
  assert.equal(response.status(), expected, await response.text());
  await page.waitForFunction(() => !document.querySelector('[aria-label="Refresh library"]')?.disabled);
  return response.json();
}
async function importFile(relative) {
  await page.getByRole('button', {name:'Import wallpaper',exact:true}).click();
  const dialog = page.getByRole('dialog');
  await dialog.getByLabel('Absolute file or project-folder path').fill(path.join(fixtures, relative));
  const next = await mutation(() => dialog.getByRole('button', {name:'Import wallpaper',exact:true}).click(), '/api/import');
  await dialog.waitFor({state:'hidden'});
  return next;
}
try {
  await page.goto(base);
  await page.getByRole('heading', {name:'Your library',exact:true}).waitFor();
  const initial = await state();
  for (const wallpaper of initial.wallpapers) {
    assert.ok(wallpaper.path.startsWith(fixtures + '/'), 'Use an isolated WALL_IN_DATA_DIR for browser tests.');
    assert.equal((await page.request.delete(base + '/api/wallpapers/' + wallpaper.id)).status(), 200);
  }
  assert.equal((await page.request.patch(base + '/api/settings', {data:{volume:50,muted:true,fit:'cover',playlist_ids:[]}})).status(),200);
  await page.reload();
  await page.getByRole('heading', {name:'A fresh start for your desktop.'}).waitFor();
  await page.screenshot({path:path.join(root,'artifacts/library-empty.png'),fullPage:true});
  check('connected service and genuine empty library');

  await page.getByRole('button',{name:'Import wallpaper',exact:true}).click();
  const dialog = page.getByRole('dialog');
  await dialog.getByLabel('Absolute file or project-folder path').fill('relative.png');
  await dialog.getByRole('button',{name:'Import wallpaper',exact:true}).click();
  await dialog.getByText(/Enter an absolute Linux path/).waitFor();
  await dialog.getByLabel('Absolute file or project-folder path').fill('/wall-in-test-no-such-file.png');
  await mutation(() => dialog.getByRole('button',{name:'Import wallpaper',exact:true}).click(),'/api/import','POST',400);
  await dialog.getByText(/Cannot access import path/).waitFor();
  await page.keyboard.press('Escape');
  await dialog.waitFor({state:'hidden'});
  check('relative-path validation, missing-file error, and keyboard dialog dismissal');

  let next = await importFile('Alpine dusk.png');
  const image = next.wallpapers.find(w => w.title === 'Alpine dusk');
  assert.ok(image?.supported);
  await page.getByRole('img',{name:'Alpine dusk',exact:true}).waitFor();
  await page.waitForFunction(()=>{const el=document.querySelector('img[alt="Alpine dusk"]');return el?.complete && el.naturalWidth>0;});
  if (!next.platform.available) assert.equal(await page.getByRole('button',{name:'Apply wallpaper',exact:true}).isEnabled(),false);
  await importFile('Alpine dusk.png');
  assert.equal((await state()).wallpapers.length,1);
  check('image import, decoded preview, deduplication, and honest unavailable Apply control');

  await mutation(()=>page.getByRole('button',{name:'Favorite Alpine dusk',exact:true}).click(),'/api/wallpapers/'+image.id,'PATCH');
  await nav('Favorites').click();
  await page.getByRole('button',{name:'Preview Alpine dusk',exact:true}).waitFor();
  await page.getByRole('textbox',{name:'Search wallpapers'}).fill('no-matching-wallpaper');
  await page.getByRole('heading',{name:'No wallpapers match.'}).waitFor();
  await page.getByRole('button',{name:'Clear search',exact:true}).click();
  await nav('Library').click();
  await page.getByRole('button',{name:'Unfavorite Alpine dusk',exact:true}).waitFor();
  check('favorites persist across pages and search handles empty results');

  await importFile('Motion test.mp4');
  const video = page.getByLabel('Video preview of Motion test', {exact:true});
  await video.waitFor();
  await page.waitForFunction(()=>{const el=document.querySelector('video');return el && el.readyState>=2;});
  const box = await video.boundingBox();
  await video.click({position:{x:24,y:box.height-48}});
  await page.waitForFunction(()=>{const el=document.querySelector('video');return el && !el.paused && el.currentTime>0;});
  await video.click({position:{x:24,y:box.height-48}});
  await page.waitForFunction(()=>document.querySelector('video')?.paused);
  check('real video decoding, user-operated preview play and pause');

  await importFile('valid-project');
  await page.getByRole('complementary',{name:'Wallpaper details'}).getByRole('heading',{name:'Workshop sample',exact:true}).waitFor();
  await importFile('unsupported-scene');
  await page.getByText('Scene rendering is not available for this project.').waitFor();
  assert.equal(await page.getByRole('button',{name:'Apply wallpaper',exact:true}).isEnabled(),false);
  await page.getByRole('combobox',{name:'Wallpaper type',exact:true}).selectOption('video');
  assert.equal(await page.getByRole('button',{name:'Preview Alpine dusk',exact:true}).count(),0);
  assert.equal(await page.getByRole('button',{name:'Preview Motion test',exact:true}).count(),1);
  await page.getByRole('combobox',{name:'Sort wallpapers',exact:true}).selectOption('za');
  await page.getByRole('combobox',{name:'Wallpaper type',exact:true}).selectOption('all');
  check('Workshop import, type filtering, sorting and unsupported-scene handling');

  await page.getByRole('button',{name:'Import wallpaper',exact:true}).click();
  await page.getByRole('dialog').getByLabel('Absolute file or project-folder path').fill(path.join(fixtures,'workshop-video'));
  await mutation(()=>page.getByRole('dialog').getByRole('button',{name:'Import wallpaper',exact:true}).click(),'/api/import','POST',400);
  await page.getByRole('dialog').getByText(/cannot traverse parents/).waitFor();
  await page.keyboard.press('Escape');
  await mutation(()=>page.getByRole('button',{name:'Scan Steam',exact:true}).click(),'/api/scan-steam');
  await page.getByRole('button',{name:'Preview Steam scan sample',exact:true}).waitFor();
  check('unsafe project paths rejected and secondary Steam library discovered');

  await nav('Displays').click();
  await page.getByRole('heading',{name:'Connected displays',exact:true}).waitFor();
  await mutation(()=>page.getByRole('combobox',{name:'Wallpaper fit',exact:true}).selectOption('contain'),'/api/settings','PATCH');
  await nav('Settings').click();
  assert.equal(await page.getByRole('combobox',{name:'Wallpaper fit',exact:true}).inputValue(),'contain');
  const mute = page.getByRole('button',{name:/^(Mute|Unmute) wallpaper$/}).first();
  const wasMuted = (await state()).settings.muted;
  await mutation(()=>mute.click(),'/api/settings','PATCH');
  assert.equal((await state()).settings.muted,!wasMuted);
  const volume = page.getByRole('slider',{name:'Wallpaper volume',exact:true}).first();
  await volume.focus();
  await mutation(()=>volume.press('Home'),'/api/settings','PATCH');
  assert.equal((await state()).settings.volume,0);
  assert.equal(await page.getByRole('switch',{name:'Pause on battery'}).isEnabled(),true);
  check('display/fit, mute and keyboard volume changes remain consistent across pages');

  await nav('Playlists').click();
  const queued = page.getByRole('checkbox',{name:/Alpine dusk/});
  await mutation(()=>queued.click(),'/api/settings','PATCH');
  assert.equal(await queued.isChecked(),true);
  assert.ok((await state()).settings.playlist_ids.includes(image.id));
  assert.equal(await page.getByRole('checkbox',{name:/Scene compatibility test/}).isEnabled(),false);
  await nav('Compatibility').click();
  await page.getByRole('heading',{name:'Your desktop environment'}).waitFor();
  await page.getByRole('heading',{name:'Wallpaper Engine scene projects'}).waitFor();
  await page.getByRole('heading',{name:'Wall-in native 3D scenes'}).waitFor();
  await page.reload();
  await page.getByRole('heading',{name:'Your library',exact:true}).waitFor();
  assert.equal((await state()).settings.fit,'contain');
  await nav('Playlists').click();
  assert.equal(await page.getByRole('checkbox',{name:/Alpine dusk/}).isChecked(),true);
  check('playlist membership, compatibility reporting and persisted state after reload');

  await nav('Library').click();
  await page.getByRole('button',{name:'Preview Alpine dusk',exact:true}).click();
  await page.screenshot({path:path.join(root,'artifacts/library-desktop.png'),fullPage:true});
  await page.setViewportSize({width:390,height:844});
  for (const route of ['Library','Favorites','Playlists','Displays','Settings','Compatibility']) {
    await nav(route).click();
    await page.getByRole('heading',{name:route==='Library'?'Your library':route,exact:true}).waitFor();
    const overflow = await page.evaluate(()=>document.documentElement.scrollWidth>window.innerWidth);
    assert.equal(overflow,false,'Horizontal page overflow on '+route);
    await page.screenshot({path:path.join(root,'artifacts/mobile-'+route.toLowerCase()+'.png'),fullPage:true});
  }
  await nav('Library').click();
  await page.getByRole('button',{name:'Import wallpaper',exact:true}).click();
  await page.getByRole('dialog').getByLabel('Absolute file or project-folder path').fill(path.join(fixtures,'Alpine dusk.png'));
  await mutation(()=>page.getByRole('dialog').getByRole('button',{name:'Import wallpaper',exact:true}).click(),'/api/import');
  await page.getByRole('dialog').waitFor({state:'hidden'});
  check('all six pages at mobile width, mobile import, no horizontal overflow');

  await page.setViewportSize({width:1440,height:1000});
  const imagePath = path.join(fixtures,'Alpine dusk.png');
  await rename(imagePath,imagePath+'.missing');
  try {
    await page.reload();
    await page.getByRole('button',{name:'Preview Alpine dusk',exact:true}).click();
    await page.getByRole('complementary',{name:'Wallpaper details'}).getByText('Preview unavailable',{exact:true}).waitFor();
    assert.equal((await page.request.get(base + image.media_url)).status(),404);
  } finally {
    await rename(imagePath+'.missing',imagePath);
  }
  await page.reload();
  await page.getByRole('button',{name:'Preview Alpine dusk',exact:true}).click();
  await page.waitForFunction(()=>{const el=document.querySelector('img[alt="Alpine dusk"]');return el?.complete && el.naturalWidth>0;});
  check('missing source after import reports a preview error and recovers after restoration');
  await page.getByRole('button',{name:'Remove from library',exact:true}).click();
  await page.getByRole('dialog').getByRole('button',{name:'Keep wallpaper',exact:true}).click();
  assert.ok((await state()).wallpapers.some(w=>w.id===image.id));
  await page.getByRole('button',{name:'Remove from library',exact:true}).click();
  await mutation(()=>page.getByRole('dialog').getByRole('button',{name:'Remove wallpaper',exact:true}).click(),'/api/wallpapers/'+image.id,'DELETE');
  await access(path.join(fixtures,'Alpine dusk.png'));
  await nav('Favorites').click();
  await page.getByRole('heading',{name:'Keep your favorites close.'}).waitFor();
  await nav('Playlists').click();
  assert.equal(await page.getByRole('checkbox',{name:/Alpine dusk/}).count(),0);
  assert.equal((await state()).settings.playlist_ids.includes(image.id),false);
  check('cancel/removal flow keeps source files and removes stale favorites/queue entries');

  await page.route('**/api/state*', route=>route.abort());
  await page.getByRole('button',{name:'Refresh library'}).click();
  await page.getByText('Service disconnected',{exact:true}).waitFor();
  await page.unroute('**/api/state*');
  await page.getByRole('button',{name:'Refresh library'}).click();
  await page.getByText('Local service connected',{exact:true}).waitFor();
  check('service disconnect and recovery preserve the library');
  assert.deepEqual(exceptions,[]);
  console.log('All browser interaction checks passed. Screenshots saved in artifacts/.');
} catch (error) {
  await page.screenshot({path:path.join(root,'artifacts/browser-failure.png'),fullPage:true});
  console.error(await page.locator('body').innerText());
  throw error;
} finally {
  await browser.close();
}
