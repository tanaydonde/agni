import type { ReactNode } from 'react'

/** The bordered panel every page's 3D view sits in. */
export function ViewportFrame({ children }: { children: ReactNode }) {
  return (
    <section
      aria-label="3D scene"
      className="relative min-h-0 min-w-0 flex-1 overflow-hidden rounded-lg border border-hairline bg-white"
    >
      {children}
    </section>
  )
}

export function ViewportMessage({ children }: { children: ReactNode }) {
  return (
    <div className="flex h-full items-center justify-center p-8">
      <p className="max-w-md text-center leading-normal text-muted">{children}</p>
    </div>
  )
}

/** A titled message over the view: nothing run yet, running, or failed. */
export function StageMessage({ title, children }: { title: string; children: ReactNode }) {
  return (
    <div className="absolute inset-0 z-10 flex items-center justify-center bg-white/85 p-8">
      <div className="flex max-w-sm flex-col items-center gap-3 text-center text-sm leading-normal text-muted">
        <h2 className="text-base font-semibold text-ink">{title}</h2>
        <div className="flex flex-col items-center gap-3">{children}</div>
      </div>
    </div>
  )
}

export function PrimaryButton({
  onClick,
  disabled = false,
  children,
}: {
  onClick: () => void
  disabled?: boolean
  children: ReactNode
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      className="rounded-md border border-accent bg-accent px-4 py-2 text-sm font-medium text-white hover:border-accent-strong hover:bg-accent-strong disabled:cursor-not-allowed disabled:opacity-60"
    >
      {children}
    </button>
  )
}
