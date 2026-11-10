import { useEffect, useMemo, useState } from 'react'
import { z } from 'zod'
import { useForm } from 'react-hook-form'
import { zodResolver } from '@hookform/resolvers/zod'
import { Loader2, Save } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { type ApiUser, useAuthStore } from '@/stores/auth-store'
import { getApiErrorMessage, getApiFieldErrors } from '@/lib/api-error'
import { api } from '@/lib/axios'
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
import { Input } from '@/components/ui/input'

type ProfileFormValues = {
  displayName: string
}

/**
 * Self-profile form (spec §6.4): `PATCH /users/me`, the display name only —
 * the login identifier is not self-service. The store user is refreshed from
 * the response, so the sidebar and header pick the new name up immediately.
 */
export function ProfileForm() {
  const { t } = useTranslation()
  const user = useAuthStore((state) => state.user)
  const setUser = useAuthStore((state) => state.setUser)
  const [isPending, setIsPending] = useState(false)

  const formSchema = useMemo(
    () =>
      z.object({
        displayName: z
          .string()
          .trim()
          .min(1, t('errors.DISPLAY_NAME_REQUIRED'))
          .max(32, t('errors.DISPLAY_NAME_TOO_LONG')),
      }),
    [t]
  )

  const form = useForm<ProfileFormValues>({
    resolver: zodResolver(formSchema),
    defaultValues: { displayName: user?.displayName ?? '' },
  })

  // Re-seed when the store user changes (e.g. after a refresh re-hydration).
  useEffect(() => {
    if (user) form.reset({ displayName: user.displayName })
    // form.reset is stable.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [user])

  const submit = async (values: ProfileFormValues) => {
    setIsPending(true)
    try {
      const { data } = await api.patch<{ user: ApiUser }>('/users/me', {
        displayName: values.displayName,
      })
      setUser(data.user)
      toast.success(t('settings.profile.toasts.updated'))
    } catch (error) {
      const code = getApiFieldErrors(error).displayName
      if (code) {
        form.setError('displayName', { type: 'server', message: t(`errors.${code}`) })
      } else {
        toast.error(getApiErrorMessage(error, t))
      }
    } finally {
      setIsPending(false)
    }
  }

  return (
    <Form {...form}>
      <form onSubmit={form.handleSubmit(submit)} className='space-y-8'>
        <FormField
          control={form.control}
          name='displayName'
          render={({ field }) => (
            <FormItem>
              <FormLabel>{t('settings.profile.displayName')}</FormLabel>
              <FormControl>
                <Input autoComplete='name' {...field} />
              </FormControl>
              <FormDescription>
                {t('settings.profile.displayNameHint')}
              </FormDescription>
              <FormMessage />
            </FormItem>
          )}
        />
        <FormItem>
          <FormLabel>{t('settings.profile.email')}</FormLabel>
          <FormControl>
            <Input value={user?.email ?? ''} disabled readOnly />
          </FormControl>
          <FormDescription>{t('settings.profile.emailHint')}</FormDescription>
        </FormItem>
        <Button type='submit' disabled={isPending}>
          {isPending ? <Loader2 className='animate-spin' /> : <Save />}
          {isPending
            ? t('settings.profile.saving')
            : t('settings.profile.submit')}
        </Button>
      </form>
    </Form>
  )
}
