# Wall-in desktop design

## Direction

Operate mode, as approved in the task. A graphite media workspace, with imported wallpaper artwork as the centerpiece. The interface stays quiet rather than inventing preview art. Impeccable context ran once; the direction seed was 82bad178. The roll ran degraded without remote challengers; the user's pinned visual direction takes precedence.

## Built system

- Graphite background `#191b1a`, deeper sidebar `#141615`, warm text `#eeeee7`.
- Restrained lime `#d0e795` identifies primary actions, selection, and active playback.
- Locally hosted Manrope variable font; its OFL license ships under `public/fonts/`.
- Lucide outline icons, compact rectangular controls, visible keyboard focus, themed selection and scrollbar.
- Desktop: left navigation, library toolbar and responsive media grid, selected-item detail panel, persistent playback strip.
- Mobile: horizontally scrollable navigation, stacked filters, details below the grid, simplified playback strip. Audio controls remain available in Settings.
- Empty states describe import and Steam scan rather than showing example wallpapers. All counts and environment information come from the API.
- Preview video uses native controls and starts muted. It is distinct from desktop playback.
- One short lateral detail-panel reveal; reduced-motion preference disables it.

## Function and truth

Absolute paths are typed into a focus-trapped modal because browser file selection cannot return an original local absolute path. Removal requires explicit confirmation and promises only library removal. Unavailable rendering and automation are labeled honestly. Saved playlist membership is functional; timed rotation and shuffle controls are disabled.

## Verification boundary

TypeScript, production build, and jsdom interaction tests are run by the frontend worker. Real browser visual inspection and desktop integration verification are delegated to the parent integration pass; this document does not claim they have passed.
