import axios from 'axios'
import { type SessionResponse, useAuthStore } from '@/stores/auth-store'

/**
 * Refresh plumbing shared by the axios 401 interceptor and the route guard
 * (spec §4.2 / §6.2, ADR 0003).
 *
 * The refresh token is an HttpOnly cookie, so the only way back from an
 * expired access token is this endpoint. Two hazards shape the design:
 *
 * 1. Concurrent 401s in one tab must not each rotate the cookie — hence the
 *    module-level single-flight promise.
 * 2. Two tabs expiring at the same time must not refresh concurrently either:
 *    both would present the same cookie, the loser would trip reuse
 *    detection, and the whole family — both tabs — would be revoked. The
 *    `navigator.locks` request wraps the HTTP call itself, so the tab that
 *    goes second sends the cookie the first tab's rotation just wrote.
 */

/** Cross-tab lock name (ADR 0003.2). */
const AUTH_REFRESH_LOCK = 'auth-refresh'

/**
 * A bare axios call, not the `api` instance: the refresh endpoint must never
 * re-enter the 401 interceptor it feeds. Same-origin, so the HttpOnly cookie
 * rides along; the header is the CSRF second line of defense (spec §4.1).
 */
async function requestRefresh(): Promise<SessionResponse> {
  const { data } = await axios.post<SessionResponse>(
    '/api/auth/refresh',
    null,
    {
      withCredentials: true,
      headers: { 'X-Requested-With': 'XMLHttpRequest' },
    }
  )
  useAuthStore.getState().setSession(data)
  return data
}

let inflight: Promise<SessionResponse> | null = null

/**
 * Refresh the session once per tab at a time, serialized across tabs.
 * Rejects when the cookie is missing / expired / revoked; callers decide
 * whether that means "route to sign-in".
 */
export function refreshSession(): Promise<SessionResponse> {
  if (inflight) return inflight

  inflight = (async () => {
    try {
      if (typeof navigator !== 'undefined' && navigator.locks) {
        return await navigator.locks.request(AUTH_REFRESH_LOCK, requestRefresh)
      }
      // Insecure contexts have no Web Locks; single-flight still holds
      // within this tab.
      return await requestRefresh()
    } finally {
      inflight = null
    }
  })()

  return inflight
}
