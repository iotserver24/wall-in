import { chromium } from 'playwright'
import assert from 'node:assert/strict'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { mkdir } from 'node:fs/promises'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const base = process.env.WALL_IN_TEST_URL || 'http://127.0.0.1:3978'
const skinRoot = path.join(root, 'tests/fixtures/widget-skin')
const browser = await chromium.launch({ executablePath: process.env.CHROME_BIN || '/usr/bin/google-chrome', headless: true, args: ['--disable-gpu'] })
const page = await browser.newPage({ viewport: { width: 1440, height: 1000 } })
const errors = []
page.on('pageerror', error => errors.push(error.message))
const nav = name => page.getByRole('navigation', { name: 'Main navigation' }).getByRole('button', { name: name === 'Library' ? /^Library/ : name, exact: name !== 'Library' })
const state = async () => (await page.request.get(base + '/api/state')).json()
async function mutate(action, endpoint, method, status = 200) {
  const responsePromise = page.waitForResponse(response => new URL(response.url()).pathname === endpoint && response.request().method() === method)
  await action()
  const response = await responsePromise
  assert.equal(response.status(), status, await response.text())
  return response.json()
}
try {
  const initial = await state()
  for (const widget of initial.widgets) {
    assert.equal(widget.root, skinRoot, 'Use an isolated WALL_IN_DATA_DIR for widget browser tests.')
    assert.equal((await page.request.delete(`${base}/api/widgets/${widget.id}`)).status(), 200)
  }
  await page.goto(base)
  await nav('Widgets').click()
  await page.getByRole('heading', { name: 'No skins imported' }).waitFor()
  await page.getByRole('button', { name: 'Import skin', exact: true }).first().click()
  const dialog = page.getByRole('dialog')
  await dialog.getByLabel('Absolute skin-folder path').fill('relative')
  await dialog.getByLabel('INI file relative to skin folder').fill('Clock.ini')
  await dialog.getByRole('button', { name: 'Import skin' }).click()
  await dialog.getByText(/absolute Linux skin-folder path/).waitFor()
  await dialog.getByLabel('Absolute skin-folder path').fill(skinRoot)
  await dialog.getByLabel('INI file relative to skin folder').fill('../Clock.ini')
  await dialog.getByRole('button', { name: 'Import skin' }).click()
  await dialog.getByText(/relative .ini path/).waitFor()
  await dialog.getByLabel('INI file relative to skin folder').fill('Clock.ini')
  let next = await mutate(() => dialog.getByRole('button', { name: 'Import skin' }).click(), '/api/widgets/import', 'POST')
  await dialog.waitFor({ state: 'hidden' })
  assert.equal(next.widgets.length, 1)
  assert.equal(next.widgets[0].enabled, false)
  assert.equal(next.widgets[0].meters.length, 2)
  assert.equal(next.widgets[0].meters[1].clock_format, '%H:%M:%S')
  assert.ok(next.widgets[0].diagnostics.some(d => d.code === 'unsupported_native_meter'))
  const id = next.widgets[0].id
  const card = page.getByRole('article', { name: 'Skin Clock.ini' })
  await card.getByText(/2 supported Text meters \(1 live clock\)/).waitFor()
  await card.getByText(/unsupported_native_meter/).waitFor()
  await mkdir(path.join(root, 'artifacts'), { recursive: true })
  await page.screenshot({ path: path.join(root, 'artifacts/widgets-card-desktop.png'), fullPage: true })
  await page.setViewportSize({ width: 390, height: 844 })
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > window.innerWidth), false, 'Widget card overflows on mobile')
  await page.screenshot({ path: path.join(root, 'artifacts/widgets-card-mobile.png'), fullPage: true })
  await page.setViewportSize({ width: 1440, height: 1000 })
  await card.getByLabel('X offset').fill('80')
  await card.getByLabel('Y offset').fill('65')
  next = await mutate(() => card.getByRole('button', { name: 'Save position' }).click(), `/api/widgets/${id}`, 'PATCH')
  assert.equal(next.widgets[0].x, 80)
  assert.equal(next.widgets[0].y, 65)
  await nav('Library').click()
  await nav('Displays').click()
  await nav('Widgets').click()
  assert.equal(await card.getByLabel('X offset').inputValue(), '80')
  await page.reload()
  await nav('Widgets').click()
  assert.equal((await state()).widgets[0].x, 80)
  const enabledResponse = await mutate(() => card.getByRole('switch', { name: 'Enable Clock.ini' }).click(), `/api/widgets/${id}`, 'PATCH', 503)
  assert.match(enabledResponse.error, /widget bridge is not supported/i)
  await card.getByRole('switch', { name: 'Enable Clock.ini' }).waitFor()
  assert.equal(await card.getByRole('switch', { name: 'Enable Clock.ini' }).getAttribute('aria-checked'), 'false')
  assert.equal((await state()).widgets[0].enabled, false)
  await card.getByRole('button', { name: 'Remove skin' }).click()
  await page.getByRole('dialog').getByRole('button', { name: 'Keep skin' }).click()
  assert.equal((await state()).widgets.length, 1)
  await card.getByRole('button', { name: 'Remove skin' }).click()
  await mutate(() => page.getByRole('dialog').getByRole('button', { name: 'Remove skin' }).click(), `/api/widgets/${id}`, 'DELETE')
  await page.getByRole('heading', { name: 'No skins imported' }).waitFor()
  assert.equal((await state()).widgets.length, 0)
  await nav('Library').click()
  await page.getByRole('button', { name: 'Import wallpaper', exact: true }).click()
  const sceneDialog = page.getByRole('dialog')
  await sceneDialog.getByLabel('Absolute file or project-folder path').fill(path.join(root, 'examples/scenes/RotatingCube.wallin-scene.json'))
  const sceneState = await mutate(() => sceneDialog.getByRole('button', { name: 'Import wallpaper', exact: true }).click(), '/api/import', 'POST')
  await sceneDialog.waitFor({ state: 'hidden' })
  const scene = sceneState.wallpapers.find(w => w.title === 'RotatingCube')
  assert.equal(scene?.kind, 'scene')
  assert.equal(scene?.supported, true)
  await page.getByRole('complementary', { name: 'Wallpaper details' }).getByText('Wall-in native 3D scene').waitFor()
  await page.getByText(/Native 3D scenes render on the desktop/).waitFor()
  await nav('Compatibility').click()
  await page.getByRole('heading', { name: 'Wall-in native 3D scenes' }).waitFor()
  await page.getByRole('heading', { name: 'Wallpaper Engine scene projects' }).waitFor()
  await nav('Library').click()
  await page.getByRole('button', { name: 'Remove from library' }).click()
  await mutate(() => page.getByRole('dialog').getByRole('button', { name: 'Remove wallpaper' }).click(), `/api/wallpapers/${scene.id}`, 'DELETE')
  assert.equal((await state()).wallpapers.some(w => w.id === scene.id), false)
  for (const width of [390, 1440]) {
    await page.setViewportSize({ width, height: width === 390 ? 844 : 1000 })
    for (const route of ['Library', 'Favorites', 'Playlists', 'Displays', 'Widgets', 'Settings', 'Compatibility']) {
      await nav(route).click()
      assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > window.innerWidth), false, `${route} overflows at ${width}px`)
      if (route === 'Widgets') {
        await mkdir(path.join(root, 'artifacts'), { recursive: true })
        await page.screenshot({ path: path.join(root, `artifacts/widgets-${width}.png`), fullPage: true })
      }
    }
  }
  assert.deepEqual(errors, [])
  console.log('PASS live-clock import, diagnostics, persistence, unsupported current bridge error, native 3D scene classification, removal, and seven routes at mobile/desktop widths')
} finally {
  await browser.close()
}
