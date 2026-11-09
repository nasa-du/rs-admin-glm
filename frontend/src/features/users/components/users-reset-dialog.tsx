import { useState } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { KeyRound, Loader2 } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { getApiErrorMessage } from '@/lib/api-error'
import { api } from '@/lib/axios'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { type User } from '../data/schema'
import { OneTimePassword } from './one-time-password'
import { useRequestSeq } from './use-request-seq'
import { useUsers } from './users-provider'

type UsersResetDialogProps = {
  open: boolean
  onOpenChange: (open: boolean) => void
}

/**
 * Password-reset dialog (spec §6.3): confirm, then display the new one-time
 * initial password. The reset terminates all of the target's sessions and
 * re-arms the forced password change, but deliberately does not touch the
 * enabled flag — resetting is not un-disabling (spec §5.3).
 */
export function UsersResetDialog({
  open,
  onOpenChange,
}: UsersResetDialogProps) {
  const { t } = useTranslation()
  const queryClient = useQueryClient()
  const { currentRow } = useUsers()
  const user: User | null = currentRow

  const [initialPassword, setInitialPassword] = useState<string | null>(null)
  const [isPending, setIsPending] = useState(false)
  // Same stale-response guard as the create dialog: a response from a closed
  // dialog session must never surface its one-time password on the next open.
  const request = useRequestSeq()

  if (!user) return null

  const handleReset = async () => {
    const stamp = request.next()
    setIsPending(true)
    try {
      const { data } = await api.post<{ initialPassword: string }>(
        `/users/${user.id}/password-reset`
      )
      if (!request.isCurrent(stamp)) return
      // The one-time password lives only in this component's local state.
      setInitialPassword(data.initialPassword)
      void queryClient.invalidateQueries({ queryKey: ['users'] })
    } catch (error) {
      if (request.isCurrent(stamp)) {
        toast.error(getApiErrorMessage(error, t))
      }
    } finally {
      if (request.isCurrent(stamp)) setIsPending(false)
    }
  }

  const handleOpenChange = (nextOpen: boolean) => {
    if (!nextOpen) {
      request.invalidate()
      setInitialPassword(null)
      setIsPending(false)
    }
    onOpenChange(nextOpen)
  }

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <DialogContent className='sm:max-w-md'>
        {initialPassword ? (
          <>
            <DialogHeader className='text-start'>
              <DialogTitle>
                {t('users.resetDialog.credentialsTitle')}
              </DialogTitle>
              <DialogDescription>
                {t('users.resetDialog.resetFor', { email: user.email })}
              </DialogDescription>
            </DialogHeader>
            <OneTimePassword password={initialPassword} />
            <DialogFooter>
              <Button onClick={() => handleOpenChange(false)}>
                {t('users.createDialog.done')}
              </Button>
            </DialogFooter>
          </>
        ) : (
          <>
            <DialogHeader className='text-start'>
              <DialogTitle className='flex items-center gap-2'>
                <KeyRound size={18} /> {t('users.resetDialog.title')}
              </DialogTitle>
              <DialogDescription>
                {t('users.resetDialog.description', { email: user.email })}
              </DialogDescription>
            </DialogHeader>
            <DialogFooter>
              <Button
                variant='destructive'
                disabled={isPending}
                onClick={() => void handleReset()}
              >
                {isPending ? (
                  <Loader2 className='animate-spin' />
                ) : (
                  <KeyRound />
                )}
                {isPending
                  ? t('users.resetDialog.resetting')
                  : t('users.resetDialog.confirm')}
              </Button>
            </DialogFooter>
          </>
        )}
      </DialogContent>
    </Dialog>
  )
}
