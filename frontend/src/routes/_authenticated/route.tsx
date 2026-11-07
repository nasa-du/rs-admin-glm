import { createFileRoute, redirect } from '@tanstack/react-router'
import { useAuthStore } from '@/stores/auth-store'
import { refreshSession } from '@/lib/auth-refresh'
import { AuthenticatedLayout } from '@/components/layout/authenticated-layout'

/**
 * Guard layer 1 (spec §6.2.6): the access token is memory-only, so a hard
 * refresh (or a new tab) arrives without one. Hydrate from the refresh
 * cookie before deciding the route is unreachable; a failed refresh means a
 * dead session and a trip to sign-in carrying the return path.
 *
 * The forced password change and role-rank layers land with M5.
 */
export const Route = createFileRoute('/_authenticated')({
  beforeLoad: async ({ location }) => {
    if (useAuthStore.getState().accessToken) return

    try {
      await refreshSession()
    } catch {
      throw redirect({
        to: '/sign-in',
        search: { redirect: location.href },
      })
    }
  },
  component: AuthenticatedLayout,
})
