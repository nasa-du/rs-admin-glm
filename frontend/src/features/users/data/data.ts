import {
  Shield,
  UserCheck,
  Users,
  type LucideIcon,
} from 'lucide-react'
import { roleStrictlyOutranks } from '@/lib/route-access'
import { USER_ROLES, type UserRole, type UserStatus } from './schema'

/** Role icons; labels live in the locales under `roles.*`. */
export const roleOptions: {
  value: UserRole
  icon: LucideIcon
}[] = [
  { value: 'superadmin', icon: Shield },
  { value: 'admin', icon: UserCheck },
  { value: 'user', icon: Users },
]

/** Badge colours for the enabled / disabled status. */
export const statusBadgeClasses: Record<UserStatus, string> = {
  enabled:
    'bg-teal-100/30 text-teal-900 dark:text-teal-200 border-teal-200',
  disabled:
    'bg-destructive/10 dark:bg-destructive/50 text-destructive dark:text-primary border-destructive/10',
}

/** Roles the actor may create: strictly below their own rank (spec §3). */
export function creatableRoles(actorRole: string | undefined): UserRole[] {
  return USER_ROLES.filter((role) => roleStrictlyOutranks(actorRole, role))
}
