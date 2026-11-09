import { useState } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { getApiErrorMessage } from '@/lib/api-error'
import { api } from '@/lib/axios'
import { ConfirmDialog } from '@/components/confirm-dialog'
import { type User } from '../data/schema'
import { useUsers } from './users-provider'

type UsersDisableDialogProps = {
  open: boolean
  onOpenChange: (open: boolean) => void
}

/**
 * Disable / enable confirmation (spec §6.3). Disabling is the reversible
 * alternative to deletion: it ends every session of the target at their next
 * refresh, at the latest within the 15-minute access-token window (spec §4.2).
 */
export function UsersDisableDialog({
  open,
  onOpenChange,
}: UsersDisableDialogProps) {
  const { t } = useTranslation()
  const queryClient = useQueryClient()
  const { currentRow } = useUsers()
  const user: User | null = currentRow
  const [isPending, setIsPending] = useState(false)

  if (!user) return null

  const disabling = !user.isEnabled

  const handleConfirm = async () => {
    setIsPending(true)
    try {
      const { data } = await api.patch<{ user: User }>(`/users/${user.id}`, {
        isEnabled: disabling,
      })
      toast.success(
        disabling
          ? t('users.toasts.disabled', { email: data.user.email })
          : t('users.toasts.enabled', { email: data.user.email })
      )
      void queryClient.invalidateQueries({ queryKey: ['users'] })
      onOpenChange(false)
    } catch (error) {
      toast.error(getApiErrorMessage(error, t))
    } finally {
      setIsPending(false)
    }
  }

  return (
    <ConfirmDialog
      open={open}
      onOpenChange={onOpenChange}
      title={
        disabling
          ? t('users.disableDialog.title')
          : t('users.disableDialog.enableTitle')
      }
      desc={
        disabling
          ? t('users.disableDialog.description', { email: user.email })
          : t('users.disableDialog.enableDescription', { email: user.email })
      }
      confirmText={
        disabling
          ? t('users.disableDialog.confirm')
          : t('users.disableDialog.enableConfirm')
      }
      destructive={disabling}
      isLoading={isPending}
      handleConfirm={() => void handleConfirm()}
      className='sm:max-w-sm'
    />
  )
}
