import { useEffect } from 'react'
import { useFireStore } from './fireStore'

/**
 * Advances the fire playback clock every animation frame while playing.
 * Lives outside any 3D canvas so playback runs in whichever view is showing.
 */
export function usePlaybackClock() {
  const playing = useFireStore((state) => state.status === 'playing')
  const tick = useFireStore((state) => state.tick)

  useEffect(() => {
    if (!playing) return
    let last = performance.now()
    let request = requestAnimationFrame(function step(now) {
      tick((now - last) / 1000)
      last = now
      request = requestAnimationFrame(step)
    })
    return () => cancelAnimationFrame(request)
  }, [playing, tick])
}
