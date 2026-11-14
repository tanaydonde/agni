import type { ThreeElements } from '@react-three/fiber'

// TypeScript 7 does not currently pick up React Three Fiber's JSX augmentation
// through the package entry point, so expose the renderer elements explicitly.
declare module 'react/jsx-runtime' {
  namespace JSX {
    interface IntrinsicElements extends ThreeElements {}
  }
}

declare global {
  /** Scene shown at startup: VITE_SCENE_ID, else MATTERPORT_MODEL_ID from the repo-root .env. */
  const __SCENE_ID__: string
  /** Showcase SDK key (MATTERPORT_SDK_KEY in the repo-root .env); empty when unset. */
  const __MATTERPORT_SDK_KEY__: string
  /** Where the dev server serves Matterport's Showcase player page. */
  const __SHOWCASE_BUNDLE_URL__: string
}
