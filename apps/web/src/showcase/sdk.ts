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
}

/**
 * A Scene component, in the shape Showcase expects from `Scene.register`
 * factories. Showcase fills in `context` and `outputs` before `onInit`.
 */
export interface SceneComponent<Inputs extends object = Record<string, unknown>> {
  inputs: Inputs
  outputs?: { objectRoot?: THREE.Object3D | null }
  context?: ComponentContext
  onInit?(): void
  onInputsUpdated?(previous: Inputs): void
  onTick?(delta: number): void
  onDestroy?(): void
}

export interface SceneNode {
  addComponent<Inputs extends object>(name: string, inputs?: Partial<Inputs>): { inputs: Inputs }
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
    moveTo(mode: Mode, options?: { transition?: string }): Promise<string>
  }
  Camera: { moveInDirection(direction: Direction): Promise<void> }
  Sweep: {
    data: ObservableMap<SweepData>
    current: Observable<SweepData>
    moveTo(sweepId: string, options?: { transition?: string }): Promise<string>
  }
  Scene: {
    register(name: string, factory: () => SceneComponent<never>): Promise<{ dispose(): void } | null>
    createObjects(count: number): Promise<SceneObject[]>
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
