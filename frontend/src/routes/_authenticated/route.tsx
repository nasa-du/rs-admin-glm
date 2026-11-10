import { createFileRoute, redirect } from '@tanstack/react-router'
import { refreshSession } from '@/lib/auth-refresh'
import {
  requiredRoleForMatches,
  roleAtLeast,
} from '@/lib/route-access'
import {
  getCurrentRole,
  getMustChangePassword,
  useAuthStore,
} from '@/stores/auth-store'
import { AuthenticatedLayout } from '@/components/layout/authenticated-layout'

/** The one authenticated page a must-change-password user may visit. */
const FORCED_PASSWORD_PATH = '/settings/password'

/**
 * The three guard layers (spec §6.2.6):
 *
 * 1. Hydration — the access token is memory-only, so a hard refresh (or a
 *    new tab) arrives without one. Refresh from the HttpOnly cookie before
 *    deciding the route is unreachable; a failed refresh means a dead
 *    session and a trip to sign-in carrying the return path.
 * 2. Forced password change — a user whose flag is armed (initial password
 *    still live) is redirected to the password page from every route except
 *    that page itself (sign-in is outside this guard). The initial
 *    password's mission ends there (issue #19).
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

    if (
      getMustChangePassword() &&
      !location.pathname.startsWith(FORCED_PASSWORD_PATH)
    ) {
      throw redirect({ to: FORCED_PASSWORD_PATH, replace: true })
    }

    const role = getCurrentRole()
    const required = requiredRoleForMatches(matches)
    if (!roleAtLeast(role, required)) {
      throw redirect({ to: '/403', replace: true })
    }
  },
  component: AuthenticatedLayout,
})
