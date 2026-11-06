import { z } from 'zod'

const userStatusSchema = z.union([
  z.literal('active'),
  z.literal('inactive'),
  z.literal('invited'),
  z.literal('suspended'),
])
export type UserStatus = z.infer<typeof userStatusSchema>

// Canonical three-tier hierarchy (CONTEXT.md 角色): superadmin ⊃ admin ⊃ user
const userRoleSchema = z.union([
  z.literal('superadmin'),
  z.literal('admin'),
  z.literal('user'),
])

// Prefix `_` marks the schema as type-inference-only (mock data; reworked when the API lands)
const _userSchema = z.object({
  id: z.string(),
  firstName: z.string(),
  lastName: z.string(),
  username: z.string(),
  email: z.string(),
  phoneNumber: z.string(),
  status: userStatusSchema,
  role: userRoleSchema,
  createdAt: z.coerce.date(),
  updatedAt: z.coerce.date(),
})
export type User = z.infer<typeof _userSchema>
