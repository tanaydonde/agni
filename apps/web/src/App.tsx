import { useEffect, useState } from 'react'
import type { ChatMessage } from './chat'
import { ActionBar } from './components/ActionBar'
import { AssetPalette } from './components/AssetPalette'
import { BuildingBrief } from './components/BuildingBrief'
import { ChatPanel } from './components/ChatPanel'
import { SceneViewport } from './components/SceneViewport'
import { useFireStore } from './fire/fireStore'
import {
  fetchProperty,
  isUntouchedScenario,
  scenarioFromProperty,
  scenarioWithoutProperty,
} from './property/api'
import type { PropertyBrief } from './property/types'
import { useEditorStore } from './scene/editorStore'


export default function App() {
  const [messages, setMessages] = useState<ChatMessage[]>([])
  const selectedObjectId = useEditorStore((state) => state.selectedObjectId)
  const fireLoading = useFireStore((state) => state.status === 'loading')
  const startFire = useFireStore((state) => state.start)
  const scenario = useFireStore((state) => state.scenario)
  const setScenario = useFireStore((state) => state.setScenario)
  const [property, setProperty] = useState<PropertyBrief | null>(null)
  const [useCostar, setUseCostar] = useState(true)

  const toggleCostar = (on: boolean) => {
    setUseCostar(on)
    if (!property) return
    const current = useFireStore.getState().scenario
    setScenario(on ? scenarioFromProperty(property) : scenarioWithoutProperty(property, current))
  }

  // The scan's CoStar record prefills the scenario unless the user already
  // changed it by hand. The Brief's toggle switches the CoStar values off; the
  // occupant count and layout controls override those two.
  useEffect(() => {
    let cancelled = false
    void fetchProperty().then((brief) => {
      if (cancelled || !brief) return
      setProperty(brief)
      if (isUntouchedScenario(useFireStore.getState().scenario)) {
        setScenario(scenarioFromProperty(brief))
      }
    })
    return () => {
      cancelled = true
    }
  }, [setScenario])

  const handleSend = (text: string) => {
    setMessages((prev) => [...prev, { id: crypto.randomUUID(), role: 'user', text }])
  }

  return (
    <main className="flex h-full min-h-[600px] min-w-[960px] gap-4 bg-canvas p-4">
      <SceneViewport sceneId={__SCENE_ID__} />
      <aside className="flex w-[400px] flex-none flex-col gap-3">
        {property && (
          <BuildingBrief brief={property} applied={useCostar} onAppliedChange={toggleCostar} />
        )}
        <ActionBar
          canRunSimulation={selectedObjectId !== null && !fireLoading}
          onRunSimulation={() => selectedObjectId && void startFire(selectedObjectId)}
          scenario={scenario}
          onScenarioChange={setScenario}
          suggestedOccupantCount={useCostar ? property?.profile.occupant_count.value : undefined}
        />
        <AssetPalette />
        <ChatPanel messages={messages} onSend={handleSend} />
      </aside>
    </main>
  )
}
