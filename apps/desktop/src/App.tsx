import { useCallback, useEffect, useRef, useState } from 'react'
import type { FormEvent, ReactNode } from 'react'
import {
  ArrowDownToLine,
  ArrowRight,
  Check,
  ChevronRight,
  CircleHelp,
  Clapperboard,
  FolderOpen,
  Heart,
  Image,
  Layers3,
  LayoutGrid,
  ListMusic,
  LoaderCircle,
  Monitor,
  Pause,
  Play,
  Plus,
  RefreshCw,
  Search,
  Settings2,
  ShieldCheck,
  Square,
  Trash2,
  Volume2,
  VolumeX,
  Wallpaper as WallpaperIcon,
  X,
} from 'lucide-react'
import { filterWallpapers, requestState } from './api'
import type { AppState, Settings, Wallpaper, Widget } from './api'

type Page =
  | 'Library'
  | 'Favorites'
  | 'Playlists'
  | 'Displays'
  | 'Widgets'
  | 'Settings'
  | 'Compatibility'
type Mutation = (
  path: string,
  method: string,
  body?: unknown,
  message?: string,
) => Promise<AppState | null>
const navigation = [
  { name: 'Library', icon: LayoutGrid },
  { name: 'Favorites', icon: Heart },
  { name: 'Playlists', icon: ListMusic },
  { name: 'Displays', icon: Monitor },
  { name: 'Widgets', icon: Layers3 },
  { name: 'Settings', icon: Settings2 },
  { name: 'Compatibility', icon: ShieldCheck },
] as const
const subtitles: Record<Page, string> = {
  Library: 'A space for everything that moves you.',
  Favorites: 'The wallpapers you keep coming back to.',
  Playlists: 'Your collection, in your order.',
  Displays: 'Find the right home for your wallpaper.',
  Widgets: 'Manage local Rainmeter skins on your desktop.',
  Settings: 'Make Wall-in work your way.',
  Compatibility: 'Know what runs. Know what comes next.',
}
const kindNames = {
  image: 'Image',
  video: 'Video',
  web: 'Web',
  scene: 'Scene',
  application: 'Application',
}

function wallpaperKindName(wallpaper: Wallpaper) {
  return wallpaper.kind === 'scene' && wallpaper.path.endsWith('.wallin-scene.json')
    ? 'Wall-in native 3D scene'
    : kindNames[wallpaper.kind]
}

export default function App() {
  const [state, setState] = useState<AppState | null>(null)
  const [loading, setLoading] = useState(true)
  const [connectionError, setConnectionError] = useState('')
  const [actionError, setActionError] = useState('')
  const [notice, setNotice] = useState('')
  const [busy, setBusy] = useState(false)
  const locked = useRef(false)
  const generation = useRef(0)
  const mounted = useRef(true)
  const [page, setPage] = useState<Page>('Library')
  const [query, setQuery] = useState('')
  const [kind, setKind] = useState('all')
  const [sort, setSort] = useState('az')
  const [selectedId, setSelectedId] = useState<string | null>(null)
  const [importOpen, setImportOpen] = useState(false)
  const [remove, setRemove] = useState<Wallpaper | null>(null)
  const [widgetImportOpen, setWidgetImportOpen] = useState(false)
  const [removeWidget, setRemoveWidget] = useState<Widget | null>(null)

  const refresh = useCallback(async (quiet = false) => {
    if (locked.current) return
    const version = generation.current
    if (!quiet) setLoading(true)
    try {
      const next = await requestState(
        quiet ? '/api/state' : '/api/state?refresh_platform=true',
      )
      if (!mounted.current || generation.current !== version || locked.current)
        return
      setState(next)
      setConnectionError('')
    } catch (error) {
      if (mounted.current && generation.current === version && !locked.current)
        setConnectionError((error as Error).message)
    } finally {
      if (mounted.current) setLoading(false)
    }
  }, [])
  useEffect(() => {
    mounted.current = true
    void refresh()
    const timer = window.setInterval(() => {
      if (!document.hidden) void refresh(true)
    }, 10000)
    return () => {
      mounted.current = false
      window.clearInterval(timer)
    }
  }, [refresh])
  useEffect(() => {
    if (!notice) return
    const timer = window.setTimeout(() => setNotice(''), 6000)
    return () => window.clearTimeout(timer)
  }, [notice])

  const mutate: Mutation = async (path, method, body, message) => {
    if (locked.current) return null
    locked.current = true
    generation.current += 1
    setBusy(true)
    setActionError('')
    try {
      const next = await requestState(path, method, body)
      setState(next)
      setConnectionError('')
      if (message) setNotice(message)
      return next
    } catch (error) {
      setActionError((error as Error).message)
      return null
    } finally {
      locked.current = false
      setBusy(false)
    }
  }
  const updateSettings = (patch: Partial<Settings>) =>
    mutate('/api/settings', 'PATCH', patch, 'Preferences saved.')
  const playback = (action: string, wallpaperId?: string) =>
    mutate('/api/playback', 'POST', {
      action,
      ...(wallpaperId ? { wallpaper_id: wallpaperId } : {}),
    })
  const scan = async () => {
    const before = new Set(state?.wallpapers.map((w) => w.id) ?? [])
    const next = await mutate('/api/scan-steam', 'POST', {})
    if (next) {
      const added = next.wallpapers.filter((w) => !before.has(w.id)).length
      setNotice(
        added
          ? `Steam scan complete. Added ${added} ${added === 1 ? 'wallpaper' : 'wallpapers'}.`
          : 'Steam scan complete. No new projects found. You can also import a project folder by path.',
      )
    }
  }
  const selected = state?.wallpapers.find((w) => w.id === selectedId)
  const active = state?.wallpapers.find(
    (w) => w.id === state.playback.wallpaper_id,
  )
  const items = filterWallpapers(
    state?.wallpapers ?? [],
    query,
    kind,
    sort,
    page === 'Favorites',
  )
  const collection =
    state?.wallpapers.filter((w) => page !== 'Favorites' || w.favorite) ?? []
  const unavailable = busy || !state || Boolean(connectionError)

  return (
    <div className="app-shell">
      <a className="skip-link" href="#main-content">
        Skip to content
      </a>
      <aside className="sidebar">
        <a
          className="brand"
          href="#"
          onClick={(e) => {
            e.preventDefault()
            setPage('Library')
          }}
          aria-label="Wall-in home"
        >
          <span className="brand-mark">
            <Layers3 size={25} strokeWidth={1.7} />
          </span>
          <span>
            wall-in<span className="brand-period">.</span>
          </span>
        </a>
        <div className="sidebar-caption">YOUR DESKTOP, IN MOTION</div>
        <nav aria-label="Main navigation">
          {navigation.map(({ name, icon: Icon }, index) => (
            <button
              key={name}
              className={`nav-item ${page === name ? 'active' : ''} ${index === 3 ? 'nav-break' : ''}`}
              aria-current={page === name ? 'page' : undefined}
              onClick={() => {
                setPage(name)
                setActionError('')
              }}
            >
              <Icon size={18} />
              <span>{name}</span>
              {name === 'Library' && state && (
                <span className="nav-count">{state.wallpapers.length}</span>
              )}
            </button>
          ))}
        </nav>
        <div className="sidebar-bottom">
          <span className="local-label">
            <span
              className={`status-dot ${connectionError ? 'offline' : state ? 'online' : ''}`}
            />
            {connectionError
              ? 'Service disconnected'
              : state
                ? 'Local service connected'
                : 'Connecting to service'}
          </span>
          <span className="sidebar-system">
            {state
              ? `${state.platform.desktop || 'Unknown desktop'} · ${state.platform.session || 'Unknown session'}`
              : 'Runs locally. Stays yours.'}
          </span>
        </div>
      </aside>

      <div className="workspace">
        <header className="topbar">
          <div className="breadcrumb">
            <span>Workspace</span>
            <ChevronRight size={14} />
            <strong>{page}</strong>
          </div>
          <div className="topbar-right">
            <span className="desktop-label">
              <Monitor size={14} />
              {state?.platform.desktop || 'Linux desktop'}
            </span>
            <button
              className="icon-button"
              aria-label="Refresh library"
              title="Refresh library"
              disabled={loading || busy}
              onClick={() => void refresh()}
            >
              <RefreshCw size={16} className={loading ? 'spin' : ''} />
            </button>
          </div>
        </header>
        <main id="main-content" className="main-content">
          <div className="page-heading">
            <div>
              <h1>{page === 'Library' ? 'Your library' : page}</h1>
              <p>{subtitles[page]}</p>
            </div>
            <div className="header-actions">
              {page === 'Widgets' ? (
                <button
                  className="button primary"
                  disabled={unavailable}
                  onClick={() => {
                    setActionError('')
                    setWidgetImportOpen(true)
                  }}
                >
                  <Plus size={17} />
                  Import skin
                </button>
              ) : <>
              <button
                className="button secondary scan-button"
                disabled={unavailable}
                onClick={() => void scan()}
              >
                <ArrowDownToLine size={16} />
                Scan Steam
              </button>
              <button
                className="button primary"
                disabled={unavailable}
                onClick={() => {
                  setActionError('')
                  setImportOpen(true)
                }}
              >
                <Plus size={17} />
                Import wallpaper
              </button>
              </>}
            </div>
          </div>
          {connectionError && (
            <div className="alert error" role="alert">
              <CircleHelp size={18} />
              <div>
                <strong>Wall-in service is unavailable</strong>
                <p>
                  {connectionError}
                  {state ? ' Your last loaded library is shown below.' : ''}
                </p>
              </div>
              <button
                className="button small"
                disabled={loading}
                onClick={() => void refresh()}
              >
                Retry
              </button>
            </div>
          )}
          {actionError && !importOpen && !remove && !widgetImportOpen && !removeWidget && (
            <div className="alert error" role="alert">
              <CircleHelp size={18} />
              <p>{actionError}</p>
              <button
                className="icon-button"
                aria-label="Dismiss error"
                onClick={() => setActionError('')}
              >
                <X size={16} />
              </button>
            </div>
          )}
          {state?.playback.error && (
            <div className="alert error" role="alert">
              <CircleHelp size={18} />
              <p>Playback: {state.playback.error}</p>
              <button
                className="button small"
                onClick={() => setPage('Compatibility')}
              >
                Check compatibility
              </button>
            </div>
          )}
          {!state && loading ? (
            <div className="loading-state" role="status">
              <LoaderCircle className="spin" size={26} />
              <h2>Opening your workspace</h2>
              <p>Connecting to the local Wall-in service.</p>
            </div>
          ) : !state ? (
            <div className="empty-state connection-empty">
              <Monitor size={42} strokeWidth={1.2} />
              <h2>Your desktop is waiting.</h2>
              <p>
                Start the Wall-in backend to load your library and desktop
                controls. No library data is stored in this browser.
              </p>
              <button
                className="button secondary"
                onClick={() => void refresh()}
              >
                <RefreshCw size={16} />
                Reconnect
              </button>
            </div>
          ) : (
            <>
              {(page === 'Library' || page === 'Favorites') && (
                <>
                  <div className="library-toolbar">
                    <label className="search-box">
                      <Search size={17} />
                      <input
                        aria-label="Search wallpapers"
                        placeholder="Search your wallpapers…"
                        value={query}
                        onChange={(e) => setQuery(e.target.value)}
                      />
                      {query && (
                        <button
                          className="icon-button"
                          aria-label="Clear search"
                          onClick={() => setQuery('')}
                        >
                          <X size={15} />
                        </button>
                      )}
                    </label>
                    <label className="select-wrap">
                      <span className="sr-only">Wallpaper type</span>
                      <select
                        value={kind}
                        onChange={(e) => setKind(e.target.value)}
                      >
                        <option value="all">All types</option>
                        {Object.entries(kindNames).map(([value, title]) => (
                          <option key={value} value={value}>
                            {title}
                          </option>
                        ))}
                      </select>
                    </label>
                    <label className="select-wrap sort-select">
                      <span className="sr-only">Sort wallpapers</span>
                      <select
                        value={sort}
                        onChange={(e) => setSort(e.target.value)}
                      >
                        <option value="az">Name: A–Z</option>
                        <option value="za">Name: Z–A</option>
                        <option value="kind">Type</option>
                      </select>
                    </label>
                  </div>
                  <div
                    className={`library-layout ${selected ? 'has-selection' : ''}`}
                  >
                    <section
                      className="gallery-area"
                      aria-label="Wallpaper library"
                    >
                      <div className="collection-heading">
                        <span>
                          {page === 'Favorites'
                            ? 'FAVORITES'
                            : 'ALL WALLPAPERS'}
                        </span>
                        <span>
                          {items.length}{' '}
                          {items.length === 1 ? 'wallpaper' : 'wallpapers'}
                        </span>
                      </div>
                      {collection.length === 0 && !query && kind === 'all' ? (
                        <div className="empty-state library-empty">
                          <div className="empty-symbol">
                            <WallpaperIcon size={46} strokeWidth={1.1} />
                            <span className="empty-plus">
                              <Plus size={17} />
                            </span>
                          </div>
                          <h2>
                            {page === 'Favorites'
                              ? 'Keep your favorites close.'
                              : 'A fresh start for your desktop.'}
                          </h2>
                          <p>
                            {page === 'Favorites'
                              ? 'Tap the heart on a wallpaper in your library. Your favorites will be right here.'
                              : 'Bring a little of your world into your workspace. Import a local image, video, or a Steam wallpaper project.'}
                          </p>
                          <button
                            className="button primary"
                            disabled={unavailable}
                            onClick={() =>
                              page === 'Favorites'
                                ? setPage('Library')
                                : setImportOpen(true)
                            }
                          >
                            {page === 'Favorites' ? (
                              <LayoutGrid size={16} />
                            ) : (
                              <Plus size={16} />
                            )}
                            {page === 'Favorites'
                              ? 'Explore your library'
                              : 'Import your first wallpaper'}
                          </button>
                          {page === 'Library' && (
                            <button
                              className="text-button"
                              disabled={unavailable}
                              onClick={() => void scan()}
                            >
                              Already have Steam projects? Scan your library{' '}
                              <ArrowRight size={14} />
                            </button>
                          )}
                          <div className="empty-formats">
                            <span>
                              <Image size={14} />
                              Images
                            </span>
                            <span>
                              <Clapperboard size={14} />
                              Videos
                            </span>
                            <span>
                              <FolderOpen size={14} />
                              Steam projects
                            </span>
                          </div>
                        </div>
                      ) : !items.length ? (
                        <div className="empty-state filtered-empty">
                          <Search size={32} strokeWidth={1.4} />
                          <h2>No wallpapers match.</h2>
                          <p>Try another search or choose a different type.</p>
                          <button
                            className="button secondary"
                            onClick={() => {
                              setQuery('')
                              setKind('all')
                            }}
                          >
                            Clear filters
                          </button>
                        </div>
                      ) : (
                        <div className="wallpaper-grid">
                          {items.map((w) => (
                            <article
                              className={`wallpaper-card ${selectedId === w.id ? 'selected' : ''}`}
                              key={w.id}
                            >
                              <button
                                className="wallpaper-select"
                                aria-label={`Preview ${w.title}`}
                                aria-pressed={selectedId === w.id}
                                onClick={() => setSelectedId(w.id)}
                              >
                                <div className="card-art">
                                  <Media wallpaper={w} thumbnail />
                                  <span className="kind-label">
                                    {wallpaperKindName(w)}
                                  </span>
                                  {active?.id === w.id &&
                                    state.playback.status !== 'stopped' && (
                                      <span className="playing-label">
                                        {state.playback.status === 'playing' ? (
                                          <Play size={11} />
                                        ) : (
                                          <Pause size={11} />
                                        )}
                                        {state.playback.status}
                                      </span>
                                    )}
                                </div>
                                <div className="card-info">
                                  <h3>{w.title}</h3>
                                  <span>
                                    {w.supported
                                      ? state.platform.available
                                        ? 'Ready to apply'
                                        : 'Needs desktop integration'
                                      : 'Renderer unavailable'}
                                  </span>
                                </div>
                              </button>
                              <button
                                className={`favorite-button ${w.favorite ? 'is-favorite' : ''}`}
                                aria-label={`${w.favorite ? 'Unfavorite' : 'Favorite'} ${w.title}`}
                                aria-pressed={w.favorite}
                                disabled={unavailable}
                                onClick={() =>
                                  void mutate(
                                    `/api/wallpapers/${encodeURIComponent(w.id)}`,
                                    'PATCH',
                                    { favorite: !w.favorite },
                                  )
                                }
                              >
                                <Heart
                                  size={16}
                                  fill={w.favorite ? 'currentColor' : 'none'}
                                />
                              </button>
                            </article>
                          ))}
                        </div>
                      )}
                      {page === 'Library' && !state.wallpapers.length && (
                        <div className="library-footnote">
                          <ShieldCheck size={17} />
                          <p>
                            <strong>Your files stay yours.</strong> Import from
                            your computer. Nothing is uploaded.
                          </p>
                          <button
                            className="text-button"
                            onClick={() => setPage('Compatibility')}
                          >
                            What’s supported <ArrowRight size={14} />
                          </button>
                        </div>
                      )}
                    </section>
                    {selected && (
                      <Detail
                        wallpaper={selected}
                        state={state}
                        disabled={unavailable}
                        onClose={() => setSelectedId(null)}
                        onRemove={() => {
                          setActionError('')
                          setRemove(selected)
                        }}
                        apply={() => void playback('apply', selected.id)}
                        schedule={() => void playback('schedule', selected.id)}
                        updateSettings={updateSettings}
                      />
                    )}
                  </div>
                </>
              )}
              {page === 'Displays' && (
                <Displays
                  state={state}
                  disabled={unavailable}
                  updateSettings={updateSettings}
                />
              )}
              {page === 'Widgets' && (
                <WidgetsPage
                  state={state}
                  disabled={unavailable}
                  mutate={mutate}
                  onImport={() => {
                    setActionError('')
                    setWidgetImportOpen(true)
                  }}
                  onRemove={(widget) => {
                    setActionError('')
                    setRemoveWidget(widget)
                  }}
                />
              )}
              {page === 'Settings' && (
                <SettingsPage
                  state={state}
                  disabled={unavailable}
                  updateSettings={updateSettings}
                />
              )}
              {page === 'Playlists' && (
                <Playlists
                  state={state}
                  disabled={unavailable}
                  updateSettings={updateSettings}
                />
              )}
              {page === 'Compatibility' && <Compatibility state={state} />}
            </>
          )}
        </main>
        <footer className="playback-bar">
          <div className="now-playing-art">
            {active ? (
              <Media wallpaper={active} thumbnail />
            ) : (
              <WallpaperIcon size={22} strokeWidth={1.5} />
            )}
          </div>
          <div className="now-playing">
            <strong>{active?.title || 'Nothing playing'}</strong>
            <span>
              {state?.playback.status === 'playing'
                ? 'Playing on your desktop'
                : state?.playback.status === 'paused'
                  ? 'Desktop playback paused'
                  : state?.playback.status === 'pending'
                    ? 'Ready to apply on the next service start'
                    : active && state?.playback.error
                      ? 'Could not start on this desktop'
                      : 'Choose a wallpaper to make it yours'}
            </span>
          </div>
          <div className="transport">
            <button
              className="transport-play"
              aria-label={
                state?.playback.status === 'playing'
                  ? 'Pause wallpaper'
                  : 'Resume wallpaper'
              }
              title={
                state?.playback.status === 'stopped'
                  ? 'Apply a wallpaper from its preview first'
                  : undefined
              }
              disabled={
                unavailable || !['playing', 'paused'].includes(state?.playback.status ?? '') || !active
              }
              onClick={() =>
                void playback(
                  state?.playback.status === 'playing' ? 'pause' : 'resume',
                )
              }
            >
              {state?.playback.status === 'playing' ? (
                <Pause size={18} fill="currentColor" />
              ) : (
                <Play size={18} fill="currentColor" />
              )}
            </button>
            <button
              className="icon-button"
              aria-label={state?.playback.status === 'pending' || (state?.playback.status === 'stopped' && active) ? 'Cancel scheduled wallpaper' : 'Stop wallpaper'}
              disabled={unavailable || (!active && state?.playback.status === 'stopped')}
              onClick={() => void playback('stop')}
            >
              <Square size={16} />
            </button>
          </div>
          <div className="footer-volume">
            {state && (
              <Volume
                settings={state.settings}
                disabled={unavailable}
                updateSettings={updateSettings}
              />
            )}
          </div>
          <button
            className="footer-monitor text-button"
            onClick={() => setPage('Displays')}
          >
            <Monitor size={16} />
            <span>
              {state?.monitors.find((m) => m.id === state.settings.monitor)
                ?.name || 'Displays'}
            </span>
          </button>
        </footer>
      </div>
      {notice && (
        <div className="toast" role="status">
          <Check size={17} />
          <span>{notice}</span>
          <button
            className="icon-button"
            aria-label="Dismiss notification"
            onClick={() => setNotice('')}
          >
            <X size={15} />
          </button>
        </div>
      )}
      {importOpen && (
        <ImportModal
          disabled={busy}
          error={actionError}
          close={() => {
            setImportOpen(false)
            setActionError('')
          }}
          onImport={async (path) => {
            const next = await mutate(
              '/api/import',
              'POST',
              { path },
              'Wallpaper imported into your library.',
            )
            if (next) {
              const added = next.wallpapers.find(
                (w) => !state?.wallpapers.some((old) => old.id === w.id),
              )
              if (added) setSelectedId(added.id)
              setImportOpen(false)
              setPage('Library')
              setQuery('')
              setKind('all')
            }
          }}
          steamPaths={state?.steam_paths ?? []}
        />
      )}
      {widgetImportOpen && (
        <WidgetImportModal
          disabled={busy}
          error={actionError}
          close={() => {
            setWidgetImportOpen(false)
            setActionError('')
          }}
          onImport={async (root, ini) => {
            if (await mutate('/api/widgets/import', 'POST', { root, ini }, 'Skin imported. Enable it to request display.'))
              setWidgetImportOpen(false)
          }}
        />
      )}
      {removeWidget && (
        <Modal
          title="Remove skin?"
          close={() => {
            if (!busy) {
              setRemoveWidget(null)
              setActionError('')
            }
          }}
        >
          <p><strong>{removeWidget.ini}</strong> will be removed from Wall-in. The original skin files will not be deleted.</p>
          {actionError && <p className="form-error" role="alert">{actionError}</p>}
          <div className="modal-actions">
            <button className="button secondary" disabled={busy} onClick={() => setRemoveWidget(null)}>Keep skin</button>
            <button className="button danger" disabled={busy} onClick={async () => {
              if (await mutate(`/api/widgets/${encodeURIComponent(removeWidget.id)}`, 'DELETE', undefined, 'Skin removed. Your source files are unchanged.'))
                setRemoveWidget(null)
            }}><Trash2 size={16} />Remove skin</button>
          </div>
        </Modal>
      )}
      {remove && (
        <Modal
          title="Remove from your library?"
          close={() => {
            if (!busy) {
              setRemove(null)
              setActionError('')
            }
          }}
        >
          <p>
            <strong>{remove.title}</strong> will be removed from Wall-in and any
            saved playlist. The original files on your computer will not be
            deleted.
          </p>
          {remove.id === state?.playback.wallpaper_id && (
            <p className="warning-text">
              This wallpaper is currently active. Removing it will stop
              playback.
            </p>
          )}
          {actionError && (
            <p className="form-error" role="alert">
              {actionError}
            </p>
          )}
          <div className="modal-actions">
            <button
              className="button secondary"
              disabled={busy}
              onClick={() => setRemove(null)}
            >
              Keep wallpaper
            </button>
            <button
              className="button danger"
              disabled={busy}
              onClick={async () => {
                if (
                  await mutate(
                    `/api/wallpapers/${encodeURIComponent(remove.id)}`,
                    'DELETE',
                    undefined,
                    'Wallpaper removed. Your source files are unchanged.',
                  )
                ) {
                  setRemove(null)
                  setSelectedId(null)
                }
              }}
            >
              {busy ? (
                <LoaderCircle size={16} className="spin" />
              ) : (
                <Trash2 size={16} />
              )}
              Remove wallpaper
            </button>
          </div>
        </Modal>
      )}
    </div>
  )
}

function Media({
  wallpaper,
  thumbnail = false,
}: {
  wallpaper: Wallpaper
  thumbnail?: boolean
}) {
  const [failed, setFailed] = useState(false)
  useEffect(
    () => setFailed(false),
    [wallpaper.id, wallpaper.media_url, wallpaper.preview_url],
  )
  const imageUrl =
    wallpaper.preview_url ||
    (wallpaper.kind === 'image' ? wallpaper.media_url : null)
  if (
    !thumbnail &&
    wallpaper.kind === 'video' &&
    wallpaper.media_url &&
    !failed
  )
    return (
      <video
        key={wallpaper.media_url}
        src={wallpaper.media_url}
        poster={wallpaper.preview_url ?? undefined}
        controls
        muted
        playsInline
        preload="metadata"
        onError={() => setFailed(true)}
        aria-label={`Video preview of ${wallpaper.title}`}
      />
    )
  if (imageUrl && !failed)
    return (
      <img
        src={imageUrl}
        alt={thumbnail ? '' : wallpaper.title}
        loading={thumbnail ? 'lazy' : 'eager'}
        onError={() => setFailed(true)}
      />
    )
  return (
    <div className="media-placeholder">
      <WallpaperIcon size={thumbnail ? 28 : 38} strokeWidth={1.2} />
      <span>{failed ? 'Preview unavailable' : 'No preview available'}</span>
      {!thumbnail && wallpaper.kind === 'scene' && wallpaper.supported && (
        <small>Native 3D scenes render on the desktop; this browser view has no scene preview.</small>
      )}
      {failed && wallpaper.kind === 'video' && !thumbnail && (
        <small>
          Your browser may not support this video codec. Desktop playback uses
          GStreamer and may still work; an H.264 MP4 is recommended for browser
          previews.
        </small>
      )}
    </div>
  )
}

function Detail({
  wallpaper: w,
  state,
  disabled,
  onClose,
  onRemove,
  apply,
  schedule,
  updateSettings,
}: {
  wallpaper: Wallpaper
  state: AppState
  disabled: boolean
  onClose: () => void
  onRemove: () => void
  apply: () => void
  schedule: () => void
  updateSettings: (s: Partial<Settings>) => unknown
}) {
  return (
    <aside className="detail-panel" aria-label="Wallpaper details">
      <div className="panel-heading">
        <h2>Wallpaper details</h2>
        <button
          className="icon-button"
          aria-label="Close details"
          onClick={onClose}
        >
          <X size={17} />
        </button>
      </div>
      <div className="detail-preview">
        <Media wallpaper={w} />
      </div>
      <div className="detail-body">
        <span className="detail-kind">{wallpaperKindName(w)}</span>
        <h2>{w.title}</h2>
        <p>{w.description || 'Imported from your computer.'}</p>
        {w.tags.length > 0 && (
          <div className="tags">
            {w.tags.map((tag) => (
              <span key={tag}>{tag}</span>
            ))}
          </div>
        )}
        <label className="field-label">
          Display
          <MonitorSelect
            state={state}
            disabled={disabled}
            updateSettings={updateSettings}
          />
        </label>
        <label className="field-label">
          Wallpaper fit
          <FitSelect
            settings={state.settings}
            disabled={disabled}
            updateSettings={updateSettings}
          />
        </label>
        <p className="field-help">
          Display and fit are used when you apply a wallpaper.
        </p>
        <button
          className="button primary full-width"
          disabled={disabled || !w.supported || !state.platform.available}
          onClick={apply}
        >
          <Play size={16} />
          Apply wallpaper
        </button>
        {!state.platform.available && w.supported && state.settings.autostart && (
          <button
            className="button secondary full-width"
            disabled={disabled || state.playback.status === 'playing' || state.playback.status === 'paused'}
            onClick={schedule}
          >
            Use after next login
          </button>
        )}
        {(!w.supported || !state.platform.available) && (
          <p className="field-help">
            {!w.supported
              ? `${wallpaperKindName(w)} rendering is not available for this project.`
              : state.platform.message ||
                'A compatible desktop integration is required.'}
          </p>
        )}
        <details className="source-details">
          <summary>Source file</summary>
          <code>{w.path}</code>
        </details>
        <button
          className="text-button remove-button"
          disabled={disabled}
          onClick={onRemove}
        >
          <Trash2 size={15} />
          Remove from library
        </button>
      </div>
    </aside>
  )
}

function MonitorSelect({
  state,
  disabled,
  updateSettings,
}: {
  state: AppState
  disabled: boolean
  updateSettings: (s: Partial<Settings>) => unknown
}) {
  const known = state.monitors.some((m) => m.id === state.settings.monitor)
  return (
    <select
      aria-label="Target display"
      value={state.settings.monitor}
      disabled={disabled}
      onChange={(e) => updateSettings({ monitor: e.target.value })}
    >
      {!known && (
        <option value={state.settings.monitor}>
          {state.settings.monitor
            ? `Saved display: ${state.settings.monitor}`
            : 'Default display'}
        </option>
      )}
      {state.monitors.map((m) => (
        <option key={m.id} value={m.id}>
          {m.name}
          {m.width && m.height ? ` · ${m.width} × ${m.height}` : ''}
        </option>
      ))}
    </select>
  )
}
function FitSelect({
  settings,
  disabled,
  updateSettings,
}: {
  settings: Settings
  disabled: boolean
  updateSettings: (s: Partial<Settings>) => unknown
}) {
  return (
    <select
      aria-label="Wallpaper fit"
      value={settings.fit}
      disabled={disabled}
      onChange={(e) =>
        updateSettings({ fit: e.target.value as Settings['fit'] })
      }
    >
      <option value="cover">Cover · crop to fill</option>
      <option value="contain">Contain · show all</option>
      <option value="fill">Stretch · fill display</option>
    </select>
  )
}
function Volume({
  settings,
  disabled,
  updateSettings,
}: {
  settings: Settings
  disabled: boolean
  updateSettings: (s: Partial<Settings>) => unknown
}) {
  const [volume, setVolume] = useState(settings.volume)
  useEffect(() => setVolume(settings.volume), [settings.volume, disabled])
  return (
    <div className="volume-control">
      <button
        className="icon-button"
        aria-label={settings.muted ? 'Unmute wallpaper' : 'Mute wallpaper'}
        aria-pressed={settings.muted}
        disabled={disabled}
        onClick={() => updateSettings({ muted: !settings.muted })}
      >
        {settings.muted ? <VolumeX size={18} /> : <Volume2 size={18} />}
      </button>
      <input
        aria-label="Wallpaper volume"
        type="range"
        min="0"
        max="100"
        step="1"
        value={volume}
        disabled={disabled}
        onChange={(e) => setVolume(Number(e.target.value))}
        onPointerUp={() => {
          if (volume !== settings.volume) updateSettings({ volume })
        }}
        onKeyUp={(e) => {
          if (
            [
              'ArrowLeft',
              'ArrowRight',
              'ArrowUp',
              'ArrowDown',
              'Home',
              'End',
              'PageUp',
              'PageDown',
            ].includes(e.key) &&
            volume !== settings.volume
          )
            updateSettings({ volume })
        }}
        onBlur={() => {
          if (volume !== settings.volume && !disabled)
            updateSettings({ volume })
        }}
      />
      <output>{volume}%</output>
    </div>
  )
}
function WidgetsPage({
  state,
  disabled,
  mutate,
  onImport,
  onRemove,
}: {
  state: AppState
  disabled: boolean
  mutate: Mutation
  onImport: () => void
  onRemove: (widget: Widget) => void
}) {
  return (
    <div className="settings-page widgets-page">
      <div className="info-note">
        <Layers3 size={20} />
        <div>
          <strong>Text and live clock meters</strong>
          <p>Wall-in inspects local Rainmeter INI files without running skin scripts. Supported Time measures update each second with a short fade. Image, Shape, other measures and effects are not rendered. Importing a skin does not enable it.</p>
        </div>
      </div>
      {state.widget_error && (
        <div className="alert error" role="alert">
          <CircleHelp size={18} />
          <div>
            <strong>Widget display problem</strong>
            <p>{state.widget_error}</p>
            {state.widget_error === 'GNOME widget bridge is not supported by this extension' && (
              <p>The updated extension is installed, but this GNOME session is still running its older code. Save your work, sign out, then sign back in before enabling a skin. Restarting Wall-in alone will not reload GNOME Shell.</p>
            )}
          </div>
        </div>
      )}
      <div className="collection-heading">
        <span>IMPORTED SKINS</span>
        <span>{state.widgets.length} {state.widgets.length === 1 ? 'skin' : 'skins'}</span>
      </div>
      {!state.widgets.length ? (
        <div className="inline-empty">
          <Layers3 size={30} />
          <h3>No skins imported</h3>
          <p>Import a local skin folder and a relative INI file to inspect its supported Text meters.</p>
          <button className="button primary" disabled={disabled} onClick={onImport}><Plus size={16} />Import skin</button>
        </div>
      ) : (
        <div className="widget-list">
          {state.widgets.map((widget) => (
            <WidgetRow
              key={widget.id}
              widget={widget}
              state={state}
              disabled={disabled}
              mutate={mutate}
              onRemove={() => onRemove(widget)}
            />
          ))}
        </div>
      )}
    </div>
  )
}

function WidgetRow({ widget, state, disabled, mutate, onRemove }: {
  widget: Widget
  state: AppState
  disabled: boolean
  mutate: Mutation
  onRemove: () => void
}) {
  const [x, setX] = useState(String(widget.x))
  const [y, setY] = useState(String(widget.y))
  useEffect(() => { setX(String(widget.x)); setY(String(widget.y)) }, [widget.x, widget.y])
  const position = [x, y].map(Number)
  const valid = [x, y].every((value, index) => value.trim() !== '' && Number.isInteger(position[index]) && position[index] >= -2147483648 && position[index] <= 2147483647)
  const path = `/api/widgets/${encodeURIComponent(widget.id)}`
  const knownMonitor = state.monitors.some((monitor) => monitor.id === widget.monitor)
  const clocks = widget.meters.filter((meter) => meter.clock_format !== undefined).length
  return (
    <article className="widget-card" aria-label={`Skin ${widget.ini}`}>
      <div className="widget-heading">
        <div>
          <h2>{widget.ini}</h2>
          <span className="support-status">{widget.enabled ? 'Enabled in Wall-in' : 'Disabled'}</span>
        </div>
        <button className="toggle" role="switch" aria-label={`Enable ${widget.ini}`} aria-checked={widget.enabled} disabled={disabled} onClick={() => void mutate(path, 'PATCH', { enabled: !widget.enabled })}><span /></button>
      </div>
      <p className="widget-source">Skin root: <code>{widget.root}</code></p>
      <p className="widget-source">{widget.meters.length} supported Text {widget.meters.length === 1 ? 'meter' : 'meters'} ({clocks} live {clocks === 1 ? 'clock' : 'clocks'}). Enabling requests display on the selected monitor; it does not guarantee the widget is visible.</p>
      {widget.enabled && state.widget_error && <p className="warning-text">Display is not confirmed. See the widget error above.</p>}
      <div className="widget-fields">
        <label>Monitor
          <select value={widget.monitor} disabled={disabled || !state.monitors.length} onChange={(e) => void mutate(path, 'PATCH', { monitor: e.target.value })}>
            {!knownMonitor && <option value={widget.monitor}>{widget.monitor ? `Saved (disconnected): ${widget.monitor}` : 'No monitor selected'}</option>}
            {state.monitors.map((monitor) => <option key={monitor.id} value={monitor.id}>{monitor.name} ({monitor.id})</option>)}
          </select>
        </label>
        <form className="widget-position" onSubmit={(e) => {
          e.preventDefault()
          if (valid) void mutate(path, 'PATCH', { x: position[0], y: position[1] })
        }}>
          <label>X offset<input type="number" step="1" min="-2147483648" max="2147483647" value={x} disabled={disabled} onChange={(e) => setX(e.target.value)} /></label>
          <label>Y offset<input type="number" step="1" min="-2147483648" max="2147483647" value={y} disabled={disabled} onChange={(e) => setY(e.target.value)} /></label>
          <button className="button small secondary" type="submit" disabled={disabled || !valid || (position[0] === widget.x && position[1] === widget.y)}>Save position</button>
        </form>
      </div>
      <p className="field-help">Offsets are in pixels from the monitor’s top-left. Enabled Text meters must fit entirely on the selected display.</p>
      {widget.diagnostics.length > 0 && (
        <div className="widget-diagnostics">
          <h3>Inspection diagnostics ({widget.diagnostics.length})</h3>
          <ul>{widget.diagnostics.map((diagnostic, index) => <li key={`${diagnostic.section}-${diagnostic.code}-${index}`}><strong>{diagnostic.section}</strong> ({diagnostic.code}): {diagnostic.message}</li>)}</ul>
        </div>
      )}
      <button className="text-button remove-button" disabled={disabled} onClick={onRemove}><Trash2 size={15} />Remove skin</button>
    </article>
  )
}

function WidgetImportModal({ disabled, error, close, onImport }: {
  disabled: boolean
  error: string
  close: () => void
  onImport: (root: string, ini: string) => Promise<void>
}) {
  const [root, setRoot] = useState('')
  const [ini, setIni] = useState('')
  const [validation, setValidation] = useState('')
  const submit = (e: FormEvent) => {
    e.preventDefault()
    const cleanRoot = root.trim(), cleanIni = ini.trim()
    if (!cleanRoot.startsWith('/') || cleanRoot.includes('\0')) {
      setValidation('Enter an absolute Linux skin-folder path beginning with /.')
      return
    }
    if (!cleanIni || cleanIni.startsWith('/') || cleanIni.split(/[\\/]/).some((part) => part === '..' || part === '.') || !cleanIni.toLowerCase().endsWith('.ini')) {
      setValidation('Enter a relative .ini path inside the skin folder (no . or .. segments).')
      return
    }
    setValidation('')
    void onImport(cleanRoot, cleanIni)
  }
  return (
    <Modal title="Import a skin" close={() => { if (!disabled) close() }}>
      <p>Point to a local skin folder and its INI file. The backend inspects the file without executing the skin. Supported static Text and Time-measure clocks are imported disabled.</p>
      <form onSubmit={submit}>
        <label className="field-label" htmlFor="widget-root">Absolute skin-folder path</label>
        <div className="path-input"><FolderOpen size={18} /><input id="widget-root" autoComplete="off" spellCheck={false} placeholder="/home/you/Skins/Clock" value={root} disabled={disabled} aria-invalid={Boolean(validation || error)} onChange={(e) => { setRoot(e.target.value); setValidation('') }} /></div>
        <label className="field-label" htmlFor="widget-ini">INI file relative to skin folder</label>
        <div className="path-input"><input id="widget-ini" autoComplete="off" spellCheck={false} placeholder="Clock.ini" value={ini} disabled={disabled} aria-invalid={Boolean(validation || error)} onChange={(e) => { setIni(e.target.value); setValidation('') }} /></div>
        <p className="field-help">Use paths on the machine running the Wall-in service. Browsers cannot provide absolute paths through a file picker.</p>
        {(validation || error) && <p className="form-error" role="alert">{validation || error}</p>}
        <div className="modal-actions">
          <button type="button" className="button secondary" disabled={disabled} onClick={close}>Cancel</button>
          <button type="submit" className="button primary" disabled={disabled || !root.trim() || !ini.trim()}>{disabled ? 'Importing…' : 'Import skin'}</button>
        </div>
      </form>
    </Modal>
  )
}

function Displays({
  state,
  disabled,
  updateSettings,
}: {
  state: AppState
  disabled: boolean
  updateSettings: (s: Partial<Settings>) => unknown
}) {
  return (
    <div className="settings-page">
      <div className="section-intro">
        <h2>Connected displays</h2>
        <p>
          Choose where the next wallpaper is applied. Display detection comes
          from your local desktop session.
        </p>
      </div>
      {state.monitors.length ? (
        <div className="display-grid">
          {state.monitors.map((m) => (
            <button
              key={m.id}
              className={`display-card ${state.settings.monitor === m.id ? 'chosen' : ''}`}
              aria-pressed={state.settings.monitor === m.id}
              disabled={disabled}
              onClick={() => updateSettings({ monitor: m.id })}
            >
              <Monitor size={70} strokeWidth={1} />
              <strong>{m.name}</strong>
              <span>
                {m.width && m.height
                  ? `${m.width} × ${m.height}`
                  : 'Resolution not reported'}
              </span>
              <small>
                {state.settings.monitor === m.id
                  ? 'Selected display'
                  : 'Select display'}
              </small>
            </button>
          ))}
        </div>
      ) : (
        <div className="inline-empty">
          <Monitor size={28} />
          <h3>No displays reported</h3>
          <p>
            The backend could not detect an output. Check desktop integration
            before applying a wallpaper.
          </p>
        </div>
      )}
      <div className="settings-section">
        <h2>Placement</h2>
        <div className="setting-row">
          <div>
            <h3>Wallpaper fit</h3>
            <p>
              Saved for the next apply. Renderer behavior can vary by desktop.
            </p>
          </div>
          <FitSelect
            settings={state.settings}
            disabled={disabled}
            updateSettings={updateSettings}
          />
        </div>
      </div>
      <div className="info-note">
        <CircleHelp size={18} />
        <p>
          {state.platform.message ||
            'Wallpaper placement depends on the desktop integration.'}{' '}
          A saved display preference does not guarantee per-monitor placement on
          every desktop.
        </p>
      </div>
    </div>
  )
}
function SettingsPage({
  state,
  disabled,
  updateSettings,
}: {
  state: AppState
  disabled: boolean
  updateSettings: (s: Partial<Settings>) => unknown
}) {
  return (
    <div className="settings-page">
      <div className="settings-section">
        <h2>Playback</h2>
        <div className="setting-row">
          <div>
            <h3>Wallpaper audio</h3>
            <p>
              Volume and mute for desktop video playback. Preview audio is
              separate.
            </p>
          </div>
          <Volume
            settings={state.settings}
            disabled={disabled}
            updateSettings={updateSettings}
          />
        </div>
        <div className="setting-row">
          <div>
            <h3>Default display</h3>
            <p>Used the next time you apply a wallpaper.</p>
          </div>
          <MonitorSelect
            state={state}
            disabled={disabled}
            updateSettings={updateSettings}
          />
        </div>
        <div className="setting-row">
          <div>
            <h3>Wallpaper fit</h3>
            <p>Save how the next wallpaper fills the display.</p>
          </div>
          <FitSelect
            settings={state.settings}
            disabled={disabled}
            updateSettings={updateSettings}
          />
        </div>
      </div>
      <div className="settings-section">
        <h2>Automation</h2>
        <p className="section-description">
          Playback policies are evaluated by the native renderer. Automatic
          startup restores the last successfully applied wallpaper after login.
        </p>
        {([
          {
            key: 'pause_on_battery',
            title: 'Pause on battery',
            copy: 'Pause video when the system reports battery power.',
            available: true,
          },
          {
            key: 'pause_on_fullscreen',
            title: 'Pause for fullscreen apps',
            copy: 'Pause when GNOME reports a fullscreen window on the chosen display.',
            available: state.platform.backend === 'gnome-shell',
          },
          {
            key: 'autostart',
            title: 'Start with your desktop',
            copy: 'Install or remove a user-level startup service. A first-time GNOME extension install may require logging in again.',
            available: true,
          },
        ] as const).map((item) => (
          <div className="setting-row" key={item.key}>
            <div>
              <h3>
                {item.title}
                {!item.available && <span className="unavailable-label">Unavailable</span>}
              </h3>
              <p>{item.copy}</p>
            </div>
            <button
              className="toggle"
              role="switch"
              aria-checked={state.settings[item.key]}
              aria-label={item.title}
              disabled={disabled || !item.available}
              onClick={() => updateSettings({ [item.key]: !state.settings[item.key] })}
            >
              <span />
            </button>
          </div>
        ))}
      </div>
      <div className="settings-section">
        <h2>Steam project locations</h2>
        <p className="section-description">
          Locations reported by the backend. Scan Steam checks local Workshop
          projects; it does not download or subscribe to items.
        </p>
        {state.steam_paths.length ? (
          <ul className="path-list">
            {state.steam_paths.map((path) => (
              <li key={path}>
                <FolderOpen size={16} />
                <code>{path}</code>
              </li>
            ))}
          </ul>
        ) : (
          <p className="muted">
            No Steam library paths were detected. Import a project folder
            directly instead.
          </p>
        )}
      </div>
    </div>
  )
}
function Playlists({
  state,
  disabled,
  updateSettings,
}: {
  state: AppState
  disabled: boolean
  updateSettings: (s: Partial<Settings>) => unknown
}) {
  const ids = state.settings.playlist_ids
  return (
    <div className="settings-page">
      <div className="info-note">
        <ListMusic size={20} />
        <div>
          <strong>Rotate the wallpapers you choose.</strong>
          <p>
            Add supported wallpapers below. When one is playing, Wall-in changes
            to the next item at the selected interval. Stop playback to stop
            rotation.
          </p>
        </div>
      </div>
      <div className="section-intro">
        <h2>Saved queue</h2>
        <p>Choose supported wallpapers, then apply one from the library to start playback.</p>
      </div>
      {state.wallpapers.length ? (
        <div className="queue-list">
          {state.wallpapers.map((w) => (
            <label className="queue-row" key={w.id}>
              <input
                type="checkbox"
                checked={ids.includes(w.id)}
                disabled={disabled || !w.supported}
                onChange={(e) =>
                  updateSettings({
                    playlist_ids: e.target.checked
                      ? [...ids, w.id]
                      : ids.filter((id) => id !== w.id),
                  })
                }
              />
              <span className="queue-thumb">
                <Media wallpaper={w} thumbnail />
              </span>
              <span>
                <strong>{w.title}</strong>
                <small>
                  {wallpaperKindName(w)}
                  {!w.supported ? ' · Renderer unavailable' : ''}
                </small>
              </span>
            </label>
          ))}
        </div>
      ) : (
        <div className="inline-empty">
          <ListMusic size={30} />
          <h3>Your queue starts with your library.</h3>
          <p>Import a wallpaper first, then add it here.</p>
        </div>
      )}
      <div className="settings-section">
        <div className="setting-row">
          <div>
            <h3>Rotation interval</h3>
            <p>Time between wallpaper changes while a queued wallpaper is playing.</p>
          </div>
          <select
            aria-label="Rotation interval"
            value={state.settings.playlist_interval}
            disabled={disabled}
            onChange={(e) => updateSettings({ playlist_interval: Number(e.target.value) })}
          >
            {![60, 300, 900, 3600, 86400].includes(state.settings.playlist_interval) && (
              <option value={state.settings.playlist_interval}>
                {state.settings.playlist_interval} seconds
              </option>
            )}
            <option value={60}>1 minute</option>
            <option value={300}>5 minutes</option>
            <option value={900}>15 minutes</option>
            <option value={3600}>1 hour</option>
            <option value={86400}>1 day</option>
          </select>
        </div>
        <div className="setting-row">
          <div>
            <h3>Shuffle</h3>
            <p>Choose the next queued wallpaper in a different order.</p>
          </div>
          <button
            className="toggle"
            role="switch"
            aria-label="Shuffle playlist"
            aria-checked={state.settings.shuffle}
            disabled={disabled}
            onClick={() => updateSettings({ shuffle: !state.settings.shuffle })}
          >
            <span />
          </button>
        </div>
      </div>
    </div>
  )
}
function Compatibility({ state }: { state: AppState }) {
  return (
    <div className="settings-page">
      <section className="platform-report">
        <div className="platform-title">
          <Monitor size={24} />
          <h2>Your desktop environment</h2>
          <span
            className={`support-status ${state.platform.available ? 'supported' : ''}`}
          >
            {state.platform.available
              ? 'Integration detected'
              : 'Integration unavailable'}
          </span>
        </div>
        <dl>
          <div>
            <dt>Desktop</dt>
            <dd>{state.platform.desktop || 'Not detected'}</dd>
          </div>
          <div>
            <dt>Session</dt>
            <dd>{state.platform.session || 'Not detected'}</dd>
          </div>
          <div>
            <dt>Backend</dt>
            <dd>{state.platform.backend || 'Not detected'}</dd>
          </div>
        </dl>
        <p>{state.platform.message}</p>
      </section>
      <div className="section-intro">
        <h2>Wallpaper formats</h2>
        <p>
          Importing a project is not the same as being able to render it. Here
          is what Wall-in currently supports.
        </p>
      </div>
      <div className="compatibility-list">
        {[
          {
            name: 'Images',
            icon: Image,
            supported: true,
            text: 'Local image wallpapers. Applying requires a compatible desktop integration.',
          },
          {
            name: 'Videos',
            icon: Clapperboard,
            supported: true,
            text: 'Local video loops and video-based Steam projects. Playback requires a compatible renderer, desktop integration, and media codecs.',
          },
          {
            name: 'Web projects',
            icon: LayoutGrid,
            supported: false,
            text: 'HTML and JavaScript wallpaper rendering is not implemented. Imported projects can be listed, but not applied.',
          },
          {
            name: 'Wall-in native 3D scenes',
            icon: Layers3,
            supported: true,
            text: 'Only the built-in versioned cube scene format (.wallin-scene.json) is supported. It animates a depth-tested 3D cube without project scripts or assets.',
          },
          {
            name: 'Wallpaper Engine scene projects',
            icon: Layers3,
            supported: false,
            text: 'Wallpaper Engine .pkg scenes, materials, particles and effects are not rendered.',
          },
          {
            name: 'Applications',
            icon: Square,
            supported: false,
            text: 'Executable wallpaper projects are not implemented and are not launched.',
          },
        ].map((item) => (
          <div className="compatibility-row" key={item.name}>
            <item.icon size={21} />
            <div>
              <h3>{item.name}</h3>
              <p>{item.text}</p>
            </div>
            <span
              className={`support-status ${item.supported ? 'supported' : ''}`}
            >
              {item.supported ? 'Conditional support' : 'Not implemented'}
            </span>
          </div>
        ))}
      </div>
      <div className="info-note">
        <ShieldCheck size={20} />
        <p>
          Linux desktops do not share one wallpaper protocol. GNOME, KDE Plasma,
          X11 desktops, and Wayland compositors can require different helpers.
          The live environment report above is the authority for this session,
          not a promise of universal support.
        </p>
      </div>
    </div>
  )
}
function Modal({
  title,
  children,
  close,
}: {
  title: string
  children: ReactNode
  close: () => void
}) {
  const ref = useRef<HTMLDivElement>(null)
  const closeRef = useRef(close)
  closeRef.current = close
  useEffect(() => {
    const previous = document.activeElement as HTMLElement | null
    const overflow = document.body.style.overflow
    document.body.style.overflow = 'hidden'
    const dialog = ref.current!
    const getFocusable = () =>
      Array.from(
        dialog.querySelectorAll<HTMLElement>(
          'button:not(:disabled), input:not(:disabled), select:not(:disabled), [tabindex="0"]',
        ),
      )
    ;(
      dialog.querySelector<HTMLElement>('input') ||
      getFocusable()[0] ||
      dialog
    ).focus()
    const keydown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault()
        closeRef.current()
      }
      if (e.key === 'Tab') {
        const elements = getFocusable()
        const first = elements[0],
          last = elements[elements.length - 1]
        if (!first) {
          e.preventDefault()
          dialog.focus()
        } else if (
          e.shiftKey &&
          (document.activeElement === first ||
            document.activeElement === dialog)
        ) {
          e.preventDefault()
          last.focus()
        } else if (!e.shiftKey && document.activeElement === last) {
          e.preventDefault()
          first.focus()
        }
      }
    }
    document.addEventListener('keydown', keydown)
    return () => {
      document.body.style.overflow = overflow
      document.removeEventListener('keydown', keydown)
      previous?.focus()
    }
  }, [])
  return (
    <div
      className="modal-backdrop"
      onClick={(e) => {
        if (e.target === e.currentTarget) close()
      }}
    >
      <div
        className="modal"
        role="dialog"
        aria-modal="true"
        aria-labelledby="modal-title"
        tabIndex={-1}
        ref={ref}
      >
        <div className="modal-heading">
          <h2 id="modal-title">{title}</h2>
          <button
            className="icon-button"
            aria-label="Close dialog"
            onClick={close}
          >
            <X size={20} />
          </button>
        </div>
        {children}
      </div>
    </div>
  )
}
function ImportModal({
  disabled,
  error,
  close,
  onImport,
  steamPaths,
}: {
  disabled: boolean
  error: string
  close: () => void
  onImport: (path: string) => Promise<void>
  steamPaths: string[]
}) {
  const [path, setPath] = useState('')
  const [validation, setValidation] = useState('')
  const submit = (e: FormEvent) => {
    e.preventDefault()
    const clean = path.trim()
    if (!clean.startsWith('/')) {
      setValidation(
        'Enter an absolute Linux path beginning with /, not a relative path or ~/ shortcut.',
      )
      return
    }
    setValidation('')
    void onImport(clean)
  }
  return (
    <Modal
      title="Import a wallpaper"
      close={() => {
        if (!disabled) close()
      }}
    >
      <p>
        Add an image, video, or a local Steam project folder to your library.
        Your files stay on this computer.
      </p>
      <form onSubmit={submit}>
        <label className="field-label" htmlFor="import-path">
          Absolute file or project-folder path
        </label>
        <div className="path-input">
          <FolderOpen size={18} />
          <input
            id="import-path"
            autoComplete="off"
            spellCheck={false}
            placeholder="/home/you/Pictures/wallpaper.mp4"
            value={path}
            disabled={disabled}
            aria-invalid={Boolean(validation || error)}
            aria-describedby="path-help path-error"
            onChange={(e) => {
              setPath(e.target.value)
              setValidation('')
            }}
          />
        </div>
        <p id="path-help" className="field-help">
          Copy the full path from your file manager. Browsers cannot provide the
          original absolute path from a file picker.
        </p>
        {(validation || error) && (
          <p id="path-error" className="form-error" role="alert">
            {validation || error}
          </p>
        )}
        <div className="import-note">
          <strong>Importing from Steam?</strong>
          <p>
            Use the downloaded project folder containing{' '}
            <code>project.json</code>. Wallpaper Engine web, scene, and application projects can
            be cataloged, but their renderers are not implemented. Wall-in's separate native
            <code>.wallin-scene.json</code> format supports one animated 3D cube.
          </p>
          {steamPaths.length > 0 && (
            <details>
              <summary>Detected Steam locations</summary>
              {steamPaths.map((p) => (
                <code className="steam-path" key={p}>
                  {p}
                </code>
              ))}
            </details>
          )}
        </div>
        <div className="modal-actions">
          <button
            type="button"
            className="button secondary"
            disabled={disabled}
            onClick={close}
          >
            Cancel
          </button>
          <button
            type="submit"
            className="button primary"
            disabled={disabled || !path.trim()}
          >
            {disabled ? (
              <LoaderCircle size={16} className="spin" />
            ) : (
              <Plus size={16} />
            )}
            {disabled ? 'Importing…' : 'Import wallpaper'}
          </button>
        </div>
      </form>
    </Modal>
  )
}
