import type { ReactNode } from 'react'

/** The frame both demo pages share: page actions, then the view. */
export function DemoShell({
  label,
  actions,
  children,
}: {
  label: string
  actions?: ReactNode
  children: ReactNode
}) {
  return (
    <main className="flex h-full min-h-[600px] min-w-[960px] flex-col gap-3 bg-canvas p-3">
      {actions && (
        <header aria-label={label} className="flex flex-none flex-wrap items-center gap-x-4 gap-y-2">
          {actions}
        </header>
      )}
      {children}
    </main>
  )
}

/** A two-or-more-way switch in the accent colour, like the editor's view switch. */
export function Switch<Value extends string>({
  label,
  options,
  value,
  onChange,
}: {
  label: string
  options: readonly (readonly [value: Value, label: string])[]
  value: Value
  onChange: (value: Value) => void
}) {
  return (
    <div
      role="radiogroup"
      aria-label={label}
      className="pointer-events-auto flex shrink-0 rounded-md border border-hairline bg-white p-0.5 text-xs font-medium shadow-[0_4px_12px_rgba(26,31,54,0.08)]"
    >
      {options.map(([option, text]) => (
        <button
          key={option}
          type="button"
          role="radio"
          aria-checked={value === option}
          onClick={() => onChange(option)}
          className={
            value === option
              ? 'rounded bg-accent px-2.5 py-1 text-white'
              : 'rounded px-2.5 py-1 text-ink-secondary hover:text-ink'
          }
        >
          {text}
        </button>
      ))}
    </div>
  )
}
