import { createReadStream } from 'node:fs'
import { stat } from 'node:fs/promises'
import { fileURLToPath } from 'node:url'
import tailwindcss from '@tailwindcss/vite'
import react from '@vitejs/plugin-react'
import { defineConfig, loadEnv, type Plugin } from 'vite'

const REPO_ROOT = fileURLToPath(new URL('../../', import.meta.url))
const PREPARED_SCENES_DIR = `${REPO_ROOT}data/prepared-scenes/`

/**
 * Dev-only stand-in for the Rust server's scene routes, serving what
 * `cargo run -p import-scene` writes:
 *   GET /api/scenes/:id           → data/prepared-scenes/:id.json
 *   GET /api/scenes/:id/mesh.obj  → data/prepared-scenes/:id.obj (defurnished mesh)
 */
function preparedScenes(): Plugin {
  return {
    name: 'prepared-scenes',
    configureServer(server) {
      server.middlewares.use('/api/scenes', async (req, res, next) => {
        const route = /^\/([A-Za-z0-9_-]+)(\/mesh\.obj)?$/.exec((req.url ?? '').split('?')[0])
        if (!route || (req.method !== 'GET' && req.method !== 'HEAD')) return next()

        const [, id, mesh] = route
        const file = `${PREPARED_SCENES_DIR}${id}${mesh ? '.obj' : '.json'}`
        const info = await stat(file).catch(() => null)
        if (!info?.isFile()) {
          res.statusCode = 404
          res.setHeader('Content-Type', 'application/json')
          res.end(JSON.stringify({ error: `No prepared ${mesh ? 'mesh' : 'scene'} for ${id}` }))
          return
        }

        res.setHeader('Content-Type', mesh ? 'model/obj' : 'application/json')
        res.setHeader('Content-Length', info.size)
        if (req.method === 'HEAD') return res.end()
        createReadStream(file).pipe(res)
      })
    },
  }
}

export default defineConfig(({ mode }) => {
  // Default to the model the importer uses. Only this key is read from the
  // repo-root .env; the Matterport credentials never reach the client.
  const { MATTERPORT_MODEL_ID } = loadEnv(mode, REPO_ROOT, 'MATTERPORT_MODEL_ID')
  const sceneId = process.env.VITE_SCENE_ID ?? MATTERPORT_MODEL_ID ?? ''

  return {
    plugins: [react(), tailwindcss(), preparedScenes()],
    define: { __SCENE_ID__: JSON.stringify(sceneId) },
  }
})
