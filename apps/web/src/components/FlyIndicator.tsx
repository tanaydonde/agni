/**
 * Shown while a 3D view has the pointer captured for flying: an accent ring
 * around the view and a dot marking its middle.
 */
export function FlyIndicator() {
  return (
    <>
      <div aria-hidden className="pointer-events-none absolute inset-0 rounded-lg ring-2 ring-inset ring-accent" />
      <div
        aria-hidden
        className="pointer-events-none absolute top-1/2 left-1/2 size-1.5 -translate-1/2 rounded-full bg-white ring-1 ring-ink/60"
      />
    </>
  )
}
