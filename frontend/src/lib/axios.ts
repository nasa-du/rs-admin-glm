import axios from 'axios'
import { useAuthStore } from '@/stores/auth-store'

/**
 * The API client (spec §5.1): `/api` baseURL — same origin in production,
 * one Vite proxy rule in development; CORS is never configured (ADR 0001).
 *
 * Every request carries the in-memory access token, plus the
 * `X-Requested-With` header the cookie-eating endpoints (refresh / logout)
 * require as the CSRF second line of defense (spec §4.1). The 401
 * single-flight refresh interceptor arrives with the session-persistence
 * ticket.
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
