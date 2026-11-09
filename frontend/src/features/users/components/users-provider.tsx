import React from 'react'
import useDialogState from '@/hooks/use-dialog-state'
import { type User } from '../data/schema'

type UsersDialogType = 'create' | 'edit' | 'disable' | 'delete' | 'reset'

type UsersContextType = {
  open: UsersDialogType | null
  setOpen: (str: UsersDialogType | null) => void
  /** The row the management dialog targets; `null` for create. */
  currentRow: User | null
  setCurrentRow: (row: User | null) => void
}

const UsersContext = React.createContext<UsersContextType | null>(null)

export function UsersProvider({ children }: { children: React.ReactNode }) {
  const [open, setOpen] = useDialogState<UsersDialogType>(null)
  const [currentRow, setCurrentRow] = React.useState<User | null>(null)

  return (
    <UsersContext value={{ open, setOpen, currentRow, setCurrentRow }}>
      {children}
    </UsersContext>
  )
}

// eslint-disable-next-line react-refresh/only-export-components
export const useUsers = () => {
  const usersContext = React.useContext(UsersContext)

  if (!usersContext) {
    throw new Error('useUsers has to be used within <UsersContext>')
  }

  return usersContext
}
