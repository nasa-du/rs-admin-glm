import axios, { type AxiosError, type InternalAxiosRequestConfig } from 'axios'
import { useAuthStore } from '@/stores/auth-store'
import { refreshSession } from '@/lib/auth-refresh'

/**
 * The API client (spec §5.1): `/api` baseURL — same origin in production,
 * one Vite proxy rule in development; CORS is never configured (ADR 0001).
 *
 * Every request carries the in-memory access token, plus the
 * `X-Requested-With` header the cookie-eating endpoints (refresh / logout)
 * require as the CSRF second line of defense (spec §4.1).
 */
export const api = axios.create({
  baseURL: '/api',
})

api.interceptors.request.use((config) => {
  const { accessToken } = useAuthStore.getState()
  if (accessToken) {
    config.headers.set('Authorization', `Bearer ${accessToken}`)
  }
  config.headers.set('X-Requested-With', 'XMLHttpRequest')
  return config
})

/**
 * One marker per request: a request replayed after a successful refresh is
 * never retried again, even if it 401s a second time (spec §6.2.1).
 */
interface RetriableConfig extends InternalAxiosRequestConfig {
  _retry?: boolean
}

/** A login 401 means bad credentials, not an expired session. */
function isLoginRequest(config: InternalAxiosRequestConfig): boolean {
  return (config.url ?? '').endsWith('/auth/login')
}

/**
 * The refresh cookie is gone or dead: drop the in-memory session and return
 * to sign-in, remembering where the user was (spec §6.2.1). A hard
 * navigation is intentional — the whole app tree is stale at this point, and
 * the sign-in page then hands `redirect` back to the form.
 */
function endSession(): void {
  useAuthStore.getState().reset()
  const { pathname, search, hash } = window.location
  if (pathname === '/sign-in') return
  const redirect = encodeURIComponent(`${pathname}${search}${hash}`)
  window.location.replace(`/sign-in?redirect=${redirect}`)
}

/**
 * 401 interceptor: refresh once (single-flight, cross-tab serialized via Web
 * Locks), replay the original request once, and give up on a second 401.
 * Any other error passes straight through; 500 / 403 handling stays in the
 * query cache (main.tsx).
 */
api.interceptors.response.use(
  (response) => response,
  async (error: AxiosError) => {
    const config = error.config as RetriableConfig | undefined

    if (error.response?.status !== 401 || !config || isLoginRequest(config)) {
      return Promise.reject(error)
    }

    // The replay itself 401ed even after a fresh refresh: never retry again,
    // and end the dead session instead of leaving the shell up tokenless.
    if (config._retry) {
      endSession()
      return Promise.reject(error)
    }

    config._retry = true

    try {
      await refreshSession()
    } catch (refreshError) {
      endSession()
      return Promise.reject(refreshError)
    }

    return api(config)
  }
)
