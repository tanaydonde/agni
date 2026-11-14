/** Key or gesture names, and what they do. */
export type Controls = [keys: string[], action: string][]

/** The floating legend of controls in the corner of a 3D view. */
export function ControlsHint({ title, controls }: { title: string; controls: Controls }) {
  return (
    <div className="pointer-events-none absolute bottom-3 left-3 flex flex-col gap-2 rounded-lg border border-hairline bg-white px-3 py-2.5 text-xs text-ink-secondary shadow-[0_4px_12px_rgba(26,31,54,0.08)]">
      <span className="font-medium text-ink">{title}</span>
      <div className="grid grid-cols-[auto_auto] items-center gap-x-3 gap-y-1.5">
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
    </div>
  )
}
