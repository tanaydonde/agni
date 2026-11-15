import { useState } from 'react'
import type { ChatMessage } from './chat'
import { ActionBar } from './components/ActionBar'
import { AssetPalette } from './components/AssetPalette'
import { ChatPanel } from './components/ChatPanel'
import { SceneViewport } from './components/SceneViewport'
import { useFireStore } from './fire/fireStore'
import { useEditorStore } from './scene/editorStore'


export default function App() {
  const [messages, setMessages] = useState<ChatMessage[]>([])
  const selectedObjectId = useEditorStore((state) => state.selectedObjectId)
  const fireLoading = useFireStore((state) => state.status === 'loading')
  const startFire = useFireStore((state) => state.start)
  const scenario = useFireStore((state) => state.scenario)
  const setScenario = useFireStore((state) => state.setScenario)

  const handleSend = (text: string) => {
    setMessages((prev) => [...prev, { id: crypto.randomUUID(), role: 'user', text }])
  }

  return (
    <main className="flex h-full min-h-[600px] min-w-[960px] gap-4 bg-canvas p-4">
      <SceneViewport sceneId={__SCENE_ID__} />
      <aside className="flex w-[400px] flex-none flex-col gap-3">
        <ActionBar
          canRunSimulation={selectedObjectId !== null && !fireLoading}
          hasSelectedOrigin={selectedObjectId !== null}
          onRunSimulation={() => selectedObjectId && void startFire(selectedObjectId)}
          scenario={scenario}
          onScenarioChange={setScenario}
        />
        <AssetPalette />
        <ChatPanel messages={messages} onSend={handleSend} />
      </aside>
    </main>
  )
}
