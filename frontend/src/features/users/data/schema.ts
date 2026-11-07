import { ROLES } from '@/lib/route-access'
import { type ApiUser } from '@/stores/auth-store'

/** The three-tier role, wire spelling (spec §5.1), from the shared vocabulary. */
export const USER_ROLES = ROLES
export type UserRole = (typeof USER_ROLES)[number]

/** `is_enabled` as the list filter sees it (spec §5.4). */
export const USER_STATUSES = ['enabled', 'disabled'] as const
export type UserStatus = (typeof USER_STATUSES)[number]

/** The backend `User` wire shape, with the role narrowed to its enum. */
export interface User extends Omit<ApiUser, 'role'> {
  role: UserRole
}

/** `GET /users` response (spec §5.4). */
export interface UsersListResponse {
  data: User[]
  meta: {
    total: number
    page: number
    pageSize: number
  }
}

/** `POST /users` response (spec §5.3): the one-time password rides here. */
export interface CreateUserResponse {
  user: User
  initialPassword: string
}

/** Default list order (spec §5.4: `createdAt.desc`). */
export const DEFAULT_USERS_SORT = 'createdAt.desc'

/** The list query the users page sends (spec §5.4). */
export interface UsersListParams {
  page: number
  pageSize: number
  sort: string
  role?: UserRole[]
  status?: UserStatus
  q?: string
}
