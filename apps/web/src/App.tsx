import { ActionBar } from './components/ActionBar'
import { SceneViewport } from './components/SceneViewport'
import { useFireStore } from './fire/fireStore'
import { useEditorStore } from './scene/editorStore'

export default function App() {
  const selectedObjectId = useEditorStore((state) => state.selectedObjectId)
  const fireLoading = useFireStore((state) => state.status === 'loading')
  const startFire = useFireStore((state) => state.start)
  const scenario = useFireStore((state) => state.scenario)
  const setScenario = useFireStore((state) => state.setScenario)

  return (
    <main className="flex h-full min-h-[600px] min-w-[960px] flex-col gap-3 bg-canvas p-3">
      <header aria-label="Fire simulation setup" className="flex-none">
        <ActionBar
          canRunSimulation={selectedObjectId !== null && !fireLoading}
          hasSelectedOrigin={selectedObjectId !== null}
          onRunSimulation={() => selectedObjectId && void startFire(selectedObjectId)}
          scenario={scenario}
          onScenarioChange={setScenario}
        />
      </header>
      <SceneViewport sceneId={__SCENE_ID__} />
    </main>
  )
}
