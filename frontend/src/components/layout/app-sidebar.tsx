import { useTranslation } from 'react-i18next'
import { useAuthStore, useCurrentRole } from '@/stores/auth-store'
import { useLayout } from '@/context/layout-provider'
import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarHeader,
  SidebarRail,
} from '@/components/ui/sidebar'
import { AppTitle } from './app-title'
import { getSidebarNavGroups } from './data/sidebar-data'
import { NavGroup } from './nav-group'
import { NavUser } from './nav-user'

export function AppSidebar() {
  const { collapsible, variant } = useLayout()
  const { t } = useTranslation()
  const user = useAuthStore((state) => state.user)
  const role = useCurrentRole()

  // Sidebar and route guard share the static role → visible-route table
  // (spec §6.2.7): a regular user never sees the users entry.
  const navGroups = getSidebarNavGroups(role, (key) => t(key))

  return (
    <Sidebar collapsible={collapsible} variant={variant}>
      <SidebarHeader>
        <AppTitle />
      </SidebarHeader>
      <SidebarContent>
        {navGroups.map((props) => (
          <NavGroup key={props.title} {...props} />
        ))}
      </SidebarContent>
      <SidebarFooter>
        <NavUser
          user={{
            name: user?.displayName ?? '',
            email: user?.email ?? '',
          }}
        />
      </SidebarFooter>
      <SidebarRail />
    </Sidebar>
  )
}
