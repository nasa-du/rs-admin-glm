import {
  Ban,
  CircleCheck,
  KeyRound,
  MoreHorizontal,
  Pencil,
  Trash2,
} from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { useCurrentRole } from '@/stores/auth-store'
import { roleStrictlyOutranks } from '@/lib/route-access'
import { Button } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { type User } from '../data/schema'
import { useUsers } from './users-provider'

type UsersRowActionsProps = {
  user: User
}

/**
 * The per-row action menu (spec §6.3): edit / disable / reset follow
 * manage-down — strictly below the operator's rank, which also hides the
 * operator's own row — and delete is superadmin-only. Rows the operator
 * cannot manage (peers, superiors, self) render no menu at all: 对同级及更
 * 高角色至多可见、不可操作.
 */
export function UsersRowActions({ user }: UsersRowActionsProps) {
  const { t } = useTranslation()
  const actorRole = useCurrentRole()
  const { setOpen, setCurrentRow } = useUsers()

  const canManage = roleStrictlyOutranks(actorRole, user.role)
  const canDelete = actorRole === 'superadmin'

  if (!canManage) return null

  const openDialog = (dialog: 'edit' | 'disable' | 'delete' | 'reset') => {
    setCurrentRow(user)
    setOpen(dialog)
  }

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          variant='ghost'
          className='data-[state=open]:bg-muted flex size-8 p-0'
          aria-label={t('users.actions.open', {
            displayName: user.displayName,
          })}
        >
          <MoreHorizontal size={16} className='pointer-events-none' />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align='end' className='w-44'>
        <DropdownMenuItem onClick={() => openDialog('edit')}>
          <Pencil className='mr-2 size-4' />
          {t('users.actions.edit')}
        </DropdownMenuItem>
        <DropdownMenuItem onClick={() => openDialog('disable')}>
          {user.isEnabled ? (
            <Ban className='mr-2 size-4' />
          ) : (
            <CircleCheck className='mr-2 size-4' />
          )}
          {user.isEnabled
            ? t('users.actions.disable')
            : t('users.actions.enable')}
        </DropdownMenuItem>
        <DropdownMenuItem onClick={() => openDialog('reset')}>
          <KeyRound className='mr-2 size-4' />
          {t('users.actions.resetPassword')}
        </DropdownMenuItem>
        {canDelete && (
          <>
            <DropdownMenuSeparator />
            <DropdownMenuItem
              variant='destructive'
              onClick={() => openDialog('delete')}
            >
              <Trash2 className='mr-2 size-4' />
              {t('users.actions.delete')}
            </DropdownMenuItem>
          </>
        )}
      </DropdownMenuContent>
    </DropdownMenu>
  )
}
