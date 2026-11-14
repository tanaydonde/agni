// Asks the Rust world, while the user drags, whether the current placement
// would be accepted. Requests are throttled and never overlap: only the newest
// edit is sent once the previous answer arrives, so a fast drag costs at most
// one request per interval.

import type { CheckEditsResponse, EditCommand } from './types'

/** Minimum time between dry-run requests. */
export const CHECK_INTERVAL_MS = 60

/** A stable identity for an edit, to match answers to what's on screen. */
export function editKey(edit: EditCommand): string {
  return JSON.stringify(edit)
}

export interface LiveChecker {
  /** Checks `edit` soon, replacing any edit still waiting to be sent. */
  request: (edit: EditCommand) => void
  /** Drops the waiting edit and ignores answers to requests already sent. */
  cancel: () => void
}

export function createLiveChecker(
  check: (edit: EditCommand) => Promise<CheckEditsResponse>,
  onResult: (key: string, response: CheckEditsResponse) => void,
  intervalMs = CHECK_INTERVAL_MS,
): LiveChecker {
  let pending: EditCommand | null = null
  let busy = false
  let timer: ReturnType<typeof setTimeout> | null = null
  let lastStart = -Infinity
  let generation = 0

  const pump = () => {
    if (busy || timer || !pending) return
    timer = setTimeout(run, Math.max(0, lastStart + intervalMs - Date.now()))
  }

  const run = async () => {
    timer = null
    const edit = pending
    pending = null
    if (!edit) return
    busy = true
    lastStart = Date.now()
    const current = generation
    try {
      const response = await check(edit)
      if (current === generation) onResult(editKey(edit), response)
    } catch {
      // Unreachable server: leave the verdict unknown; committing will report it.
    } finally {
      busy = false
      pump()
    }
  }

  return {
    request: (edit) => {
      pending = edit
      pump()
    },
    cancel: () => {
      pending = null
      generation++
      if (timer) clearTimeout(timer)
      timer = null
    },
  }
}
