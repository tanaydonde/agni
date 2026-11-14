import { createReadStream } from 'node:fs'
import { stat } from 'node:fs/promises'
import { fileURLToPath } from 'node:url'
import tailwindcss from '@tailwindcss/vite'
import react from '@vitejs/plugin-react'
import { defineConfig, loadEnv, type Plugin } from 'vite'

const REPO_ROOT = fileURLToPath(new URL('../../', import.meta.url))
const PREPARED_SCENES_DIR = `${REPO_ROOT}data/prepared-scenes/`

/**
 * Dev-only route for the defurnished mesh that `cargo run -p import-scene`
 * writes. The scene itself comes from the Rust world server (`/api/world`).
 *   GET /api/scenes/:id/mesh.obj  → data/prepared-scenes/:id.obj
 */
function preparedMeshes(): Plugin {
  return {
    name: 'prepared-meshes',
    configureServer(server) {
      server.middlewares.use('/api/scenes', async (req, res, next) => {
        const route = /^\/([A-Za-z0-9_-]+)\/mesh\.obj$/.exec((req.url ?? '').split('?')[0])
        if (!route || (req.method !== 'GET' && req.method !== 'HEAD')) return next()

        const file = `${PREPARED_SCENES_DIR}${route[1]}.obj`
        const info = await stat(file).catch(() => null)
        if (!info?.isFile()) {
          res.statusCode = 404
          res.setHeader('Content-Type', 'application/json')
          res.end(JSON.stringify({ error: `No prepared mesh for ${route[1]}` }))
          return
        }

        res.setHeader('Content-Type', 'model/obj')
        res.setHeader('Content-Length', info.size)
        if (req.method === 'HEAD') return res.end()
        createReadStream(file).pipe(res)
      })
    },
  }
}

export default defineConfig(({ mode }) => {
  // Default to the model the importer uses. Only these keys are read from the
  // repo-root .env; the Matterport credentials never reach the client.
  const env = loadEnv(mode, REPO_ROOT, ['MATTERPORT_MODEL_ID', 'TFLOP_SERVER_ADDR'])
  const sceneId = process.env.VITE_SCENE_ID ?? env.MATTERPORT_MODEL_ID ?? ''
  const worldServer = `http://${process.env.TFLOP_SERVER_ADDR ?? env.TFLOP_SERVER_ADDR ?? '127.0.0.1:3000'}`

  return {
    plugins: [react(), tailwindcss(), preparedMeshes()],
    define: { __SCENE_ID__: JSON.stringify(sceneId) },
    // The Rust world server (`cargo run -p tflop-server`) owns the scene.
    server: { proxy: { '/api/world': worldServer } },
  }
})
