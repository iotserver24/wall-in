import { chromium } from 'playwright'
import assert from 'node:assert/strict'

const base = 'http://127.0.0.1:3977'
const browser = await chromium.launch({executablePath: process.env.CHROME_BIN || '/usr/bin/google-chrome', headless: true, args: ['--disable-gpu']})
const page = await browser.newPage({viewport: {width: 1440, height: 900}})
const state = async () => (await page.request.get(`${base}/api/state`)).json()
const original = (await state()).playback.wallpaper_id
if (!original) throw new Error('Refusing live test without a wallpaper to restore')
try {
  await page.goto(base)
  await page.getByRole('button', {name: 'Preview RotatingCube', exact: true}).click()
  const detail = page.getByRole('complementary', {name: 'Wallpaper details'})
  await detail.getByText('Wall-in native 3D scene').waitFor()
  await detail.getByText(/Native 3D scenes render on the desktop/).waitFor()
  const responsePromise = page.waitForResponse(r => new URL(r.url()).pathname === '/api/playback' && r.request().method() === 'POST')
  await detail.getByRole('button', {name: 'Apply wallpaper'}).click()
  const response = await responsePromise
  assert.equal(response.status(), 200, await response.text())
  const scene = (await state()).wallpapers.find(w => w.title === 'RotatingCube')
  assert.equal((await state()).playback.wallpaper_id, scene.id)
  assert.equal((await state()).playback.status, 'playing')
  await page.getByRole('navigation', {name: 'Main navigation'}).getByRole('button', {name: 'Playlists'}).click()
  assert.equal(await page.getByRole('checkbox', {name: /RotatingCube/}).isEnabled(), true)
  await page.getByRole('navigation', {name: 'Main navigation'}).getByRole('button', {name: 'Compatibility'}).click()
  await page.getByRole('heading', {name: 'Wall-in native 3D scenes'}).waitFor()
  await page.getByRole('heading', {name: 'Wallpaper Engine scene projects'}).waitFor()
  await page.setViewportSize({width: 390, height: 844})
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false)
  console.log('SCENE_APPLIED_AND_BROWSER_ROUTES_VERIFIED')
  await new Promise(resolve => setTimeout(resolve, 20000))
} finally {
  const response = await page.request.post(`${base}/api/playback`, {data: {action: 'apply', wallpaper_id: original}})
  assert.equal(response.status(), 200, await response.text())
  assert.equal((await state()).playback.wallpaper_id, original)
  console.log('ORIGINAL_VIDEO_RESTORED')
  await browser.close()
}
