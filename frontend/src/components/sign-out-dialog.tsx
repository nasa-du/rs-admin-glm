import { useLocation, useNavigate } from '@tanstack/react-router'
import { useTranslation } from 'react-i18next'
import { useAuthStore } from '@/stores/auth-store'
import { api } from '@/lib/axios'
import { ConfirmDialog } from '@/components/confirm-dialog'

interface SignOutDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
}

export function SignOutDialog({ open, onOpenChange }: SignOutDialogProps) {
  const navigate = useNavigate()
  const location = useLocation()
  const reset = useAuthStore((state) => state.reset)
  const { t } = useTranslation()

  const handleSignOut = async () => {
    // Revoke this browser's token family server-side (spec §4.2): other
    // browsers keep their own families. A failed call must not strand the
    // user in a signed-in-looking UI, so the local session always goes.
    try {
      await api.post('/auth/logout')
    } catch {
      // Local sign-out wins; the cookie expires on its own otherwise.
    } finally {
      reset()
      // Preserve current location for redirect after sign-in.
      const currentPath = location.href
      navigate({
        to: '/sign-in',
        search: { redirect: currentPath },
        replace: true,
      })
    }
  }

  return (
    <ConfirmDialog
      open={open}
      onOpenChange={onOpenChange}
      title={t('signOut.title')}
      desc={t('signOut.description')}
      confirmText={t('signOut.confirm')}
      destructive
      handleConfirm={handleSignOut}
      className='sm:max-w-sm'
    />
  )
}
