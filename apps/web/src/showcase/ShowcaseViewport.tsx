import { type ReactNode, type RefObject, useEffect, useMemo, useRef, useState } from 'react'
import { ControlsHint, type Controls } from '../components/ControlsHint'
import { burningObjectIds, fireCellBoxes } from '../fire/cells'
import { fireVolume } from '../fire/volume'
import { useFireFrame, useSimulationAgents } from '../fire/fireStore'
import { isShownAgent } from '../fire/frames'
import { hoseStream } from '../scene/agentModels'
import { FlyIndicator } from '../components/FlyIndicator'
import { type CameraPose, wheelPixels } from '../scene/cameraMath'
import type { ModelPlacement } from '../scene/modelChoice'
import type { AgentFrame, SimulationFrame } from '../fire/types'
import type { Floor, Scene, Vec3 } from '../scene/types'
import { chainLoops } from '../scene/geometry'
import { vantagePose } from '../demo/cameras'
import { fovFromProjection, orbitForFov, orbitPosition, orbitYawPitch, type Orbit } from '../scene/orbitCamera'
import { type CameraDriver, createHopDriver } from './cameraDriver'
import { activateDefurnishedView, showDollhouse, showSpaceVersion, type SpaceVersion } from './defurnish'
import { createFlyDriver } from './flyDriver'
import { createPoseDriver, type PoseDriver } from './poseDriver'
import { mountAgents, type ShowcaseAgents } from './agents'
import { mountFire, type ShowcaseFire } from './fire'
import { mountWater, type ShowcaseWater } from './water'
import { mountRoute, type ShowcaseRoute } from './route'
import type { RouteRibbon } from '../demo/route'
import { furnitureItems, mountFurniture, type ShowcaseFurniture } from './furniture'
import { mountEnvironment } from './environment'
import { mountModels, type ShowcaseModels } from './models'
import {
  connectShowcase,
  hideShowcaseControls,
  MODE,
  type MpSdk,
  readMap,
  SWEEP_TRANSITION,
  type SweepData,
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
  /** With `birdsEye`: the camera to show (input is handled by the caller). */
  orbit?: Orbit | null
  /**
   * Put the camera here (Three.js Y-up) and take no camera input: the viewer
   * rides along instead of flying. Null holds the camera where it is. Whether
   * the view is steered (by this or `vantageFocus`) is fixed when it mounts.
   */
  cameraPose?: CameraPose | null
  /**
   * Watch this point (canonical Z-up metres) in this room from a fixed spot
   * chosen for the scene (see `vantagePose`), taking no camera input. Null
   * waits for a point.
   */
  vantageFocus?: { position_m: Vec3; room_id: string } | null
  /**
   * With `vantageFocus`: start at the vantage but keep the fly controls, to
   * look for a better spot. F goes back to the vantage.
   */
  flyFromVantage?: boolean
  /** An agent to leave out, e.g. the firefighter whose eyes the camera is. */
  hiddenAgentId?: string
  /**
   * A route to lay on the floor as an arrow. Only the part after
   * `routeSeconds` (the playback clock) is drawn.
   */
  route?: RouteRibbon | null
  routeSeconds?: number
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
 *
 * With `cameraPose` or `vantageFocus` the camera is steered for the viewer
 * instead, and the view takes no camera input at all (unless
 * `flyFromVantage`, which only starts the flight at the vantage).
 */
export function ShowcaseViewport({
  scene,
  floor,
  models,
  version,
  timeline,
  birdsEye = false,
  orbit = null,
  cameraPose,
  vantageFocus,
  flyFromVantage = false,
  hiddenAgentId,
  route = null,
  routeSeconds = 0,
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
  const steered = cameraPose !== undefined || (vantageFocus !== undefined && !flyFromVantage)
  const latestSteered = useRef(steered)
  const [poseDriver, setPoseDriver] = useState<PoseDriver | null>(null)
  const [sweeps, setSweeps] = useState<SweepData[]>([])
  const [dollhouseReady, setDollhouseReady] = useState(false)
  /** Showcase's dollhouse camera is wider than ours; measured once it's up. */
  const dollhouseFov = useRef(DEFAULT_DOLLHOUSE_FOV_DEGREES)
  // Burning furniture draws see-through. Keyed by the IDs so the boxes are
  // only rebuilt when that set changes, not on every fire frame.
  const burningKey = useMemo(() => [...burningObjectIds(fireFrame)].sort().join('\n'), [fireFrame])
  const items = useMemo(
    () => furnitureItems(floor, scene.assets, new Set(burningKey ? burningKey.split('\n') : [])),
    [floor, scene.assets, burningKey],
  )
  const latestItems = useRef(items)
  const fireVolumeNow = useMemo(
    () => fireVolume(fireCellBoxes(fireFrame, floor.objects)),
    [fireFrame, floor.objects],
  )
  const fire = useRef<ShowcaseFire | null>(null)
  const latestFireVolume = useRef(fireVolumeNow)
  const agents = useMemo(
    () =>
      simulationAgents.filter((agent) => agent.id !== hiddenAgentId && isShownAgent(agent, floor.id)),
    [simulationAgents, floor.id, hiddenAgentId],
  )
  // Every spraying firefighter's water, the hidden one's included: riding
  // with them, the stream leaves from just below the camera.
  const streams = useMemo(
    () =>
      simulationAgents.flatMap((agent) => (isShownAgent(agent, floor.id) && hoseStream(agent)) || []),
    [simulationAgents, floor.id],
  )
  const water = useRef<ShowcaseWater | null>(null)
  const latestStreams = useRef(streams)
  const routeArrow = useRef<ShowcaseRoute | null>(null)
  const latestRoute = useRef({ route, routeSeconds })
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
    latestFireVolume.current = fireVolumeNow
    fire.current?.update(fireVolumeNow)
  }, [fireVolumeNow])

  useEffect(() => {
    latestStreams.current = streams
    water.current?.update(streams)
  }, [streams])

  useEffect(() => {
    latestRoute.current = { route, routeSeconds }
    routeArrow.current?.update(route, routeSeconds)
  }, [route, routeSeconds])

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
    let steering: PoseDriver | null = null
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
        // A steered view hides the mode buttons too: they would walk off on their own.
        hideShowcaseControls(iframe.contentDocument, {
          birdsEye: latestBirdsEye.current || latestSteered.current,
        })
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
      fire.current = await mountFire(sdk, latestFireVolume.current)
      water.current = await mountWater(sdk, latestStreams.current)
      routeArrow.current = await mountRoute(sdk, latestRoute.current.route, latestRoute.current.routeSeconds)
      agentOverlay.current = await mountAgents(sdk, latestAgents.current)
      environment = await mountEnvironment(sdk)
      signal.throwIfAborted()
      if (latestBirdsEye.current) {
        await showDollhouse(sdk, signal)
        if (signal.aborted) return
        // Clicks can't reach Showcase here (see the layer below), but if the
        // view leaves the dollhouse anyway, put it back.
        dollhouseFov.current = await readFieldOfView(sdk)
        if (signal.aborted) return
        setDollhouseReady(true)
        dollhouseLock = sdk.Mode.current.subscribe((mode) => {
          if (mode === MODE.INSIDE || mode === MODE.FLOORPLAN) {
            sdk.Mode.moveTo(MODE.DOLLHOUSE).catch(() => {})
          }
        })
        clearTimeout(slowTimer)
        setStatus({ kind: 'ready', driver: null })
        return
      }
      if (latestSteered.current) {
        const scans = Object.values(await readMap(sdk.Sweep.data)).filter((sweep) => sweep.enabled)
        steering = await createPoseDriver(sdk)
        if (signal.aborted) return steering.dispose()
        clearTimeout(slowTimer)
        setSweeps(scans)
        setPoseDriver(steering)
        setStatus({ kind: 'ready', driver: null })
        return
      }
      // Scans are where a vantage stands (see `vantage` below).
      const scans = Object.values(await readMap(sdk.Sweep.data)).filter((sweep) => sweep.enabled)
      driver = await createDriver(sdk, startSweep)
      if (signal.aborted) return driver.dispose()
      setSweeps(scans)
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
      steering?.dispose()
      setPoseDriver(null)
      furniture.current?.dispose()
      furniture.current = null
      furnitureModels.current?.dispose()
      furnitureModels.current = null
      fire.current?.dispose()
      fire.current = null
      water.current?.dispose()
      water.current = null
      routeArrow.current?.dispose()
      routeArrow.current = null
      agentOverlay.current?.dispose()
      agentOverlay.current = null
      environment?.dispose()
      dollhouseLock?.cancel()
      setDollhouseReady(false)
      sdkRef.current = null
      iframe.remove()
    }
  }, [scene.id])

  useDollhouseOrbit(sdkRef, dollhouseReady ? orbit : null, dollhouseFov)

  const vantage = useMemo(() => {
    if (!vantageFocus) return null
    const room = floor.rooms.find((candidate) => candidate.id === vantageFocus.room_id)
    const outline = room ? chainLoops(room.boundary)[0] : undefined
    return vantagePose(scene.id, sweeps, vantageFocus.position_m, outline)
  }, [scene.id, floor.rooms, sweeps, vantageFocus])
  const steerTo = cameraPose ?? vantage
  useEffect(() => {
    if (steerTo) poseDriver?.setTarget(steerTo)
  }, [poseDriver, steerTo])
  // Flying from the vantage: start there (and on each new fire), then the
  // viewer is free.
  const flightDriver = status.kind === 'ready' ? status.driver : null
  useEffect(() => {
    if (flyFromVantage && vantage) flightDriver?.place?.(vantage)
  }, [flyFromVantage, flightDriver, vantage])

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
      {/* In the bird's-eye and steered views Showcase gets no input of its
          own: clicks would fly inside or walk away. The review's orbit controls sit above this and
          drive the camera through the `orbit` prop instead. */}
      {(birdsEye || steered) && <div aria-hidden className="absolute inset-0" />}
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

/** Showcase's dollhouse camera reported about 75° when measured; used until read. */
const DEFAULT_DOLLHOUSE_FOV_DEGREES = 75

/** Showcase's current vertical field of view, from its camera's projection. */
function readFieldOfView(sdk: MpSdk): Promise<number> {
  return new Promise((resolve) => {
    let done = false
    const finish = (fov: number) => {
      if (done) return
      done = true
      resolve(fov)
    }
    const subscription = sdk.Camera.pose.subscribe((pose) => {
      finish(fovFromProjection(pose.projection) ?? DEFAULT_DOLLHOUSE_FOV_DEGREES)
      queueMicrotask(() => subscription.cancel())
    })
    setTimeout(() => finish(DEFAULT_DOLLHOUSE_FOV_DEGREES), 2000)
  })
}

/** A rejected move (Showcase still mid-transition) is retried this often... */
const ORBIT_RETRY_MS = 250
/** ...this many times before waiting for the next change. */
const ORBIT_RETRIES = 12

/**
 * Moves Showcase's dollhouse camera to `orbit` whenever it changes. Moves
 * are instant and one at a time: while one is in flight, only the newest
 * orbit is kept and sent when it lands, so fast drags never queue up. Showcase
 * rejects moves while it's still settling into the dollhouse, so a rejected
 * move is retried (unless a newer orbit has arrived).
 */
function useDollhouseOrbit(
  sdkRef: RefObject<MpSdk | null>,
  orbit: Orbit | null,
  fovDegrees: RefObject<number>,
) {
  const inFlight = useRef(false)
  const pending = useRef<Orbit | null>(null)

  useEffect(() => {
    if (!orbit) return
    const send = (next: Orbit, retriesLeft: number) => {
      const sdk = sdkRef.current
      if (!sdk) return
      inFlight.current = true
      // Same framing through Showcase's wider lens: move in to match.
      const framed = orbitForFov(next, fovDegrees.current)
      const position = orbitPosition(framed)
      const { yaw, pitch } = orbitYawPitch(framed)
      sdk.Mode.moveTo(MODE.DOLLHOUSE, {
        position: { x: position.x, y: position.y, z: position.z },
        rotation: { x: (pitch * 180) / Math.PI, y: (yaw * 180) / Math.PI },
        transition: SWEEP_TRANSITION.INSTANT,
      }).then(
        () => {
          inFlight.current = false
          const queued = pending.current
          pending.current = null
          if (queued) send(queued, ORBIT_RETRIES)
        },
        () => {
          inFlight.current = false
          const queued = pending.current
          pending.current = null
          if (queued) send(queued, ORBIT_RETRIES)
          else if (retriesLeft > 0) setTimeout(() => {
            if (!inFlight.current && !pending.current) send(next, retriesLeft - 1)
          }, ORBIT_RETRY_MS)
        },
      )
    }
    if (inFlight.current) pending.current = orbit
    else send(orbit, ORBIT_RETRIES)
  }, [orbit, sdkRef, fovDegrees])
}
