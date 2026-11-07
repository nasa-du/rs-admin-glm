import {
  LayoutDashboard,
  Palette,
  Settings,
  UserCog,
  Users,
} from 'lucide-react'
import { canAccessRoute } from '@/lib/route-access'
import { type NavGroup, type NavItem } from '../types'

type NavItemDef = {
  titleKey: string
  url?: string
  icon?: React.ElementType
  items?: NavItemDef[]
}

type NavGroupDef = {
  titleKey: string
  items: NavItemDef[]
}

/**
 * Structural sidebar definition. Titles are locale keys, not copy; the
 * visibility gate is the shared `ROUTE_ACCESS` table (spec §6.2.7), applied
 * in `getSidebarNavGroups` below.
 */
const navGroups: NavGroupDef[] = [
  {
    titleKey: 'nav.general',
    items: [
      {
        titleKey: 'nav.dashboard',
        url: '/',
        icon: LayoutDashboard,
      },
      {
        titleKey: 'nav.users',
        url: '/users',
        icon: Users,
      },
    ],
  },
  {
    titleKey: 'nav.other',
    items: [
      {
        titleKey: 'nav.settings',
        icon: Settings,
        items: [
          {
            titleKey: 'nav.profile',
            url: '/settings',
            icon: UserCog,
          },
          {
            titleKey: 'nav.appearance',
            url: '/settings/appearance',
            icon: Palette,
          },
        ],
      },
    ],
  },
]

function localizeItem(
  item: NavItemDef,
  role: string | undefined,
  t: (key: string) => string
): NavItem | null {
  if (item.items) {
    const items = item.items
      .filter(
        (child): child is NavItemDef & { url: string } =>
          child.url !== undefined && canAccessRoute(role, child.url)
      )
      .map((child) => ({
        title: t(child.titleKey),
        url: child.url,
        icon: child.icon,
      }))
    // A parent with no visible children disappears too.
    return items.length > 0
      ? { title: t(item.titleKey), icon: item.icon, items }
      : null
  }

  if (!item.url || !canAccessRoute(role, item.url)) {
    return null
  }
  return {
    title: t(item.titleKey),
    url: item.url,
    icon: item.icon,
  }
}

/**
 * Translate the structural sidebar and drop every entry the current role may
 * not see. A regular user therefore never sees the users entry; the guard
 * independently refuses the same route.
 */
export function getSidebarNavGroups(
  role: string | undefined,
  t: (key: string) => string
): NavGroup[] {
  return navGroups
    .map((group) => ({
      title: t(group.titleKey),
      items: group.items
        .map((item) => localizeItem(item, role, t))
        .filter((item): item is NavItem => item !== null),
    }))
    .filter((group) => group.items.length > 0)
}
