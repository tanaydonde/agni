import { type ReactNode, useEffect, useMemo, useRef, useState } from 'react'
import { ControlsHint, type Controls } from '../components/ControlsHint'
import type { Floor, Scene } from '../scene/types'
import { type CameraDriver, createHopDriver, HOP_KEYS } from './cameraDriver'
import { activateDefurnishedView } from './defurnish'
import { furnitureItems, mountFurniture, type ShowcaseFurniture } from './furniture'
import { connectShowcase, showcaseUrl, untilPlaying } from './sdk'

/** After this long without the space playing, say so (it may still finish). */
const SLOW_LOAD_MS = 60_000

const CONTROLS: Controls = [
  [['W', 'A', 'S', 'D'], 'Hop to the next scan'],
  [['Q', 'E'], 'Down / up'],
  [['Drag'], 'Look around'],
  [['Click'], 'Walk to a spot'],
  [['F'], 'Back to start'],
]

type Status =
  | { kind: 'loading'; slow: boolean }
  | { kind: 'ready' }
  | { kind: 'error'; message: string }

interface ShowcaseViewportProps {
  scene: Scene
  floor: Floor
}

/**
 * Matterport's own player (Showcase) showing the space's defurnished view:
 * the empty rooms with their photo textures. Our placed furniture is drawn
 * into it read-only. Showcase can only stand the camera at scan positions, so
 * WASD hops between scans.
 */
export function ShowcaseViewport({ scene, floor }: ShowcaseViewportProps) {
  const container = useRef<HTMLDivElement>(null)
  const [status, setStatus] = useState<Status>({ kind: 'loading', slow: false })
  const furniture = useRef<ShowcaseFurniture | null>(null)
  const items = useMemo(() => furnitureItems(floor, scene.assets), [floor, scene.assets])
  const latestItems = useRef(items)

  // Keep Showcase's furniture in step with world edits.
  useEffect(() => {
    latestItems.current = items
    furniture.current?.update(items)
  }, [items])

  useEffect(() => {
    const host = container.current
    if (!host || !__MATTERPORT_SDK_KEY__) return
    const abort = new AbortController()
    const { signal } = abort
    let driver: CameraDriver | null = null
    let detachKeys = () => {}
    setStatus({ kind: 'loading', slow: false })
    const slowTimer = setTimeout(
      () => setStatus((current) => (current.kind === 'loading' ? { kind: 'loading', slow: true } : current)),
      SLOW_LOAD_MS,
    )

    // A fresh player per mount: Showcase can't be reconnected to after a disconnect.
    const iframe = document.createElement('iframe')
    iframe.title = 'Matterport Showcase'
    iframe.allow = 'fullscreen; xr-spatial-tracking'
    iframe.className = 'absolute inset-0 size-full border-0'
    iframe.src = showcaseUrl(__SHOWCASE_BUNDLE_URL__, scene.id, __MATTERPORT_SDK_KEY__)
    host.prepend(iframe)

    const run = async () => {
      const sdk = await connectShowcase(iframe, signal)
      signal.throwIfAborted()
      await untilPlaying(sdk, signal)
      const startSweep = await activateDefurnishedView(sdk)
      signal.throwIfAborted()
      furniture.current = await mountFurniture(sdk, latestItems.current)
      signal.throwIfAborted()
      driver = createHopDriver(sdk, startSweep)
      detachKeys = attachKeys([host, iframe.contentWindow], driver)
      clearTimeout(slowTimer)
      setStatus({ kind: 'ready' })
      iframe.contentWindow?.focus()
    }
    run().catch((error: unknown) => {
      if (signal.aborted) return
      clearTimeout(slowTimer)
      setStatus({ kind: 'error', message: error instanceof Error ? error.message : String(error) })
    })

    return () => {
      abort.abort()
      clearTimeout(slowTimer)
      detachKeys()
      driver?.dispose()
      furniture.current?.dispose()
      furniture.current = null
      iframe.remove()
    }
  }, [scene.id])

  if (!__MATTERPORT_SDK_KEY__) {
    return (
      <Overlay>
        Add <code className="font-mono">MATTERPORT_SDK_KEY</code> (a Showcase SDK key, not the Model API
        token) to the repo-root <code className="font-mono">.env</code> and restart the dev server.
      </Overlay>
    )
  }

  return (
    <div ref={container} className="absolute inset-0">
      {status.kind === 'loading' && (
        <Overlay>
          {status.slow
            ? 'Matterport is still loading the space. If this never finishes, check that MATTERPORT_SDK_KEY allows this domain and look in the browser console.'
            : 'Loading the textured space…'}
        </Overlay>
      )}
      {status.kind === 'error' && <Overlay>{status.message}</Overlay>}
      {status.kind === 'ready' && <ControlsHint title="Walking" controls={CONTROLS} />}
    </div>
  )
}

/**
 * Sends keys to the driver. Showcase's iframe keeps keyboard focus once
 * clicked, so listen there too (it's same-origin), in the capture phase so
 * Showcase's own key handling doesn't also act on the keys we use.
 */
function attachKeys(targets: (EventTarget | null)[], driver: CameraDriver): () => void {
  const onKeyDown = (event: Event) => {
    const { code, metaKey, ctrlKey, altKey } = event as KeyboardEvent
    if (metaKey || ctrlKey || altKey) return // leave browser shortcuts alone
    if (code === 'KeyF') driver.home()
    else if (!driver.keyDown(code)) return
    event.preventDefault()
    event.stopImmediatePropagation()
  }
  const onKeyUp = (event: Event) => {
    const { code } = event as KeyboardEvent
    if (code in HOP_KEYS || code === 'KeyF') {
      driver.keyUp(code)
      event.stopImmediatePropagation()
    }
  }
  // Keys released while focus is elsewhere never send keyup.
  const onBlur = () => Object.keys(HOP_KEYS).forEach((code) => driver.keyUp(code))

  const live = targets.filter((target): target is EventTarget => target !== null)
  for (const target of live) {
    target.addEventListener('keydown', onKeyDown, { capture: true })
    target.addEventListener('keyup', onKeyUp, { capture: true })
    target.addEventListener('blur', onBlur)
  }
  return () => {
    for (const target of live) {
      target.removeEventListener('keydown', onKeyDown, { capture: true })
      target.removeEventListener('keyup', onKeyUp, { capture: true })
      target.removeEventListener('blur', onBlur)
    }
  }
}

function Overlay({ children }: { children: ReactNode }) {
  return (
    <div className="pointer-events-none absolute inset-0 flex items-center justify-center bg-white/85 p-8">
      <p className="max-w-md text-center leading-normal text-muted">{children}</p>
    </div>
  )
}
