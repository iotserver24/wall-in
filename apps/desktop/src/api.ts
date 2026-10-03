export type WallpaperKind = 'video' | 'image' | 'web' | 'scene' | 'application'
export interface Wallpaper {
  id: string
  title: string
  kind: WallpaperKind
  path: string
  preview_url: string | null
  media_url: string | null
  favorite: boolean
  tags: string[]
  supported: boolean
  description: string
}
export interface Settings {
  volume: number
  muted: boolean
  fit: 'cover' | 'contain' | 'fill'
  monitor: string
  pause_on_battery: boolean
  pause_on_fullscreen: boolean
  autostart: boolean
  playlist_ids: string[]
  playlist_interval: number
  shuffle: boolean
}
export interface Widget {
  id: string
  root: string
  ini: string
  enabled: boolean
  monitor: string
  x: number
  y: number
  meters: {
    id: string
    x: number
    y: number
    width: number
    height: number
    text: string
    clock_format?: string
    color: string
    font_size: number
  }[]
  diagnostics: { section: string; code: string; message: string }[]
}
export interface AppState {
  widgets: Widget[]
  widget_error: string | null
  wallpapers: Wallpaper[]
  settings: Settings
  playback: {
    wallpaper_id: string | null
    status: 'playing' | 'paused' | 'stopped' | 'pending'
    error: string | null
  }
  platform: {
    desktop: string
    session: string
    backend: string
    available: boolean
    message: string
  }
  monitors: { id: string; name: string; width: number; height: number }[]
  steam_paths: string[]
}

export async function requestState(
  path = '/api/state',
  method = 'GET',
  body?: unknown,
): Promise<AppState> {
  let response: Response
  try {
    response = await fetch(path, {
      method,
      headers:
        body === undefined ? undefined : { 'Content-Type': 'application/json' },
      body: body === undefined ? undefined : JSON.stringify(body),
      signal: AbortSignal.timeout(30000),
    })
  } catch {
    throw new Error(
      'Cannot reach the Wall-in service. Make sure it is running on 127.0.0.1:3977, then retry.',
    )
  }
  const data = await response.json().catch(() => null)
  if (!response.ok)
    throw new Error(
      data?.error ||
        `The service returned an error (${response.status}). Please retry.`,
    )
  if (
    !data ||
    !Array.isArray(data.wallpapers) ||
    !data.settings ||
    !data.platform ||
    !data.playback ||
    !Array.isArray(data.monitors) ||
    !Array.isArray(data.steam_paths) ||
    !Array.isArray(data.widgets) ||
    !(typeof data.widget_error === 'string' || data.widget_error === null)
  ) {
    throw new Error(
      'The service returned an unexpected response. Check that the Wall-in backend is up to date, then retry.',
    )
  }
  return data as AppState
}

export function filterWallpapers(
  wallpapers: Wallpaper[],
  query: string,
  kind: string,
  sort: string,
  favorites: boolean,
) {
  const search = query.trim().toLocaleLowerCase()
  return wallpapers
    .filter(
      (w) =>
        (!favorites || w.favorite) &&
        (kind === 'all' || w.kind === kind) &&
        (!search ||
          [w.title, w.description, ...w.tags]
            .join(' ')
            .toLocaleLowerCase()
            .includes(search)),
    )
    .sort((a, b) =>
      sort === 'za'
        ? b.title.localeCompare(a.title)
        : sort === 'kind'
          ? a.kind.localeCompare(b.kind) || a.title.localeCompare(b.title)
          : a.title.localeCompare(b.title),
    )
}
