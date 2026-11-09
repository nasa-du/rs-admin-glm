import { useEffect, useMemo, useState } from 'react'
import { z } from 'zod'
import { useForm } from 'react-hook-form'
import { zodResolver } from '@hookform/resolvers/zod'
import { useQueryClient } from '@tanstack/react-query'
import { Loader2, Pencil, UserCog } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { useCurrentRole } from '@/stores/auth-store'
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
import {
  Form,
  FormControl,
  FormDescription,
  FormField,
  FormItem,
  FormLabel,
  FormMessage,
} from '@/components/ui/form'
import { Input } from '@/components/ui/input'
import { SelectDropdown } from '@/components/select-dropdown'
import { creatableRoles } from '../data/data'
import { USER_ROLES, type User, type UserRole } from '../data/schema'
import { useUsers } from './users-provider'

type UsersEditDialogProps = {
  open: boolean
  onOpenChange: (open: boolean) => void
}

/**
 * Edit-user dialog (spec §6.3): display name for every manager, plus the
 * role select for superadmins only — role assignment is superadmin-exclusive
 * and strictly below the operator's own rank (spec §3). The login identifier
 * is not editable here.
 */
export function UsersEditDialog({ open, onOpenChange }: UsersEditDialogProps) {
  const { t } = useTranslation()
  const queryClient = useQueryClient()
  const actorRole = useCurrentRole()
  const { currentRow } = useUsers()
  const user: User | null = currentRow

  // Role changes are superadmin-exclusive (spec §3).
  const canAssignRole = actorRole === 'superadmin'
  const roles = useMemo(() => creatableRoles(actorRole), [actorRole])

  const [isPending, setIsPending] = useState(false)

  const formSchema = useMemo(
    () =>
      z.object({
        displayName: z
          .string()
          .trim()
          .min(1, t('users.validation.displayNameRequired'))
          .max(32, t('users.validation.displayNameTooLong')),
        role: z.enum(USER_ROLES),
      }),
    [t]
  )
  type FormValues = z.infer<typeof formSchema>

  const form = useForm<FormValues>({
    resolver: zodResolver(formSchema),
    defaultValues: {
      displayName: user?.displayName ?? '',
      role: user?.role ?? 'user',
    },
  })

  // Re-seed the form from the list row every time the dialog opens (spec
  // §5.3: the edit dialog takes its data from the row, there is no detail
  // endpoint).
  useEffect(() => {
    if (open && user) {
      form.reset({ displayName: user.displayName, role: user.role })
    }
    // form.reset is stable; user identity changes with the row.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, user])

  if (!user) return null

  const submit = async (values: FormValues) => {
    setIsPending(true)
    try {
      const { data } = await api.patch<{ user: User }>(
        `/users/${user.id}`,
        canAssignRole
          ? { displayName: values.displayName, role: values.role }
          : { displayName: values.displayName }
      )
      toast.success(t('users.toasts.updated', { email: data.user.email }))
      void queryClient.invalidateQueries({ queryKey: ['users'] })
      onOpenChange(false)
    } catch (error) {
      toast.error(getApiErrorMessage(error, t))
    } finally {
      setIsPending(false)
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className='sm:max-w-md'>
        <DialogHeader className='text-start'>
          <DialogTitle className='flex items-center gap-2'>
            <UserCog size={18} /> {t('users.editDialog.title')}
          </DialogTitle>
          <DialogDescription>
            {t('users.editDialog.description', { email: user.email })}
          </DialogDescription>
        </DialogHeader>
        <Form {...form}>
          <form
            id='user-edit-form'
            onSubmit={form.handleSubmit(submit)}
            className='space-y-4'
          >
            <FormField
              control={form.control}
              name='displayName'
              render={({ field }) => (
                <FormItem>
                  <FormLabel>{t('users.createDialog.displayName')}</FormLabel>
                  <FormControl>
                    <Input autoComplete='off' {...field} />
                  </FormControl>
                  <FormMessage />
                </FormItem>
              )}
            />
            {canAssignRole && (
              <FormField
                control={form.control}
                name='role'
                render={({ field }) => (
                  <FormItem>
                    <FormLabel>{t('users.createDialog.role')}</FormLabel>
                    <SelectDropdown
                      isControlled
                      defaultValue={field.value}
                      onValueChange={field.onChange}
                      placeholder={t('users.createDialog.role')}
                      items={roles.map((role: UserRole) => ({
                        value: role,
                        label: t(`roles.${role}`),
                      }))}
                    />
                    <FormDescription>
                      {t('users.editDialog.roleHint')}
                    </FormDescription>
                    <FormMessage />
                  </FormItem>
                )}
              />
            )}
          </form>
        </Form>
        <DialogFooter>
          <Button type='submit' form='user-edit-form' disabled={isPending}>
            {isPending ? <Loader2 className='animate-spin' /> : <Pencil />}
            {isPending
              ? t('users.editDialog.saving')
              : t('users.editDialog.submit')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
