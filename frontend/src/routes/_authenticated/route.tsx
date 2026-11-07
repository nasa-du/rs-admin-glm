import { createFileRoute, redirect } from '@tanstack/react-router'
import { useAuthStore } from '@/stores/auth-store'
import { AuthenticatedLayout } from '@/components/layout/authenticated-layout'

/**
 * Minimal guard for this milestone: an empty in-memory session goes back to
 * sign-in — the expected behavior on hard refresh until the
 * session-persistence ticket adds refresh-based hydration. The three-layer
 * guard (hydration → forced password change → role rank) lands with M5.
 */
export const Route = createFileRoute('/_authenticated')({
  beforeLoad: ({ location }) => {
    if (!useAuthStore.getState().user) {
      throw redirect({
        to: '/sign-in',
        search: { redirect: location.href },
      })
    }
  },
  component: AuthenticatedLayout,
})
