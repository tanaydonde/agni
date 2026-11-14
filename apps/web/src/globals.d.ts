/** Scene shown at startup: VITE_SCENE_ID, else MATTERPORT_MODEL_ID from the repo-root .env. */
declare const __SCENE_ID__: string
/** Showcase SDK key (MATTERPORT_SDK_KEY in the repo-root .env); empty when unset. */
declare const __MATTERPORT_SDK_KEY__: string
/** Where the dev server serves Matterport's Showcase player page. */
declare const __SHOWCASE_BUNDLE_URL__: string
