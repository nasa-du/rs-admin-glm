import { UsersCreateDialog } from './users-create-dialog'
import { useUsers } from './users-provider'

export function UsersDialogs() {
  const { open, setOpen } = useUsers()

  return (
    <UsersCreateDialog
      key='user-create'
      open={open === 'create'}
      onOpenChange={(state) => setOpen(state ? 'create' : null)}
    />
  )
}
