// A two-page app doesn't need a router library: the path picks the page, and
// links push history entries so the simulation held in memory (fireStore)
// survives moving between pages.

import { useSyncExternalStore } from 'react'

function subscribe(onChange: () => void): () => void {
  window.addEventListener('popstate', onChange)
  return () => window.removeEventListener('popstate', onChange)
}

/** The current path without a trailing slash (`/` stays `/`). */
export function usePathname(): string {
  return useSyncExternalStore(subscribe, () => normalizePath(window.location.pathname))
}

export function normalizePath(path: string): string {
  return path.length > 1 ? path.replace(/\/+$/, '') || '/' : '/'
}

export function navigate(path: string): void {
  if (normalizePath(window.location.pathname) === path) return
  window.history.pushState(null, '', path)
  window.dispatchEvent(new PopStateEvent('popstate'))
}
