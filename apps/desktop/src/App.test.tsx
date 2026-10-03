import { render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import App from './App'
import { filterWallpapers } from './api'
import type { AppState, Wallpaper, Widget } from './api'

const image: Wallpaper = {
  id: 'sample',
  title: 'Test image',
  kind: 'image',
  path: '/home/test/wall.jpg',
  preview_url: '/api/media/sample',
  media_url: '/api/media/sample',
  favorite: false,
  tags: ['nature'],
  supported: true,
  description: 'Imported test media',
}
const initial: AppState = {
  wallpapers: [],
  settings: {
    volume: 50,
    muted: true,
    fit: 'cover',
    monitor: 'all',
    pause_on_battery: true,
    pause_on_fullscreen: false,
    autostart: false,
    playlist_ids: [],
    playlist_interval: 300,
    shuffle: false,
  },
  playback: { wallpaper_id: null, status: 'stopped', error: null },
  platform: {
    desktop: 'Test desktop',
    session: 'wayland',
    backend: 'test',
    available: true,
    message: 'Test integration available',
  },
  monitors: [{ id: 'all', name: 'All displays', width: 1920, height: 1080 }],
  steam_paths: [],
  widgets: [],
  widget_error: null,
}
const skin: Widget = {
  id: 'skin_clock', root: '/home/test/Skins/Clock', ini: 'Clock.ini',
  enabled: false, monitor: 'all', x: 0, y: 0,
  meters: [{ id: 'skin_clock_0', x: 10, y: 20, width: 150, height: 40, text: 'Hello', color: '#ffffffff', font_size: 18 }],
  diagnostics: [{ section: 'Shape', code: 'unsupported_native_meter', message: 'Image and Shape meters are not rendered by the native widget bridge' }],
}
let state: AppState
let fetchMock: ReturnType<typeof vi.fn>
beforeEach(() => {
  state = structuredClone(initial)
  fetchMock = vi.fn(
    async (_url: string, _options?: RequestInit) =>
      new Response(JSON.stringify(state), { status: 200 }),
  )
  vi.stubGlobal('fetch', fetchMock)
})
async function openApp() {
  render(<App />)
  await screen.findByRole('heading', {
    name: 'A fresh start for your desktop.',
  })
}

describe('Wall-in workspace', () => {
  it('shows an honest empty library with working import entry and no fabricated media', async () => {
    await openApp()
    expect(screen.getByText('0 wallpapers')).toBeInTheDocument()
    expect(screen.queryAllByRole('img')).toHaveLength(0)
    await userEvent.click(
      screen.getByRole('button', { name: 'Import your first wallpaper' }),
    )
    expect(screen.getByRole('dialog')).toBeInTheDocument()
    expect(
      screen.getByLabelText('Absolute file or project-folder path'),
    ).toHaveFocus()
  })
  it('rejects relative paths locally and submits an absolute path exactly', async () => {
    await openApp()
    await userEvent.click(
      screen.getByRole('button', { name: 'Import your first wallpaper' }),
    )
    const dialog = screen.getByRole('dialog')
    const input = screen.getByLabelText('Absolute file or project-folder path')
    await userEvent.type(input, '~/wall.jpg')
    await userEvent.click(
      within(dialog).getByRole('button', { name: 'Import wallpaper' }),
    )
    expect(screen.getByRole('alert')).toHaveTextContent('absolute Linux path')
    expect(fetchMock).toHaveBeenCalledTimes(1)
    await userEvent.clear(input)
    await userEvent.type(input, '/home/test/wall.jpg')
    state.wallpapers = [image]
    await userEvent.click(
      within(dialog).getByRole('button', { name: 'Import wallpaper' }),
    )
    await waitFor(() =>
      expect(screen.queryByRole('dialog')).not.toBeInTheDocument(),
    )
    expect(fetchMock).toHaveBeenCalledWith(
      '/api/import',
      expect.objectContaining({
        method: 'POST',
        body: JSON.stringify({ path: '/home/test/wall.jpg' }),
      }),
    )
    expect(
      screen.getByRole('button', { name: 'Preview Test image' }),
    ).toBeInTheDocument()
  })
  it('surfaces backend import errors without dismissing the path', async () => {
    await openApp()
    await userEvent.click(
      screen.getByRole('button', { name: 'Import your first wallpaper' }),
    )
    await userEvent.type(
      screen.getByLabelText('Absolute file or project-folder path'),
      '/missing/file.jpg',
    )
    fetchMock.mockResolvedValueOnce(
      new Response(JSON.stringify({ error: 'Path does not exist' }), {
        status: 400,
      }),
    )
    await userEvent.click(
      within(screen.getByRole('dialog')).getByRole('button', {
        name: 'Import wallpaper',
      }),
    )
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Path does not exist',
    )
    expect(
      screen.getByLabelText('Absolute file or project-folder path'),
    ).toHaveValue('/missing/file.jpg')
  })
  it('reports service failures and reconnects on retry', async () => {
    fetchMock.mockRejectedValueOnce(new TypeError('offline'))
    render(<App />)
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Wall-in service is unavailable',
    )
    await userEvent.click(screen.getByRole('button', { name: 'Retry' }))
    await screen.findByRole('heading', {
      name: 'A fresh start for your desktop.',
    })
    expect(screen.queryByRole('alert')).not.toBeInTheDocument()
  })
  it('can favorite, filter, preview, apply, and safely remove actual items', async () => {
    state.wallpapers = [structuredClone(image)]
    fetchMock.mockImplementation(async (url: string, options?: RequestInit) => {
      if (url.endsWith('/sample') && options?.method === 'PATCH')
        state.wallpapers[0].favorite = true
      if (url.endsWith('/sample') && options?.method === 'DELETE')
        state.wallpapers = []
      return new Response(JSON.stringify(state), { status: 200 })
    })
    render(<App />)
    await userEvent.click(
      await screen.findByRole('button', { name: 'Favorite Test image' }),
    )
    expect(
      await screen.findByRole('button', { name: 'Unfavorite Test image' }),
    ).toHaveAttribute('aria-pressed', 'true')
    await userEvent.type(
      screen.getByRole('textbox', { name: 'Search wallpapers' }),
      'not here',
    )
    expect(
      screen.getByRole('heading', { name: 'No wallpapers match.' }),
    ).toBeInTheDocument()
    await userEvent.click(screen.getByRole('button', { name: 'Clear filters' }))
    await userEvent.click(
      screen.getByRole('button', { name: 'Preview Test image' }),
    )
    await userEvent.click(
      screen.getByRole('button', { name: 'Apply wallpaper' }),
    )
    expect(fetchMock).toHaveBeenCalledWith(
      '/api/playback',
      expect.objectContaining({
        method: 'POST',
        body: JSON.stringify({ action: 'apply', wallpaper_id: 'sample' }),
      }),
    )
    await userEvent.click(
      screen.getByRole('button', { name: 'Remove from library' }),
    )
    expect(screen.getByRole('dialog')).toHaveTextContent('will not be deleted')
    expect(
      fetchMock.mock.calls.some(([, options]) => options?.method === 'DELETE'),
    ).toBe(false)
    await userEvent.click(
      screen.getByRole('button', { name: 'Remove wallpaper' }),
    )
    await screen.findByRole('heading', {
      name: 'A fresh start for your desktop.',
    })
  })
  it('updates automation and playlist controls through the settings API', async () => {
    fetchMock.mockImplementation(async (_url: string, options?: RequestInit) => {
      if (options?.method === 'PATCH')
        state.settings = { ...state.settings, ...JSON.parse(String(options.body)) }
      return new Response(JSON.stringify(state), { status: 200 })
    })
    await openApp()
    await userEvent.click(screen.getByRole('button', { name: 'Settings' }))
    const battery = screen.getByRole('switch', { name: 'Pause on battery' })
    expect(battery).toHaveAttribute('aria-checked', 'true')
    await userEvent.click(battery)
    await waitFor(() => expect(battery).toHaveAttribute('aria-checked', 'false'))
    expect(fetchMock).toHaveBeenCalledWith(
      '/api/settings',
      expect.objectContaining({ method: 'PATCH', body: JSON.stringify({ pause_on_battery: false }) }),
    )
    await userEvent.click(screen.getByRole('button', { name: 'Playlists' }))
    expect(screen.getByText('Rotate the wallpapers you choose.')).toBeInTheDocument()
    await userEvent.selectOptions(screen.getByRole('combobox', { name: 'Rotation interval' }), '60')
    await waitFor(() => expect(state.settings.playlist_interval).toBe(60))
    const shuffle = screen.getByRole('switch', { name: 'Shuffle playlist' })
    await userEvent.click(shuffle)
    await waitFor(() => expect(shuffle).toHaveAttribute('aria-checked', 'true'))
  })
  it('can schedule a supported wallpaper for the next login when integration is unavailable', async () => {
    state.wallpapers = [structuredClone(image)]
    state.settings.autostart = true
    state.platform.available = false
    fetchMock.mockImplementation(async (url: string, options?: RequestInit) => {
      if (url === '/api/playback' && options?.method === 'POST') {
        const command = JSON.parse(String(options.body))
        if (command.action === 'schedule')
          state.playback = { wallpaper_id: command.wallpaper_id, status: 'pending', error: null }
      }
      return new Response(JSON.stringify(state), { status: 200 })
    })
    render(<App />)
    await userEvent.click(await screen.findByRole('button', { name: 'Preview Test image' }))
    expect(screen.getByRole('button', { name: 'Apply wallpaper' })).toBeDisabled()
    await userEvent.click(screen.getByRole('button', { name: 'Use after next login' }))
    expect(fetchMock).toHaveBeenCalledWith('/api/playback', expect.objectContaining({
      method: 'POST', body: JSON.stringify({ action: 'schedule', wallpaper_id: 'sample' }),
    }))
    expect(await screen.findByText('Ready to apply on the next service start')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Resume wallpaper' })).toBeDisabled()
    expect(screen.getByRole('button', { name: 'Cancel scheduled wallpaper' })).toBeEnabled()
  })
  it('scans Steam and reports no discoveries without fabricated counts', async () => {
    await openApp()
    await userEvent.click(screen.getByRole('button', { name: 'Scan Steam' }))
    expect(
      await screen.findByText(/Steam scan complete\. No new projects found/),
    ).toBeInTheDocument()
    expect(fetchMock).toHaveBeenCalledWith(
      '/api/scan-steam',
      expect.objectContaining({ method: 'POST', body: '{}' }),
    )
  })
  it('closes the modal with Escape and returns focus', async () => {
    await openApp()
    const trigger = screen.getByRole('button', {
      name: 'Import your first wallpaper',
    })
    await userEvent.click(trigger)
    await userEvent.keyboard('{Escape}')
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
    expect(trigger).toHaveFocus()
  })
  it('never enables applying unsupported formats', async () => {
    state.wallpapers = [{ ...image, kind: 'scene', supported: false }]
    render(<App />)
    await userEvent.click(
      await screen.findByRole('button', { name: 'Preview Test image' }),
    )
    expect(
      screen.getByRole('button', { name: 'Apply wallpaper' }),
    ).toBeDisabled()
  })
})

describe('Widgets management', () => {
  it('imports a root and relative INI, validates paths, and lists real diagnostics without enabling', async () => {
    fetchMock.mockImplementation(async (url: string, options?: RequestInit) => {
      if (url === '/api/widgets/import' && options?.method === 'POST') state.widgets = [structuredClone(skin)]
      return new Response(JSON.stringify(state), { status: 200 })
    })
    await openApp()
    await userEvent.click(screen.getByRole('button', { name: 'Widgets' }))
    expect(screen.getByText('No skins imported')).toBeInTheDocument()
    await userEvent.click(screen.getAllByRole('button', { name: 'Import skin' })[0])
    const dialog = screen.getByRole('dialog')
    const root = within(dialog).getByRole('textbox', { name: 'Absolute skin-folder path' })
    const ini = within(dialog).getByRole('textbox', { name: 'INI file relative to skin folder' })
    await userEvent.type(root, '~/Skins/Clock')
    await userEvent.type(ini, '../Clock.ini')
    await userEvent.click(within(dialog).getByRole('button', { name: 'Import skin' }))
    expect(within(dialog).getByRole('alert')).toHaveTextContent('absolute Linux skin-folder')
    await userEvent.clear(root)
    await userEvent.type(root, '/home/test/Skins/Clock')
    await userEvent.click(within(dialog).getByRole('button', { name: 'Import skin' }))
    expect(within(dialog).getByRole('alert')).toHaveTextContent('relative .ini path')
    expect(fetchMock).toHaveBeenCalledTimes(1)
    await userEvent.clear(ini)
    await userEvent.type(ini, 'Clock.ini')
    await userEvent.click(within(dialog).getByRole('button', { name: 'Import skin' }))
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument())
    expect(fetchMock).toHaveBeenCalledWith('/api/widgets/import', expect.objectContaining({ method: 'POST', body: JSON.stringify({ root: skin.root, ini: skin.ini }) }))
    const row = screen.getByRole('article', { name: 'Skin Clock.ini' })
    expect(within(row).getByText(/unsupported_native_meter/)).toHaveTextContent('Image and Shape meters are not rendered')
    expect(within(row).getByRole('switch', { name: 'Enable Clock.ini' })).toHaveAttribute('aria-checked', 'false')
  })

  it('keeps import paths when backend rejects a skin with no supported Text meters', async () => {
    await openApp()
    await userEvent.click(screen.getByRole('button', { name: 'Widgets' }))
    await userEvent.click(screen.getAllByRole('button', { name: 'Import skin' })[0])
    const dialog = screen.getByRole('dialog')
    await userEvent.type(within(dialog).getByRole('textbox', { name: 'Absolute skin-folder path' }), '/home/test/Skins/Image')
    await userEvent.type(within(dialog).getByRole('textbox', { name: 'INI file relative to skin folder' }), 'Image.ini')
    fetchMock.mockResolvedValueOnce(new Response(JSON.stringify({ error: 'Skin contains no native-compatible Text meters' }), { status: 400 }))
    await userEvent.click(within(dialog).getByRole('button', { name: 'Import skin' }))
    expect(await within(dialog).findByRole('alert')).toHaveTextContent('no native-compatible Text meters')
    expect(within(dialog).getByRole('textbox', { name: 'INI file relative to skin folder' })).toHaveValue('Image.ini')
    expect(screen.queryByRole('article', { name: 'Skin Image.ini' })).not.toBeInTheDocument()
  })

  it('explains when the running GNOME extension needs a new login', async () => {
    state.widget_error = 'GNOME widget bridge is not supported by this extension'
    await openApp()
    await userEvent.click(screen.getByRole('button', { name: 'Widgets' }))
    expect(screen.getByRole('alert')).toHaveTextContent('sign out, then sign back in')
    expect(screen.getByRole('alert')).toHaveTextContent('Restarting Wall-in alone will not reload GNOME Shell')
  })

  it('shows backend errors and leaves failed edits uncommitted', async () => {
    state.widgets = [structuredClone(skin)]
    state.widget_error = 'Widget helper exited; widgets are not displayed'
    fetchMock.mockImplementation(async (url: string, options?: RequestInit) => {
      if (url.startsWith('/api/widgets/') && options?.method === 'PATCH') return new Response(JSON.stringify({ error: 'Widget bridge refused update' }), { status: 503 })
      return new Response(JSON.stringify(state), { status: 200 })
    })
    await openApp()
    await userEvent.click(screen.getByRole('button', { name: 'Widgets' }))
    expect(screen.getByRole('alert')).toHaveTextContent('Widget helper exited')
    const toggle = screen.getByRole('switch', { name: 'Enable Clock.ini' })
    await userEvent.click(toggle)
    expect(await screen.findByText('Widget bridge refused update')).toBeInTheDocument()
    expect(toggle).toHaveAttribute('aria-checked', 'false')
    expect(fetchMock).toHaveBeenCalledWith('/api/widgets/skin_clock', expect.objectContaining({ method: 'PATCH', body: JSON.stringify({ enabled: true }) }))
  })

  it('updates monitor and position, toggles enablement, and confirms removal without deleting source files', async () => {
    state.widgets = [structuredClone(skin)]
    state.monitors = [{ id: 'test-0', name: 'Test display', width: 1920, height: 1080 }]
    fetchMock.mockImplementation(async (url: string, options?: RequestInit) => {
      if (url === '/api/widgets/skin_clock' && options?.method === 'PATCH') Object.assign(state.widgets[0], JSON.parse(String(options.body)))
      if (url === '/api/widgets/skin_clock' && options?.method === 'DELETE') state.widgets = []
      return new Response(JSON.stringify(state), { status: 200 })
    })
    await openApp()
    await userEvent.click(screen.getByRole('button', { name: 'Widgets' }))
    const row = screen.getByRole('article', { name: 'Skin Clock.ini' })
    await userEvent.selectOptions(within(row).getByRole('combobox', { name: 'Monitor' }), 'test-0')
    await waitFor(() => expect(state.widgets[0].monitor).toBe('test-0'))
    const x = within(row).getByRole('spinbutton', { name: 'X offset' })
    const y = within(row).getByRole('spinbutton', { name: 'Y offset' })
    await userEvent.clear(x)
    await userEvent.type(x, '30')
    await userEvent.clear(y)
    await userEvent.type(y, '40')
    await userEvent.click(within(row).getByRole('button', { name: 'Save position' }))
    await waitFor(() => expect(state.widgets[0]).toMatchObject({ x: 30, y: 40 }))
    expect(fetchMock).toHaveBeenCalledWith('/api/widgets/skin_clock', expect.objectContaining({ method: 'PATCH', body: JSON.stringify({ x: 30, y: 40 }) }))
    await userEvent.click(within(row).getByRole('switch', { name: 'Enable Clock.ini' }))
    await waitFor(() => expect(within(row).getByRole('switch')).toHaveAttribute('aria-checked', 'true'))
    await userEvent.click(within(row).getByRole('switch'))
    await waitFor(() => expect(within(row).getByRole('switch')).toHaveAttribute('aria-checked', 'false'))
    await userEvent.click(within(row).getByRole('button', { name: 'Remove skin' }))
    expect(screen.getByRole('dialog')).toHaveTextContent('original skin files will not be deleted')
    expect(fetchMock.mock.calls.some(([, options]) => options?.method === 'DELETE')).toBe(false)
    await userEvent.click(within(screen.getByRole('dialog')).getByRole('button', { name: 'Remove skin' }))
    expect(await screen.findByText('No skins imported')).toBeInTheDocument()
  })
})

it('filters titles, tags, types, favorites and sorts without mutating source data', () => {
  const wallpapers = [
    { ...image, title: 'Zebra' },
    { ...image, id: 'a', title: 'Apple', favorite: true },
  ]
  expect(
    filterWallpapers(wallpapers, 'NATURE', 'image', 'az', false).map(
      (w) => w.title,
    ),
  ).toEqual(['Apple', 'Zebra'])
  expect(wallpapers[0].title).toBe('Zebra')
  expect(
    filterWallpapers(wallpapers, '', 'all', 'za', true).map((w) => w.title),
  ).toEqual(['Apple'])
  expect(filterWallpapers(wallpapers, '', 'video', 'az', false)).toEqual([])
})
