import { UsersCreateDialog } from './users-create-dialog'
import { UsersDeleteDialog } from './users-delete-dialog'
import { UsersDisableDialog } from './users-disable-dialog'
import { UsersEditDialog } from './users-edit-dialog'
import { useUsers } from './users-provider'
import { UsersResetDialog } from './users-reset-dialog'

/**
 * The dialog set of the users page (spec §6.3): create plus the four
 * lifecycle dialogs — edit / disable / delete / password-reset. All but
 * create target `currentRow`, set by the row action menu.
 */
export function UsersDialogs() {
  const { open, setOpen } = useUsers()

  return (
    <>
      <UsersCreateDialog
        key='user-create'
        open={open === 'create'}
        onOpenChange={(state) => setOpen(state ? 'create' : null)}
      />
      <UsersEditDialog
        key='user-edit'
        open={open === 'edit'}
        onOpenChange={(state) => setOpen(state ? 'edit' : null)}
      />
      <UsersDisableDialog
        key='user-disable'
        open={open === 'disable'}
        onOpenChange={(state) => setOpen(state ? 'disable' : null)}
      />
      <UsersDeleteDialog
        key='user-delete'
        open={open === 'delete'}
        onOpenChange={(state) => setOpen(state ? 'delete' : null)}
      />
      <UsersResetDialog
        key='user-reset'
        open={open === 'reset'}
        onOpenChange={(state) => setOpen(state ? 'reset' : null)}
      />
    </>
  )
}
