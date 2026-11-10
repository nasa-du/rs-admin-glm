import { useMemo, useState } from 'react'
import { z } from 'zod'
import { useForm } from 'react-hook-form'
import { zodResolver } from '@hookform/resolvers/zod'
import { useNavigate } from '@tanstack/react-router'
import { KeyRound, Loader2 } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { useAuthStore } from '@/stores/auth-store'
import { getApiErrorMessage, getApiFieldErrors } from '@/lib/api-error'
import { api } from '@/lib/axios'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import {
  Form,
  FormControl,
  FormDescription,
  FormField,
  FormItem,
  FormLabel,
  FormMessage,
} from '@/components/ui/form'
import { PasswordInput } from '@/components/password-input'

/** Form fields = the `PATCH /auth/password` body (spec §5.3). */
type PasswordFormValues = {
  currentPassword: string
  newPassword: string
}

/**
 * Change-password form (spec §6.4): the destination of the first-login
 * forced change and of every later one. Two contracts matter here:
 *
 * - a wrong current password comes back as a 422 **field** error — it lands
 *   on the input via `setError`, never as a 401 (which the interceptor
 *   would answer with a pointless refresh replay against a live session);
 * - success revoked **every** token family, this browser's included, so the
 *   only honest next step is dropping the in-memory session and signing in
 *   again with the new password.
 */
export function PasswordForm() {
  const { t } = useTranslation()
  const navigate = useNavigate()
  const mustChangePassword = useAuthStore(
    (state) => state.user?.mustChangePassword ?? false
  )
  const [isPending, setIsPending] = useState(false)

  const formSchema = useMemo(
    () =>
      z.object({
        currentPassword: z
          .string()
          .min(1, t('settings.password.currentRequired')),
        newPassword: z
          .string()
          .min(1, t('settings.password.newRequired'))
          // 8–128 policy (spec §2.1), the same bounds as the backend.
          .min(8, t('errors.PASSWORD_TOO_SHORT'))
          .max(128, t('errors.PASSWORD_TOO_LONG')),
      }),
    [t]
  )

  const form = useForm<PasswordFormValues>({
    resolver: zodResolver(formSchema),
    defaultValues: { currentPassword: '', newPassword: '' },
  })

  const submit = async (values: PasswordFormValues) => {
    setIsPending(true)
    try {
      await api.patch('/auth/password', {
        currentPassword: values.currentPassword,
        newPassword: values.newPassword,
      })

      // Every session (this one included) died with the old password hash.
      toast.success(t('settings.password.toasts.changed'))
      useAuthStore.getState().reset()
      navigate({ to: '/sign-in', replace: true })
    } catch (error) {
      const fieldErrors = getApiFieldErrors(error)
      let handled = false
      for (const field of ['currentPassword', 'newPassword'] as const) {
        const code = fieldErrors[field]
        if (code) {
          form.setError(field, { type: 'server', message: t(`errors.${code}`) })
          handled = true
        }
      }
      if (!handled) toast.error(getApiErrorMessage(error, t))
    } finally {
      setIsPending(false)
    }
  }

  return (
    <div className='space-y-4'>
      {mustChangePassword && (
        <Alert>
          <KeyRound />
          <AlertTitle>{t('settings.password.forcedTitle')}</AlertTitle>
          <AlertDescription>
            {t('settings.password.forcedDescription')}
          </AlertDescription>
        </Alert>
      )}
      <Form {...form}>
        <form
          onSubmit={form.handleSubmit(submit)}
          className='space-y-4'
        >
          <FormField
            control={form.control}
            name='currentPassword'
            render={({ field }) => (
              <FormItem>
                <FormLabel>{t('settings.password.currentPassword')}</FormLabel>
                <FormControl>
                  <PasswordInput
                    autoComplete='current-password'
                    {...field}
                  />
                </FormControl>
                <FormMessage />
              </FormItem>
            )}
          />
          <FormField
            control={form.control}
            name='newPassword'
            render={({ field }) => (
              <FormItem>
                <FormLabel>{t('settings.password.newPassword')}</FormLabel>
                <FormControl>
                  <PasswordInput autoComplete='new-password' {...field} />
                </FormControl>
                <FormDescription>
                  {t('settings.password.newPasswordHint')}
                </FormDescription>
                <FormMessage />
              </FormItem>
            )}
          />
          <p className='text-muted-foreground text-sm'>
            {t('settings.password.reloginHint')}
          </p>
          <Button type='submit' disabled={isPending}>
            {isPending ? <Loader2 className='animate-spin' /> : <KeyRound />}
            {isPending
              ? t('settings.password.submitting')
              : t('settings.password.submit')}
          </Button>
        </form>
      </Form>
    </div>
  )
}
