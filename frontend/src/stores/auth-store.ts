import { create } from 'zustand'

/**
 * Auth session store — memory only (ADR 0001 / spec §6.2.4).
 *
 * The template's cookie persist middleware and its hardcoded key are gone
 * entirely: the access token lives in memory and dies with the tab. A hard
 * refresh lands back on sign-in until the session-persistence ticket adds
 * refresh-based hydration; that is the expected behavior at this stage.
 */

/** The backend `User` wire shape (spec §5.1). */
export interface ApiUser {
  id: string
  email: string
  displayName: string
  role: string
  isEnabled: boolean
  mustChangePassword: boolean
  createdAt: string
  updatedAt: string
}

/** `POST /auth/login` success body. */
export interface LoginResponse {
  accessToken: string
  user: ApiUser
}

/**
 * The user as the frontend keeps it: backend fields, with the single-value
 * role wrapped as `[role]` to fit the template's array shape (spec §5.1,
 * length is always 1).
 */
export interface AuthUser extends Omit<ApiUser, 'role'> {
  role: string[]
}

interface AuthState {
  user: AuthUser | null
  accessToken: string
  /** Store a successful login / refresh response (wraps the role). */
  setSession: (payload: LoginResponse) => void
  /** Drop the in-memory session (sign-out, expired refresh). */
  reset: () => void
}

export const useAuthStore = create<AuthState>()((set) => ({
  user: null,
  accessToken: '',
  setSession: ({ accessToken, user }) =>
    set({
      accessToken,
      user: { ...user, role: [user.role] },
    }),
  reset: () => set({ user: null, accessToken: '' }),
}))
