import { useEffect, useMemo } from 'react'
import { useFireStore } from '../fire/fireStore'
import { usePlaybackClock } from '../fire/usePlaybackClock'
import { useWorld } from '../scene/api'
import { buildFloorGeometry, type FloorGeometry } from '../scene/geometry'
import type { Floor } from '../scene/types'

/**
 * Playback for the demo pages. They show the batch's first representative
 * (the top scenario), so the clock runs to that run's end, not the batch's.
 * A batch simulated before the building last changed is dropped.
 */
export function useDemoPlayback() {
  usePlaybackClock()
  const setClockScope = useFireStore((state) => state.setClockScope)
  useEffect(() => {
    setClockScope('run')
    return () => setClockScope('batch')
  }, [setClockScope])

  const batch = useFireStore((state) => state.batch)
  const clear = useFireStore((state) => state.clear)
  const revision = useWorld().data?.revision
  useEffect(() => {
    if (batch && revision !== undefined && revision !== batch.revision) clear()
  }, [batch, revision, clear])
}

export function useFloorGeometry({ elevation_m, rooms }: Floor): FloorGeometry {
  return useMemo(() => buildFloorGeometry({ elevation_m, rooms }), [elevation_m, rooms])
}
