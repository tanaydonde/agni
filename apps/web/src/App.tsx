import { useEffect } from 'react'
import { useFireStore } from './fire/fireStore'
import { DemoPage } from './pages/DemoPage'
import { EditorPage } from './pages/EditorPage'
import { MethodsPage } from './pages/MethodsPage'
import { usePathname } from './router'

export default function App() {
  const path = usePathname()
  const restoreBatch = useFireStore((state) => state.restoreBatch)
  // Typing a page's address reloads the app; bring back the last simulation.
  useEffect(() => void restoreBatch(), [restoreBatch])
  if (path === '/demo') return <DemoPage />
  if (path === '/methods') return <MethodsPage />
  return <EditorPage />
}
