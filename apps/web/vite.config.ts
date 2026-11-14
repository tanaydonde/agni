import { createReadStream } from 'node:fs'
import { readFile, stat } from 'node:fs/promises'
import { extname, normalize, sep } from 'node:path'
import { fileURLToPath } from 'node:url'
import tailwindcss from '@tailwindcss/vite'
import react from '@vitejs/plugin-react'
import { defineConfig, loadEnv, type Plugin } from 'vite'

const REPO_ROOT = fileURLToPath(new URL('../../', import.meta.url))
const PREPARED_SCENES_DIR = `${REPO_ROOT}data/prepared-scenes/`
/** Matterport's self-hosted Showcase player, shipped in @matterport/webcomponent. */
const SHOWCASE_BUNDLE_DIR = fileURLToPath(
  new URL('./node_modules/@matterport/webcomponent/built-bundle/', import.meta.url),
)
const SHOWCASE_BUNDLE_ROUTE = '/matterport/bundle'

const CONTENT_TYPES: Record<string, string> = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.cjs': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.json': 'application/json',
  '.wasm': 'application/wasm',
  '.png': 'image/png',
  '.jpg': 'image/jpeg',
  '.svg': 'image/svg+xml',
  '.ico': 'image/x-icon',
  '.woff': 'font/woff',
  '.woff2': 'font/woff2',
  '.glb': 'model/gltf-binary',
  '.txt': 'text/plain; charset=utf-8',
}

interface BundlePatch {
  find: string
  replace: string
  why: string
}

/**
 * Edits applied to bundle files as they are served. They target the exact
 * minified code of the pinned @matterport/webcomponent version; the server
 * warns if one stops matching after an upgrade.
 */
const SHOWCASE_PATCHES: Record<string, BundlePatch[]> = {
  '/js/showcase.js': [
    {
      // `_` is Features["features/exterior_views"], from Matterport's server flags.
      find: '_=i.has(_t.G),',
      replace: '_=!1,',
      why:
        'turn off exterior Gaussian splats: bundle 26.5.4 cannot decode the splats ' +
        'Matterport now serves (its splat worker throws), and Showcase waits on them forever',
    },
  ],
}

/** Reads a bundle file with its patches applied. */
async function patchedBundleFile(file: string, patches: BundlePatch[], warn: (msg: string) => void) {
  let text = await readFile(file, 'utf8')
  for (const patch of patches) {
    if (text.split(patch.find).length !== 2) {
      warn(`[showcase-bundle] patch no longer matches ${file}; not applied: ${patch.why}`)
      continue
    }
    text = text.replace(patch.find, patch.replace)
  }
  return Buffer.from(text)
}

/**
 * Dev-only route for the Showcase bundle, which the textured view loads in an
 * iframe. Served straight from node_modules so the 30 MB player never lands in
 * the repo, with SHOWCASE_PATCHES applied.
 *   GET /matterport/bundle/*  → @matterport/webcomponent/built-bundle/*
 */
function showcaseBundle(): Plugin {
  return {
    name: 'showcase-bundle',
    configureServer(server) {
      const patched = new Map<string, Promise<Buffer>>()
      const warn = (msg: string) => server.config.logger.warn(msg)

      server.middlewares.use(SHOWCASE_BUNDLE_ROUTE, async (req, res, next) => {
        if (req.method !== 'GET' && req.method !== 'HEAD') return next()
        const path = decodeURIComponent((req.url ?? '/').split('?')[0])
        const file = normalize(`${SHOWCASE_BUNDLE_DIR}${path}`)
        // Refuse anything that climbs out of the bundle directory.
        if (!file.startsWith(SHOWCASE_BUNDLE_DIR.replace(/[/\\]$/, '') + sep)) return next()

        const info = await stat(file).catch(() => null)
        if (!info?.isFile()) {
          res.statusCode = 404
          res.end()
          return
        }
        res.setHeader('Content-Type', CONTENT_TYPES[extname(file)] ?? 'application/octet-stream')

        const patches = SHOWCASE_PATCHES[path]
        if (patches) {
          if (!patched.has(file)) patched.set(file, patchedBundleFile(file, patches, warn))
          const body = await patched.get(file)!
          res.setHeader('Content-Length', body.length)
          res.end(req.method === 'HEAD' ? undefined : body)
          return
        }

        res.setHeader('Content-Length', info.size)
        if (req.method === 'HEAD') return res.end()
        createReadStream(file).pipe(res)
      })
    },
  }
}

/** Prepared files served per scene: route file name → suffix on disk. */
const PREPARED_FILES: Record<string, { suffix: string; contentType: string }> = {
  'mesh.obj': { suffix: '.obj', contentType: 'model/obj' },
  'detected.json': { suffix: '.detected.json', contentType: 'application/json' },
}

/**
 * Dev-only routes for files written beside the scene: the defurnished mesh
 * from `cargo run -p import-scene` and the detected furniture from
 * `cargo run -p detect-furniture`. The scene itself comes from the Rust world
 * server (`/api/world`).
 *   GET /api/scenes/:id/mesh.obj       → data/prepared-scenes/:id.obj
 *   GET /api/scenes/:id/detected.json  → data/prepared-scenes/:id.detected.json
 */
function preparedFiles(): Plugin {
  return {
    name: 'prepared-files',
    configureServer(server) {
      server.middlewares.use('/api/scenes', async (req, res, next) => {
        const route = /^\/([A-Za-z0-9_-]+)\/([a-z.]+)$/.exec((req.url ?? '').split('?')[0])
        const prepared = route && Object.hasOwn(PREPARED_FILES, route[2]) ? PREPARED_FILES[route[2]] : undefined
        if (!route || !prepared || (req.method !== 'GET' && req.method !== 'HEAD')) return next()

        const file = `${PREPARED_SCENES_DIR}${route[1]}${prepared.suffix}`
        const info = await stat(file).catch(() => null)
        if (!info?.isFile()) {
          res.statusCode = 404
          res.setHeader('Content-Type', 'application/json')
          res.end(JSON.stringify({ error: `No prepared ${route[2]} for ${route[1]}` }))
          return
        }

        res.setHeader('Content-Type', prepared.contentType)
        res.setHeader('Content-Length', info.size)
        if (req.method === 'HEAD') return res.end()
        createReadStream(file).pipe(res)
      })
    },
  }
}

export default defineConfig(({ mode }) => {
  // Default to the model the importer uses. Only these keys are read from the
  // repo-root .env. The Model API token ID and secret never reach the client;
  // the Showcase SDK key does, since it is a browser key restricted by domain.
  const env = loadEnv(mode, REPO_ROOT, ['MATTERPORT_MODEL_ID', 'MATTERPORT_SDK_KEY', 'TFLOP_SERVER_ADDR'])
  const sceneId = process.env.VITE_SCENE_ID ?? env.MATTERPORT_MODEL_ID ?? ''
  const sdkKey = process.env.MATTERPORT_SDK_KEY ?? env.MATTERPORT_SDK_KEY ?? ''
  const worldServer = `http://${process.env.TFLOP_SERVER_ADDR ?? env.TFLOP_SERVER_ADDR ?? '127.0.0.1:3000'}`

  return {
    plugins: [react(), tailwindcss(), preparedFiles(), showcaseBundle()],
    define: {
      __SCENE_ID__: JSON.stringify(sceneId),
      __MATTERPORT_SDK_KEY__: JSON.stringify(sdkKey),
      __SHOWCASE_BUNDLE_URL__: JSON.stringify(`${SHOWCASE_BUNDLE_ROUTE}/showcase.html`),
    },
    // The Rust world server (`cargo run -p tflop-server`) owns the scene.
    server: { proxy: { '/api/world': worldServer } },
  }
})
