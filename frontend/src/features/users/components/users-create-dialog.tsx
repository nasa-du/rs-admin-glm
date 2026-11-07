/* eslint-disable react-hooks/refs -- requestSeq is an async stale-response
   guard, read only after the request resolves, never during render. */
import { useCallback, useMemo, useRef, useState } from 'react'
import { z } from 'zod'
import { useForm } from 'react-hook-form'
import { zodResolver } from '@hookform/resolvers/zod'
import { useQueryClient } from '@tanstack/react-query'
import { Check, Copy, Loader2, UserPlus } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { getApiErrorMessage } from '@/lib/api-error'
import { api } from '@/lib/axios'
import { useCurrentRole } from '@/stores/auth-store'
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
import {
  USER_ROLES,
  type CreateUserResponse,
  type UserRole,
} from '../data/schema'

type UsersCreateDialogProps = {
  open: boolean
  onOpenChange: (open: boolean) => void
}

/**
 * Create-user dialog (spec §6.3).
 *
 * The success step displays the one-time initial password with a copy
 * button. The plaintext lives only in this component's local state — never
 * in a store, the query cache, or anywhere that outlives the dialog — and is
 * cleared as soon as the dialog closes.
 */
export function UsersCreateDialog({
  open,
  onOpenChange,
}: UsersCreateDialogProps) {
  const { t } = useTranslation()
  const queryClient = useQueryClient()
  const actorRole = useCurrentRole()
  const roles = useMemo(() => creatableRoles(actorRole), [actorRole])
  const defaultRole: UserRole = roles[roles.length - 1] ?? 'user'

  const [created, setCreated] = useState<CreateUserResponse | null>(null)
  const [copied, setCopied] = useState(false)
  const [isPending, setIsPending] = useState(false)
  // Closing the dialog while the request is in flight must never surface the
  // one-time password on the next open: reset() bumps this sequence, and a
  // response from an older request is discarded.
  const requestSeq = useRef(0)

  const formSchema = useMemo(
    () =>
      z.object({
        email: z.email({
          error: (iss) =>
            iss.input === ''
              ? t('users.validation.emailRequired')
              : t('users.validation.emailInvalid'),
        }),
        displayName: z
          .string()
          .trim()
          .min(1, t('users.validation.displayNameRequired'))
          .max(32, t('users.validation.displayNameTooLong')),
        role: z.enum(USER_ROLES),
      }),
    [t]
  )

  const form = useForm<z.infer<typeof formSchema>>({
    resolver: zodResolver(formSchema),
    defaultValues: {
      email: '',
      displayName: '',
      role: defaultRole,
    },
  })

  const submit = useCallback(
    async (values: z.infer<typeof formSchema>) => {
      const seq = ++requestSeq.current
      setIsPending(true)
      try {
        const { data } = await api.post<CreateUserResponse>('/users', values)
        if (seq !== requestSeq.current) return
        // The response (and its one-time password) lives only in this
        // component's local state, never in a store or the query cache.
        setCreated(data)
        setCopied(false)
        void queryClient.invalidateQueries({ queryKey: ['users'] })
      } catch (error) {
        if (seq === requestSeq.current) {
          toast.error(getApiErrorMessage(error, t))
        }
      } finally {
        if (seq === requestSeq.current) setIsPending(false)
      }
    },
    [queryClient, t]
  )

  const reset = () => {
    requestSeq.current += 1
    form.reset({ email: '', displayName: '', role: defaultRole })
    setCreated(null)
    setCopied(false)
    setIsPending(false)
  }

  const handleOpenChange = (nextOpen: boolean) => {
    if (!nextOpen) reset()
    onOpenChange(nextOpen)
  }

  const copyPassword = async () => {
    if (!created) return
    try {
      await navigator.clipboard.writeText(created.initialPassword)
      setCopied(true)
      toast.success(t('users.createDialog.copied'))
    } catch {
      toast.error(t('users.createDialog.copyFailed'))
    }
  }

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <DialogContent className='sm:max-w-md'>
        {created ? (
          <>
            <DialogHeader className='text-start'>
              <DialogTitle>{t('users.createDialog.credentialsTitle')}</DialogTitle>
              <DialogDescription>
                {t('users.createDialog.createdFor', {
                  email: created.user.email,
                })}
              </DialogDescription>
            </DialogHeader>
            <div className='space-y-3'>
              <p className='text-sm font-medium'>
                {t('users.createDialog.initialPassword')}
              </p>
              <div className='flex items-center gap-2'>
                <code className='bg-muted flex-1 overflow-x-auto rounded-md px-3 py-2 font-mono text-sm'>
                  {created.initialPassword}
                </code>
                <Button
                  type='button'
                  variant='outline'
                  size='icon'
                  aria-label={t('users.createDialog.copy')}
                  title={t('users.createDialog.copy')}
                  onClick={() => void copyPassword()}
                >
                  {copied ? <Check /> : <Copy />}
                </Button>
              </div>
              <p className='text-muted-foreground text-sm'>
                {t('users.createDialog.shownOnce')}
              </p>
            </div>
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
                <UserPlus size={18} /> {t('users.createDialog.title')}
              </DialogTitle>
              <DialogDescription>
                {t('users.createDialog.description')}
              </DialogDescription>
            </DialogHeader>
            <Form {...form}>
              <form
                id='user-create-form'
                onSubmit={form.handleSubmit(submit)}
                className='space-y-4'
              >
                <FormField
                  control={form.control}
                  name='email'
                  render={({ field }) => (
                    <FormItem>
                      <FormLabel>{t('users.createDialog.email')}</FormLabel>
                      <FormControl>
                        <Input
                          type='email'
                          autoComplete='off'
                          placeholder={t('signIn.emailPlaceholder')}
                          {...field}
                        />
                      </FormControl>
                      <FormMessage />
                    </FormItem>
                  )}
                />
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
                        items={roles.map((role) => ({
                          value: role,
                          label: t(`roles.${role}`),
                        }))}
                      />
                      <FormDescription>
                        {t('users.createDialog.roleHint')}
                      </FormDescription>
                      <FormMessage />
                    </FormItem>
                  )}
                />
              </form>
            </Form>
            <DialogFooter>
              <Button type='submit' form='user-create-form' disabled={isPending}>
                {isPending ? <Loader2 className='animate-spin' /> : <UserPlus />}
                {isPending
                  ? t('users.createDialog.creating')
                  : t('users.createDialog.submit')}
              </Button>
            </DialogFooter>
          </>
        )}
      </DialogContent>
    </Dialog>
  )
}
