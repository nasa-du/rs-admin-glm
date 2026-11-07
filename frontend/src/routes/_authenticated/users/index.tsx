import { z } from 'zod'
import { createFileRoute } from '@tanstack/react-router'
import { Users } from '@/features/users'
import { USER_ROLES, USER_STATUSES } from '@/features/users/data/schema'
import { ROUTE_ACCESS } from '@/lib/route-access'

/**
 * The users page keeps every table state in the URL (spec §6.3): pagination,
 * sort, role / status filters and search survive a bookmark or a hard
 * refresh. Defaults stay out of the URL (page 1, pageSize 10, createdAt.desc).
 */
const usersSearchSchema = z.object({
  page: z.number().int().min(1).optional().catch(undefined),
  pageSize: z.number().int().min(1).max(100).optional().catch(undefined),
  // The server owns the authoritative whitelist (§5.4); mirroring it here
  // keeps a hand-edited bookmark from turning into a silent 422/empty list.
  sort: z
    .string()
    .regex(/^(email|displayName|role|isEnabled|createdAt|updatedAt)\.(asc|desc)$/)
    .optional()
    .catch(undefined),
  role: z.array(z.enum(USER_ROLES)).optional().catch([]),
  status: z.enum(USER_STATUSES).optional().catch(undefined),
  q: z.string().optional().catch(''),
})

export type UsersSearch = z.infer<typeof usersSearchSchema>

export const Route = createFileRoute('/_authenticated/users/')({
  // The guard reads this via the shared role → visible-route table.
  staticData: ROUTE_ACCESS['/users'],
  validateSearch: usersSearchSchema,
  component: Users,
})
