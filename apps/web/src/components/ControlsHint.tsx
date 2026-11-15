import { useEffect, useRef, useState } from 'react'

/** Key or gesture names, and what they do. */
export type Controls = [keys: string[], action: string][]

/**
 * The legend of controls in the corner of a 3D view. It starts collapsed to
 * a small button, so it stays out of the way until someone wants it.
 */
export function ControlsHint({ title, controls }: { title: string; controls: Controls }) {
  const [open, setOpen] = useState(false)
  // The 3D views listen for pointerdown natively on an ancestor (selection,
  // camera); stop it here so pressing the legend never selects, deselects,
  // or starts flying.
  const root = useRef<HTMLDivElement>(null)
  useEffect(() => {
    const el = root.current
    if (!el) return
    const stop = (event: Event) => event.stopPropagation()
    el.addEventListener('pointerdown', stop)
    return () => el.removeEventListener('pointerdown', stop)
  }, [])
  const toggle = (
    <button
      type="button"
      aria-expanded={open}
      onClick={() => setOpen((shown) => !shown)}
      className="pointer-events-auto flex items-center gap-1.5 font-medium text-ink hover:text-accent"
    >
      Controls · {title}
      <span aria-hidden className="text-muted">
        {open ? '▾' : '▸'}
      </span>
    </button>
  )

  return (
    <div ref={root} className="pointer-events-none absolute bottom-3 left-3 flex flex-col gap-2 rounded-lg border border-hairline bg-white px-3 py-2 text-xs text-ink-secondary shadow-[0_4px_12px_rgba(26,31,54,0.08)]">
      {toggle}
      {open && (
        <div className="grid grid-cols-[auto_auto] items-center gap-x-3 gap-y-1.5 pb-0.5">
          {controls.map(([keys, action]) => (
            <div key={action} className="contents">
              <span className="flex gap-1">
                {keys.map((key) => (
                  <kbd
                    key={key}
                    className="rounded border border-control-border bg-canvas px-1.5 font-mono text-[11px] leading-[18px] text-ink"
                  >
                    {key}
                  </kbd>
                ))}
              </span>
              <span>{action}</span>
            </div>
          ))}
        </div>
      )}
    </div>
  )
}
