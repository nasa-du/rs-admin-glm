import { createFileRoute, redirect } from '@tanstack/react-router'
import { refreshSession } from '@/lib/auth-refresh'
import {
  requiredRoleForMatches,
  roleAtLeast,
} from '@/lib/route-access'
import { getCurrentRole, useAuthStore } from '@/stores/auth-store'
import { AuthenticatedLayout } from '@/components/layout/authenticated-layout'

/**
 * The three guard layers (spec §6.2.6):
 *
 * 1. Hydration — the access token is memory-only, so a hard refresh (or a
 *    new tab) arrives without one. Refresh from the HttpOnly cookie before
 *    deciding the route is unreachable; a failed refresh means a dead
 *    session and a trip to sign-in carrying the return path.
 * 2. Forced password change lands with the settings ticket (M5b).
 * 3. Role rank — the deepest matched route's `staticData.minRole` comes from
 *    the shared role → visible-route table; a role below it gets the 403
 *    page. Reaching the route directly is therefore no different from
 *    following a (hidden) sidebar entry.
 */
export const Route = createFileRoute('/_authenticated')({
  beforeLoad: async ({ location, matches }) => {
    if (!useAuthStore.getState().accessToken) {
      try {
        await refreshSession()
      } catch {
        throw redirect({
          to: '/sign-in',
          search: { redirect: location.href },
        })
      }
    }

    const role = getCurrentRole()
    const required = requiredRoleForMatches(matches)
    if (!roleAtLeast(role, required)) {
      throw redirect({ to: '/403', replace: true })
    }
  },
  component: AuthenticatedLayout,
})
