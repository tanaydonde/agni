import {
  BASE_VIEW_TYPES,
  DEFURNISH_VIEW_TYPE,
  MODE,
  type MpSdk,
  type Observable,
  readMap,
  type ShowcaseView,
  SWEEP_TRANSITION,
  type SweepData,
} from './sdk'

export class NoDefurnishedViewError extends Error {
  constructor() {
    super('This space has no defurnished view, so Matterport has no empty-room textures for it.')
    this.name = 'NoDefurnishedViewError'
  }
}

/** Which copy of the space Showcase shows. */
export type SpaceVersion = 'furnished' | 'defurnished'

/** The Showcase view for a version, or undefined if the space has none. */
export function viewFor<View extends Pick<ShowcaseView, 'type'>>(views: View[], version: SpaceVersion): View | undefined {
  if (version === 'defurnished') return views.find((view) => view.type === DEFURNISH_VIEW_TYPE)
  // Prefer the plain base view over a layered one.
  for (const type of BASE_VIEW_TYPES) {
    const view = views.find((candidate) => candidate.type === type)
    if (view) return view
  }
  return undefined
}

/**
 * Switches Showcase between the furnished and defurnished copies of the space
 * without moving the camera, so the two can be compared from one spot.
 */
export async function showSpaceVersion(sdk: MpSdk, version: SpaceVersion): Promise<void> {
  const view = viewFor(Object.values(await readMap(sdk.View.views)), version)
  if (!view) throw new Error(`This space has no ${version} view.`)
  if (!view.active) await view.setActive(false)
}

const STEP_INSIDE_ATTEMPTS = 5
const RETRY_MS = 500

/**
 * Switches Showcase to the space's defurnished view (Matterport's copy with
 * the furniture removed, which streams its own textured mesh), then stands
 * the camera at a scan, since WASD only hops between scans. Returns that
 * scan, which `home` goes back to, or null if Showcase never let us in.
 */
export async function activateDefurnishedView(sdk: MpSdk): Promise<string | null> {
  const views = Object.values(await readMap(sdk.View.views))
  const defurnished = viewFor(views, 'defurnished')
  if (!defurnished) throw new NoDefurnishedViewError()
  if (!defurnished.active) await defurnished.setActive(true)

  const start = centralSweep(Object.values(await readMap(sdk.Sweep.data)))
  if (!start) return null
  // Showcase opens with an animated dollhouse fly-in and rejects moves made
  // mid-transition, so wait for it to settle and retry.
  for (let attempt = 0; attempt < STEP_INSIDE_ATTEMPTS; attempt++) {
    await untilSettled(sdk.Mode.current)
    try {
      await sdk.Sweep.moveTo(start, { transition: SWEEP_TRANSITION.FLY })
      return start
    } catch {
      await new Promise((resolve) => setTimeout(resolve, RETRY_MS))
    }
  }
  return null
}

/**
 * Moves Showcase to its dollhouse: the whole textured model seen from above,
 * with its own orbit and zoom. Retries while an intro or other transition is
 * still running, as stepping inside does.
 */
export async function showDollhouse(sdk: MpSdk): Promise<void> {
  let lastError: unknown
  for (let attempt = 0; attempt < STEP_INSIDE_ATTEMPTS; attempt++) {
    await untilSettled(sdk.Mode.current)
    try {
      await sdk.Mode.moveTo(MODE.DOLLHOUSE)
      return
    } catch (error) {
      lastError = error
      await new Promise((resolve) => setTimeout(resolve, RETRY_MS))
    }
  }
  throw lastError instanceof Error ? lastError : new Error("Matterport wouldn't switch to the dollhouse view.")
}

/** The enabled scan nearest the middle of all of them: a central place to start. */
export function centralSweep(sweeps: Pick<SweepData, 'sid' | 'enabled' | 'position'>[]): string | null {
  const enabled = sweeps.filter((sweep) => sweep.enabled)
  if (enabled.length === 0) return null
  const mean = { x: 0, y: 0, z: 0 }
  for (const { position } of enabled) {
    mean.x += position.x / enabled.length
    mean.y += position.y / enabled.length
    mean.z += position.z / enabled.length
  }
  const distance = ({ position: p }: (typeof enabled)[number]) =>
    (p.x - mean.x) ** 2 + (p.y - mean.y) ** 2 + (p.z - mean.z) ** 2
  return enabled.reduce((best, sweep) => (distance(sweep) < distance(best) ? sweep : best)).sid
}

/** Resolves once the view mode isn't mid-transition. */
function untilSettled(mode: Observable<string>): Promise<void> {
  return new Promise((resolve) => {
    let done = false
    const subscription = mode.subscribe((current) => {
      if (done || current === MODE.TRANSITIONING) return
      done = true
      resolve()
      queueMicrotask(() => subscription.cancel())
    })
  })
}
