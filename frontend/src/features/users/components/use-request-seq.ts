import { useMemo, useRef } from 'react'

/**
 * Stale-response guard for dialogs that surface a one-time secret (spec
 * §2.4): closing or re-opening the dialog while a request is in flight must
 * never let the late response's secret appear in the next dialog session.
 * Each request is stamped at start; a stamp that has been superseded
 * (another request started, or the dialog reset) discards its response.
 *
 * The returned object is stable across renders (like the ref it wraps), so
 * it is safe as a `useCallback` dependency.
 */
export function useRequestSeq() {
  const seq = useRef(0)

  return useMemo(
    () => ({
      /** Stamp a new in-flight request and open a new dialog session. */
      next: () => ++seq.current,
      /** Has `stamp` not been superseded? */
      isCurrent: (stamp: number) => stamp === seq.current,
      /** Supersede every outstanding stamp (dialog closed / reset). */
      invalidate: () => {
        seq.current += 1
      },
    }),
    []
  )
}
