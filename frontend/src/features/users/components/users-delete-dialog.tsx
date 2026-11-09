import { useState } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { getApiErrorMessage } from '@/lib/api-error'
import { api } from '@/lib/axios'
import { ConfirmDialog } from '@/components/confirm-dialog'
import { type User } from '../data/schema'
import { useUsers } from './users-provider'

type UsersDeleteDialogProps = {
  open: boolean
  onOpenChange: (open: boolean) => void
}

/**
 * Delete-user confirmation (spec §6.3): superadmin-only, hard delete — the
 * account and all of its sessions disappear; there is no undo, which is why
 * the action lives behind a destructive confirmation.
 */
export function UsersDeleteDialog({
  open,
  onOpenChange,
}: UsersDeleteDialogProps) {
  const { t } = useTranslation()
  const queryClient = useQueryClient()
  const { currentRow } = useUsers()
  const user: User | null = currentRow
  const [isPending, setIsPending] = useState(false)

  if (!user) return null

  const handleConfirm = async () => {
    setIsPending(true)
    try {
      await api.delete(`/users/${user.id}`)
      toast.success(t('users.toasts.deleted', { email: user.email }))
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
      title={t('users.deleteDialog.title')}
      desc={t('users.deleteDialog.description', {
        email: user.email,
        displayName: user.displayName,
      })}
      confirmText={t('users.deleteDialog.confirm')}
      destructive
      isLoading={isPending}
      handleConfirm={() => void handleConfirm()}
      className='sm:max-w-sm'
    />
  )
}
