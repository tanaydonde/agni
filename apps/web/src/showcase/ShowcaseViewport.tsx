import { type ReactNode, type RefObject, useEffect, useMemo, useRef, useState } from 'react'
import { ControlsHint, type Controls } from '../components/ControlsHint'
import { burningObjectIds, fireCellBoxes } from '../fire/cells'
import { useFireFrame, useSimulationAgents } from '../fire/fireStore'
import { isShownAgent } from '../fire/frames'
import { FlyIndicator } from '../components/FlyIndicator'
import { wheelPixels } from '../scene/cameraMath'
import type { ModelPlacement } from '../scene/modelChoice'
import type { AgentFrame, SimulationFrame } from '../fire/types'
import type { Floor, Scene } from '../scene/types'
import { type CameraDriver, createHopDriver } from './cameraDriver'
import { activateDefurnishedView, showDollhouse, showSpaceVersion, type SpaceVersion } from './defurnish'
import { createFlyDriver } from './flyDriver'
import { mountAgents, type ShowcaseAgents } from './agents'
import { mountFire, type ShowcaseFire } from './fire'
import { furnitureItems, mountFurniture, type ShowcaseFurniture } from './furniture'
import { mountEnvironment } from './environment'
import { mountModels, type ShowcaseModels } from './models'
import {
  connectShowcase,
  hideShowcaseControls,
  MODE,
  type MpSdk,
  showcaseUrl,
  type Subscription,
  untilPlaying,
} from './sdk'

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
  /** No driver in the bird's-eye view, where Showcase keeps its own controls. */
  | { kind: 'ready'; driver: CameraDriver | null }
  | { kind: 'error'; message: string }

interface ShowcaseViewportProps {
  scene: Scene
  floor: Floor
  /**
   * Models standing in for scanned furniture; empty to hide them. Scanned
   * furniture without a model of its kind isn't drawn here (the editor shows
   * every detected box).
   */
  models: ModelPlacement[]
  /** Show the space as scanned, or with its furniture removed. */
  version: SpaceVersion
  /**
   * The simulation to draw, e.g. one scenario under review. Without it the
   * store's current fire run is drawn.
   */
  timeline?: { frame: SimulationFrame | undefined; agents: AgentFrame[] }
  /**
   * Show Showcase's dollhouse (the whole model from above, with Showcase's
   * own orbit and zoom) instead of standing inside with our fly controls.
   */
  birdsEye?: boolean
}

/**
 * Matterport's own player (Showcase) showing the space with its photo
 * textures, either defurnished (the empty rooms) or furnished as scanned.
 * Switching keeps the camera where it is, for comparing the two. Our placed
 * furniture and the furniture detected in the scan are drawn into it
 * read-only: scanned furniture as models of its kind, where one exists.
 *
 * WASD flies freely by driving Showcase's camera directly. Showcase's own
 * mouse handling is off then, so a transparent layer over it takes the
 * pointer and works like the editor: right-click captures the pointer to fly
 * with a crosshair, Esc lets it go. If this Showcase won't hand over its
 * camera, WASD hops between scans and Showcase keeps its own mouse controls.
 */
export function ShowcaseViewport({
  scene,
  floor,
  models,
  version,
  timeline,
  birdsEye = false,
}: ShowcaseViewportProps) {
  const container = useRef<HTMLDivElement>(null)
  const inputLayer = useRef<HTMLDivElement>(null)
  const [status, setStatus] = useState<Status>({ kind: 'loading', slow: false })
  const [flying, setFlying] = useState(false)
  const furniture = useRef<ShowcaseFurniture | null>(null)
  const sdkRef = useRef<MpSdk | null>(null)
  const latestVersion = useRef(version)
  const furnitureModels = useRef<ShowcaseModels | null>(null)
  const latestModels = useRef(models)
  const storeFrame = useFireFrame()
  const storeAgents = useSimulationAgents()
  const fireFrame = timeline ? timeline.frame : storeFrame
  const simulationAgents = timeline ? timeline.agents : storeAgents
  const latestBirdsEye = useRef(birdsEye)
  // Burning furniture draws see-through. Keyed by the IDs so the boxes are
  // only rebuilt when that set changes, not on every fire frame.
  const burningKey = useMemo(() => [...burningObjectIds(fireFrame)].sort().join('\n'), [fireFrame])
  const items = useMemo(
    () => furnitureItems(floor, scene.assets, new Set(burningKey ? burningKey.split('\n') : [])),
    [floor, scene.assets, burningKey],
  )
  const latestItems = useRef(items)
  const fireCells = useMemo(() => fireCellBoxes(fireFrame, floor.objects), [fireFrame, floor.objects])
  const fire = useRef<ShowcaseFire | null>(null)
  const latestFireCells = useRef(fireCells)
  const agents = useMemo(
    () =>
      simulationAgents.filter((agent) => isShownAgent(agent, floor.id)),
    [simulationAgents, floor.id],
  )
  const agentOverlay = useRef<ShowcaseAgents | null>(null)
  const latestAgents = useRef(agents)
  const freeFlight = status.kind === 'ready' && Boolean(status.driver?.look)

  // Keep Showcase's furniture in step with world edits.
  useEffect(() => {
    latestItems.current = items
    furniture.current?.update(items)
  }, [items])

  useEffect(() => {
    latestModels.current = models
    furnitureModels.current?.update(models)
  }, [models])

  useEffect(() => {
    latestFireCells.current = fireCells
    fire.current?.update(fireCells)
  }, [fireCells])

  useEffect(() => {
    latestAgents.current = agents
    agentOverlay.current?.update(agents)
  }, [agents])

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
    let environment: { dispose(): void } | null = null
    let dollhouseLock: Subscription | null = null
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
      if (iframe.contentDocument) {
        hideShowcaseControls(iframe.contentDocument, { birdsEye: latestBirdsEye.current })
      }
      await untilPlaying(sdk, signal)
      // Bird's-eye still steps inside first: going to the dollhouse from a scan
      // frames the whole model, while going straight there from Showcase's
      // intro leaves it small and far away.
      const startSweep = await activateDefurnishedView(sdk)
      signal.throwIfAborted()
      if (latestVersion.current !== 'defurnished') await showSpaceVersion(sdk, latestVersion.current)
      signal.throwIfAborted()
      sdkRef.current = sdk
      furniture.current = await mountFurniture(sdk, latestItems.current)
      furnitureModels.current = mountModels(sdk, latestModels.current)
      fire.current = await mountFire(sdk, latestFireCells.current)
      agentOverlay.current = await mountAgents(sdk, latestAgents.current)
      environment = await mountEnvironment(sdk)
      signal.throwIfAborted()
      if (latestBirdsEye.current) {
        await showDollhouse(sdk, signal)
        if (signal.aborted) return
        // Clicks can't reach Showcase here (see the layer below), but if the
        // view leaves the dollhouse anyway, put it back.
        dollhouseLock = sdk.Mode.current.subscribe((mode) => {
          if (mode === MODE.INSIDE || mode === MODE.FLOORPLAN) {
            sdk.Mode.moveTo(MODE.DOLLHOUSE).catch(() => {})
          }
        })
        clearTimeout(slowTimer)
        setStatus({ kind: 'ready', driver: null })
        return
      }
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
      furnitureModels.current?.dispose()
      furnitureModels.current = null
      fire.current?.dispose()
      fire.current = null
      agentOverlay.current?.dispose()
      agentOverlay.current = null
      environment?.dispose()
      dollhouseLock?.cancel()
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
      {/* The bird's-eye view is a fixed picture, like the 3D overview: this
          layer keeps clicks (which would fly inside) and drags off Showcase,
          and lets wheel events reach the review for paging. */}
      {birdsEye && <div aria-hidden className="absolute inset-0" />}
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
      {status.kind === 'ready' && status.driver && (
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
