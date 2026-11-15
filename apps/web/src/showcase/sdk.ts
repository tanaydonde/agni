// The slice of Matterport's Showcase SDK (Bundle 26.5.4) that the textured
// view uses. The full typings live in @matterport/webcomponent's
// built-bundle/sdk.d.ts, which the package's `exports` don't expose, and its
// enums can't be used here (`erasableSyntaxOnly`), so the values are copied.

import type * as THREE from 'three'

export interface Vector3 {
  x: number
  y: number
  z: number
}

export interface Subscription {
  cancel(): void
}

export interface Observable<T> {
  subscribe(callback: (value: T) => void): Subscription
}

export interface ObservableMap<T> {
  subscribe(observer: { onCollectionUpdated?(collection: Record<string, T>): void }): Subscription
}

export const PHASE = {
  PLAYING: 'appphase.playing',
  ERROR: 'appphase.error',
} as const

export const MODE = {
  INSIDE: 'mode.inside',
  DOLLHOUSE: 'mode.dollhouse',
  FLOORPLAN: 'mode.floorplan',
  TRANSITIONING: 'mode.transitioning',
} as const
export type Mode = (typeof MODE)[keyof typeof MODE]

export const DIRECTION = {
  FORWARD: 'FORWARD',
  BACK: 'BACK',
  LEFT: 'LEFT',
  RIGHT: 'RIGHT',
  UP: 'UP',
  DOWN: 'DOWN',
} as const
export type Direction = (typeof DIRECTION)[keyof typeof DIRECTION]

export const SWEEP_TRANSITION = {
  INSTANT: 'transition.instant',
  FLY: 'transition.fly',
  FADE: 'transition.fade',
} as const

/** `View.ViewType.DEFURNISH`: the copy of the space with furniture removed. */
export const DEFURNISH_VIEW_TYPE = 'viewtype.defurnish'
/** `View.ViewType.BASE` / `LAYERED_BASE`: the space as scanned, furniture and all. */
export const BASE_VIEW_TYPES = ['viewtype.base', 'viewtype.layeredbase']

export interface ShowcaseView {
  readonly id: string
  readonly name: string
  readonly type: string
  readonly active: boolean
  setActive(returnToStart?: boolean): Promise<void>
}

export interface SweepData {
  sid: string
  enabled: boolean
  position: Vector3
}

/** What Showcase hands a Scene component. `three` is Showcase's own copy. */
export interface ComponentContext {
  three: typeof THREE
  camera: THREE.Camera
  renderer: THREE.WebGLRenderer
  scene: THREE.Scene
}

/**
 * A Scene component, in the shape Showcase expects from `Scene.register`
 * factories. Showcase fills in `context` and `outputs` before `onInit`.
 */
export interface SceneComponent<Inputs extends object = Record<string, unknown>> {
  /** Leave out when unused: an empty `inputs: {}` makes Bundle 26.5.4's addComponent throw. */
  inputs?: Inputs
  outputs?: { objectRoot?: THREE.Object3D | null }
  context?: ComponentContext
  onInit?(): void
  onInputsUpdated?(previous: Inputs): void
  onTick?(delta: number): void
  onDestroy?(): void
}

/** Showcase's three.js vector and quaternion on a node, set in place. */
interface Settable3 {
  set(x: number, y: number, z: number): unknown
}

export interface SceneNode {
  addComponent<Inputs extends object>(name: string, inputs?: Partial<Inputs>): { inputs: Inputs }
  readonly position: Settable3
  readonly quaternion: { set(x: number, y: number, z: number, w: number): unknown }
  readonly scale: Settable3
}

export interface SceneObject {
  addNode(): SceneNode
  start(): void
  stop(): void
}

export interface MpSdk {
  App: { state: Observable<{ phase: string }> }
  View: {
    views: ObservableMap<ShowcaseView>
    current: Observable<ShowcaseView>
  }
  Mode: {
    current: Observable<string>
    /** In the dollhouse, `position` and `rotation` (degrees: x pitch, y yaw) set the camera. */
    moveTo(
      mode: Mode,
      options?: {
        transition?: string
        position?: { x: number; y: number; z: number }
        rotation?: { x: number; y: number }
      },
    ): Promise<string>
  }
  Camera: {
    moveInDirection(direction: Direction): Promise<void>
    /** The camera's pose; `projection` is a column-major 4×4 matrix. */
    pose: Observable<{ projection: ArrayLike<number> }>
  }
  Sweep: {
    data: ObservableMap<SweepData>
    current: Observable<SweepData>
    moveTo(sweepId: string, options?: { transition?: string }): Promise<string>
  }
  Scene: {
    register(name: string, factory: () => SceneComponent<never>): Promise<{ dispose(): void } | null>
    createObjects(count: number): Promise<SceneObject[]>
  }
  /**
   * Hidden, experimental helpers (used by @matterport/r3f). Turning both off
   * stops Showcase's own drag-to-look, arrow keys and click-to-walk, so we
   * can drive the camera ourselves.
   */
  R3F?: {
    controlsToggle(enabled: boolean): Promise<void>
    navigationToggle(enabled: boolean): Promise<void>
  }
  disconnect(): void
}

interface ShowcaseWindow extends Window {
  MP_SDK?: { connect(target: Window): Promise<MpSdk> }
}

/** URL of the Showcase player page for `modelId`. */
export function showcaseUrl(bundleUrl: string, modelId: string, sdkKey: string): string {
  const params = new URLSearchParams({
    m: modelId,
    applicationKey: sdkKey,
    play: '1', // start without the "play" splash
    title: '0',
    brand: '0',
    help: '0',
  })
  return `${bundleUrl}?${params}`
}

/** Connects to the Showcase player in `iframe` once its page has loaded. */
export async function connectShowcase(iframe: HTMLIFrameElement, signal: AbortSignal): Promise<MpSdk> {
  const target = iframe.contentWindow as ShowcaseWindow | null
  if (!target?.MP_SDK) {
    await new Promise<void>((resolve, reject) => {
      iframe.addEventListener('load', () => resolve(), { once: true, signal })
      signal.addEventListener('abort', () => reject(signal.reason), { once: true })
    })
  }
  const loaded = iframe.contentWindow as ShowcaseWindow | null
  if (!loaded?.MP_SDK) throw new Error('The Showcase player did not load.')
  return loaded.MP_SDK.connect(loaded)
}

/** Resolves once Showcase is playing; rejects if it reports an error first. */
export function untilPlaying(sdk: MpSdk, signal: AbortSignal): Promise<void> {
  return new Promise((resolve, reject) => {
    let settled = false
    const subscription = sdk.App.state.subscribe(({ phase }) => {
      if (phase === PHASE.PLAYING) finish()
      else if (phase === PHASE.ERROR) finish(new Error('Showcase failed to load the space.'))
    })
    const onAbort = () => finish(signal.reason)
    signal.addEventListener('abort', onAbort, { once: true })
    function finish(error?: unknown) {
      if (settled) return
      settled = true
      // The first callback can arrive synchronously, before `subscription` is set.
      queueMicrotask(() => subscription.cancel())
      signal.removeEventListener('abort', onAbort)
      if (error) reject(error)
      else resolve()
    }
  })
}

/** The current contents of an observable map, read once. */
export function readMap<T>(map: ObservableMap<T>): Promise<Record<string, T>> {
  return new Promise((resolve) => {
    let subscription: Subscription | undefined
    let done = false
    subscription = map.subscribe({
      onCollectionUpdated(collection) {
        if (done) return
        done = true
        resolve({ ...collection })
        if (subscription) subscription.cancel()
        else queueMicrotask(() => subscription?.cancel())
      },
    })
  })
}

/**
 * Showcase controls we don't want, hidden with CSS: Showcase only has URL
 * parameters for some of them (`vr=0`; `mls=1` drops share too but keeps the
 * logo). Aria-labels are Showcase's English strings; the logo's class is
 * hashed per build except for its `logo--footer` prefix.
 */
const HIDDEN_SHOWCASE_CONTROLS = [
  'footer[class*="showcase-footer"]', // the "AI Enhanced | Help | Terms" links
  'button[aria-label="Search"]',
  'button[aria-label="Share this Space"]',
  'button.view-in-vr',
  'button[aria-label="View Fullscreen"]',
  '[class*="logo--footer"]', // the Matterport logo, linked or not
]

/**
 * Also hidden in the fixed bird's-eye view: the mode and tool buttons, which
 * would leave the dollhouse (floor plan, walking inside, measuring) or swap
 * in the furnished scan. Showcase's Help and Terms links stay.
 */
const BIRDS_EYE_HIDDEN_CONTROLS = [
  // Dollhouse / Floor Plan / Explore 3D Space, with the active-mode highlight.
  '.mode-controls',
  // Prefix matches: Showcase's labels can end with a trailing space.
  'button[aria-label^="Measurements"]',
  'button[aria-label^="Show Furniture"]',
  '.showcase-settings', // the gear in the top right
]

/**
 * Hides HIDDEN_SHOWCASE_CONTROLS in the (same-origin) Showcase page, plus
 * the mode buttons when `birdsEye` keeps the player in its dollhouse.
 */
export function hideShowcaseControls(page: Document, { birdsEye = false } = {}): void {
  const hidden = birdsEye ? [...HIDDEN_SHOWCASE_CONTROLS, ...BIRDS_EYE_HIDDEN_CONTROLS] : HIDDEN_SHOWCASE_CONTROLS
  const style = page.createElement('style')
  style.dataset.tflop = 'hidden-controls'
  style.textContent = `${hidden.join(',\n')} { display: none !important; }`
  page.head.append(style)
}
