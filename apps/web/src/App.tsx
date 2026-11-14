import { useState } from 'react'
import type { ChatMessage } from './chat'
import { ActionBar } from './components/ActionBar'
import { ChatPanel } from './components/ChatPanel'
import { SceneViewport } from './components/SceneViewport'


export default function App() {
  const [messages, setMessages] = useState<ChatMessage[]>([])

  const handleSend = (text: string) => {
    setMessages((prev) => [...prev, { id: crypto.randomUUID(), role: 'user', text }])
  }

  return (
    <main className="flex h-full min-h-[600px] min-w-[960px] gap-4 bg-canvas p-4">
      <SceneViewport sceneId={__SCENE_ID__} />
      <aside className="flex w-[400px] flex-none flex-col gap-3">
        <ActionBar canRunSimulation={false} />
        <ChatPanel messages={messages} onSend={handleSend} />
      </aside>
    </main>
  )
}
