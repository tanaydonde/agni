import { type ReactNode, type RefObject, useEffect, useMemo, useRef, useState } from 'react'
import { ControlsHint, type Controls } from '../components/ControlsHint'
import { FlyIndicator } from '../components/FlyIndicator'
import { wheelPixels } from '../scene/cameraMath'
import type { DetectedObject, Floor, Scene } from '../scene/types'
import { type CameraDriver, createHopDriver } from './cameraDriver'
import { activateDefurnishedView, showSpaceVersion, type SpaceVersion } from './defurnish'
import { createFlyDriver } from './flyDriver'
import { detectedFurnitureItems, furnitureItems, mountFurniture, type ShowcaseFurniture } from './furniture'
import { connectShowcase, hideShowcaseControls, type MpSdk, showcaseUrl, untilPlaying } from './sdk'

/** After this long without the space playing, say so (it may still finish). */
const SLOW_LOAD_MS = 60_000

const WALK_CONTROLS: Controls = [
  [['W', 'A', 'S', 'D'], 'Walk'],
  [['Q', 'E'], 'Down / up'],
  [['Shift'], 'Faster'],
  [['Drag'], 'Look around'],
  [['Swipe'], 'Look around'],
  [['F'], 'Back to start'],
  [['Right-click'], 'Fly'],
]

/** While the pointer is captured, as in the editor's fly mode. */
const FLY_CONTROLS: Controls = [
  [['Move'], 'Look around'],
  [['W', 'A', 'S', 'D'], 'Fly'],
  [['Q', 'E'], 'Down / up'],
  [['Shift'], 'Faster'],
  [['F'], 'Back to start'],
  [['Esc'], 'Stop flying'],
]

const HOP_CONTROLS: Controls = [
  [['W', 'A', 'S', 'D'], 'Hop to the next scan'],
  [['Q', 'E'], 'Down / up'],
  [['Drag'], 'Look around'],
  [['Click'], 'Walk to a spot'],
  [['F'], 'Back to start'],
]

type Status =
  | { kind: 'loading'; slow: boolean }
  | { kind: 'ready'; driver: CameraDriver }
  | { kind: 'error'; message: string }

interface ShowcaseViewportProps {
  scene: Scene
  floor: Floor
  /** Scanned furniture to draw as see-through boxes; empty to hide it. */
  detected: DetectedObject[]
  /** Show the space as scanned, or with its furniture removed. */
  version: SpaceVersion
}

/**
 * Matterport's own player (Showcase) showing the space with its photo
 * textures, either defurnished (the empty rooms) or furnished as scanned.
 * Switching keeps the camera where it is, for comparing the two. Our placed
 * furniture and the furniture detected in the scan are drawn into it
 * read-only.
 *
 * WASD flies freely by driving Showcase's camera directly. Showcase's own
 * mouse handling is off then, so a transparent layer over it takes the
 * pointer and works like the editor: right-click captures the pointer to fly
 * with a crosshair, Esc lets it go. If this Showcase won't hand over its
 * camera, WASD hops between scans and Showcase keeps its own mouse controls.
 */
export function ShowcaseViewport({ scene, floor, detected, version }: ShowcaseViewportProps) {
  const container = useRef<HTMLDivElement>(null)
  const inputLayer = useRef<HTMLDivElement>(null)
  const [status, setStatus] = useState<Status>({ kind: 'loading', slow: false })
  const [flying, setFlying] = useState(false)
  const furniture = useRef<ShowcaseFurniture | null>(null)
  const sdkRef = useRef<MpSdk | null>(null)
  const latestVersion = useRef(version)
  const items = useMemo(
    () => [...detectedFurnitureItems(detected, scene.assets), ...furnitureItems(floor, scene.assets)],
    [detected, floor, scene.assets],
  )
  const latestItems = useRef(items)
  const freeFlight = status.kind === 'ready' && Boolean(status.driver.look)

  // Keep Showcase's furniture in step with world edits.
  useEffect(() => {
    latestItems.current = items
    furniture.current?.update(items)
  }, [items])

  // Swap furnished/defurnished in place once the player is up; the initial
  // version is applied when it loads (below).
  const ready = status.kind === 'ready'
  useEffect(() => {
    latestVersion.current = version
    const sdk = sdkRef.current
    if (!ready || !sdk) return
    showSpaceVersion(sdk, version).catch((error: unknown) =>
      setStatus({ kind: 'error', message: error instanceof Error ? error.message : String(error) }),
    )
  }, [ready, version])

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
      if (iframe.contentDocument) hideShowcaseControls(iframe.contentDocument)
      await untilPlaying(sdk, signal)
      const startSweep = await activateDefurnishedView(sdk)
      signal.throwIfAborted()
      if (latestVersion.current !== 'defurnished') await showSpaceVersion(sdk, latestVersion.current)
      signal.throwIfAborted()
      sdkRef.current = sdk
      furniture.current = await mountFurniture(sdk, latestItems.current)
      signal.throwIfAborted()
      driver = await createDriver(sdk, startSweep)
      if (signal.aborted) return driver.dispose()
      // Hopping leaves the mouse to Showcase, which keeps keyboard focus once
      // clicked, so keys are caught in its window too. Free flight takes
      // input on the layer over it instead (see useFlightInput).
      if (!driver.look) {
        detachKeys = attachKeys([host, iframe.contentWindow], driver)
        iframe.contentWindow?.focus()
      }
      clearTimeout(slowTimer)
      setStatus({ kind: 'ready', driver })
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
      sdkRef.current = null
      iframe.remove()
    }
  }, [scene.id])

  useFlightInput(inputLayer, status.kind === 'ready' ? status.driver : null, setFlying)

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
      {freeFlight && (
        <div
          ref={inputLayer}
          tabIndex={-1}
          aria-label="Textured 3D view. Drag to look, WASD to walk; right-click to fly, Escape to stop."
          className="absolute inset-0 rounded-lg outline-none"
        />
      )}
      {status.kind === 'loading' && (
        <Overlay>
          {status.slow
            ? 'Matterport is still loading the space. If this never finishes, check that MATTERPORT_SDK_KEY allows this domain and look in the browser console.'
            : 'Loading the textured space…'}
        </Overlay>
      )}
      {status.kind === 'error' && <Overlay>{status.message}</Overlay>}
      {freeFlight && flying && <FlyIndicator />}
      {status.kind === 'ready' && (
        <ControlsHint
          title={flying ? 'Flying' : 'Walking'}
          controls={!freeFlight ? HOP_CONTROLS : flying ? FLY_CONTROLS : WALK_CONTROLS}
        />
      )}
    </div>
  )
}

/**
 * Free-flight input on the layer over Showcase, matching the editor's
 * CameraController: right-click (or Control-click) captures the pointer and
 * moving it looks around; Esc or another right-click releases it. Without the
 * capture, drag or a two-finger swipe looks around. WASD works either way,
 * since this view has no editing for the keys to clash with.
 */
function useFlightInput(
  layerRef: RefObject<HTMLDivElement | null>,
  driver: CameraDriver | null,
  onFlyingChange: (flying: boolean) => void,
) {
  useEffect(() => {
    const layer = layerRef.current
    const look = driver?.look?.bind(driver)
    if (!layer || !driver || !look) return
    const controller = new AbortController()
    const { signal } = controller
    const isFlying = () => document.pointerLockElement === layer
    let dragging = false

    layer.addEventListener(
      'pointerdown',
      (event) => {
        layer.focus()
        // Right-click, or Control-click on a Mac, toggles flying.
        if (event.button === 2 || (event.button === 0 && event.ctrlKey)) {
          event.preventDefault()
          if (isFlying()) document.exitPointerLock()
          // Chrome refuses a re-lock for about a second after Esc; the next click works.
          else Promise.resolve(layer.requestPointerLock()).catch(() => {})
          return
        }
        if (isFlying() || event.button !== 0) return
        dragging = true
        layer.setPointerCapture(event.pointerId)
      },
      { signal },
    )
    layer.addEventListener(
      'pointermove',
      (event) => {
        if (isFlying() || (dragging && event.buttons & 1)) look(event.movementX, event.movementY)
      },
      { signal },
    )
    layer.addEventListener(
      'pointerup',
      (event) => {
        dragging = false
        if (layer.hasPointerCapture(event.pointerId)) layer.releasePointerCapture(event.pointerId)
      },
      { signal },
    )
    layer.addEventListener('contextmenu', (event) => event.preventDefault(), { signal })
    layer.addEventListener(
      'wheel',
      (event) => {
        // Swallow wheel/pinch so the page never zooms or swipes back.
        event.preventDefault()
        if (event.ctrlKey) return
        const delta = wheelPixels(event)
        look(delta.x, delta.y)
      },
      { passive: false, signal },
    )
    const detachKeys = attachKeys([layer], driver)
    document.addEventListener(
      'pointerlockchange',
      () => {
        dragging = false
        onFlyingChange(isFlying())
      },
      { signal },
    )

    layer.focus()
    return () => {
      controller.abort()
      detachKeys()
      if (isFlying()) document.exitPointerLock()
      onFlyingChange(false)
    }
  }, [layerRef, driver, onFlyingChange])
}

/** Free flying when Showcase lets us drive its camera, else hopping between scans. */
async function createDriver(sdk: MpSdk, startSweep: string | null): Promise<CameraDriver> {
  try {
    return await createFlyDriver(sdk)
  } catch (error) {
    console.warn('Free flying unavailable; WASD will hop between scans instead.', error)
    return createHopDriver(sdk, startSweep)
  }
}

/**
 * Sends keys to the driver, in the capture phase so Showcase's own key
 * handling doesn't also act on the keys we use.
 */
function attachKeys(targets: (EventTarget | null)[], driver: CameraDriver): () => void {
  const held = new Set<string>()
  const controller = new AbortController()
  const options = { capture: true, signal: controller.signal }

  const onKeyDown = (event: Event) => {
    const { code, metaKey, ctrlKey, altKey } = event as KeyboardEvent
    if (metaKey || ctrlKey || altKey) return // leave browser shortcuts alone
    if (code === 'KeyF') driver.home()
    else if (driver.keyDown(code)) held.add(code)
    else return
    event.preventDefault()
    event.stopImmediatePropagation()
  }
  const onKeyUp = (event: Event) => {
    const { code } = event as KeyboardEvent
    if (!held.delete(code)) return
    driver.keyUp(code)
    event.stopImmediatePropagation()
  }
  // Keys released while focus is elsewhere never send keyup.
  const onBlur = () => {
    held.clear()
    driver.releaseAll()
  }
  for (const target of targets) {
    target?.addEventListener('keydown', onKeyDown, options)
    target?.addEventListener('keyup', onKeyUp, options)
    target?.addEventListener('blur', onBlur, { signal: controller.signal })
  }
  return () => controller.abort()
}

function Overlay({ children }: { children: ReactNode }) {
  return (
    <div className="pointer-events-none absolute inset-0 flex items-center justify-center bg-white/85 p-8">
      <p className="max-w-md text-center leading-normal text-muted">{children}</p>
    </div>
  )
}
