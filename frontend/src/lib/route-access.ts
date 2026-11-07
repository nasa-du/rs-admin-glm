/**
 * The single static role → visible-route table (spec §3 / §6.2.7).
 *
 * Both enforcement points read this table:
 *  - the `_authenticated` route guard checks the deepest matched route's
 *    `staticData.minRole` (route files reference the entry here);
 *  - the sidebar filters its nav items through `canAccessRoute`.
 *
 * Keeping one table means a route can never be hidden in the sidebar while
 * still being reachable, or vice versa.
 */

/** The three roles in descending rank order — the one role vocabulary. */
export const ROLES = ['superadmin', 'admin', 'user'] as const

export type AppRole = (typeof ROLES)[number]

/** Hierarchical rank of the single-value role (CONTEXT.md 角色): 3 ⊃ 2 ⊃ 1. */
export const ROLE_RANK = {
  superadmin: 3,
  admin: 2,
  user: 1,
} as const satisfies Record<AppRole, number>

export interface RouteAccess {
  /** Minimum role required to view the route. */
  minRole: AppRole
}

/**
 * Every page route the guard and sidebar know about. Routes absent from the
 * table default to the `user` tier.
 */
export const ROUTE_ACCESS = {
  '/': { minRole: 'user' },
  '/users': { minRole: 'admin' },
  '/settings': { minRole: 'user' },
  '/settings/account': { minRole: 'user' },
  '/settings/appearance': { minRole: 'user' },
} satisfies Record<string, RouteAccess>

export function isAppRole(role: string | undefined): role is AppRole {
  return (
    role !== undefined &&
    Object.prototype.hasOwnProperty.call(ROLE_RANK, role)
  )
}

/** True when `role` is at or above `minRole`. Unknown roles are denied. */
export function roleAtLeast(
  role: string | undefined,
  minRole: AppRole
): boolean {
  return isAppRole(role) && ROLE_RANK[role] >= ROLE_RANK[minRole]
}

/**
 * Manage-down (matching the backend's `Role::strictly_outranks`): the actor's
 * role must be strictly above the target's.
 */
export function roleStrictlyOutranks(
  actorRole: string | undefined,
  targetRole: AppRole
): boolean {
  return isAppRole(actorRole) && ROLE_RANK[actorRole] > ROLE_RANK[targetRole]
}

/** The access entry for a pathname; unlisted routes need the `user` tier. */
export function accessForRoute(pathname: string): RouteAccess {
  return (
    (ROUTE_ACCESS as Record<string, RouteAccess>)[pathname] ?? {
      minRole: 'user',
    }
  )
}

/** Sidebar filter: can this role see the nav entry pointing at `pathname`? */
export function canAccessRoute(
  role: string | undefined,
  pathname: string
): boolean {
  return roleAtLeast(role, accessForRoute(pathname).minRole)
}

/**
 * The highest `minRole` among the matched routes. `beforeLoad` receives the
 * full match array, so a child route's `staticData` participates in the
 * guard even though the check lives on the `_authenticated` parent.
 */
export function requiredRoleForMatches(
  matches: ReadonlyArray<{ staticData?: { minRole?: AppRole } }>
): AppRole {
  return matches.reduce<AppRole>((highest, match) => {
    const minRole = match.staticData?.minRole
    if (minRole && ROLE_RANK[minRole] > ROLE_RANK[highest]) {
      return minRole
    }
    return highest
  }, 'user')
}

declare module '@tanstack/react-router' {
  interface StaticDataRouteOption {
    /** Route-level role gate read by the `_authenticated` guard. */
    minRole?: AppRole
  }
}
