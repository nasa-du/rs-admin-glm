import { useMemo } from 'react'
import { z } from 'zod'
import { useForm } from 'react-hook-form'
import { zodResolver } from '@hookform/resolvers/zod'
import { useNavigate } from '@tanstack/react-router'
import { Loader2, LogIn } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { type LoginResponse, useAuthStore } from '@/stores/auth-store'
import { getApiErrorMessage } from '@/lib/api-error'
import { api } from '@/lib/axios'
import { cn } from '@/lib/utils'
import { Button } from '@/components/ui/button'
import {
  Form,
  FormControl,
  FormField,
  FormItem,
  FormLabel,
  FormMessage,
} from '@/components/ui/form'
import { Input } from '@/components/ui/input'
import { PasswordInput } from '@/components/password-input'

interface UserAuthFormProps extends React.HTMLAttributes<HTMLFormElement> {
  redirectTo?: string
}

export function UserAuthForm({
  className,
  redirectTo,
  ...props
}: UserAuthFormProps) {
  const { t } = useTranslation()
  const navigate = useNavigate()
  const setSession = useAuthStore((state) => state.setSession)

  // Rebuilt when the UI language changes, so validation messages follow the
  // switcher without a remount.
  const formSchema = useMemo(
    () =>
      z.object({
        email: z.email({
          error: (iss) =>
            iss.input === ''
              ? t('signIn.validation.emailRequired')
              : t('signIn.validation.emailInvalid'),
        }),
        password: z
          .string()
          .min(1, t('signIn.validation.passwordRequired'))
          // 8–128 policy (spec §2.1), aligned with the backend's own floor.
          .min(8, t('signIn.validation.passwordMin')),
      }),
    [t]
  )

  const form = useForm<z.infer<typeof formSchema>>({
    resolver: zodResolver(formSchema),
    defaultValues: {
      email: '',
      password: '',
    },
  })

  async function onSubmit(data: z.infer<typeof formSchema>) {
    try {
      const { data: session } = await api.post<LoginResponse>('/auth/login', {
        email: data.email,
        password: data.password,
      })

      setSession(session)
      toast.success(t('signIn.welcome', { name: session.user.displayName }))

      // Redirect to the stored location or default to dashboard
      const targetPath = redirectTo || '/'
      navigate({ to: targetPath, replace: true })
    } catch (error) {
      toast.error(getApiErrorMessage(error, t))
    }
  }

  const isSubmitting = form.formState.isSubmitting

  return (
    <Form {...form}>
      <form
        onSubmit={form.handleSubmit(onSubmit)}
        className={cn('grid gap-3', className)}
        {...props}
      >
        <FormField
          control={form.control}
          name='email'
          render={({ field }) => (
            <FormItem>
              <FormLabel>{t('signIn.email')}</FormLabel>
              <FormControl>
                <Input placeholder={t('signIn.emailPlaceholder')} {...field} />
              </FormControl>
              <FormMessage />
            </FormItem>
          )}
        />
        <FormField
          control={form.control}
          name='password'
          render={({ field }) => (
            <FormItem className='relative'>
              <FormLabel>{t('signIn.password')}</FormLabel>
              <FormControl>
                <PasswordInput placeholder='********' {...field} />
              </FormControl>
              <FormMessage />
            </FormItem>
          )}
        />
        <Button className='mt-2' disabled={isSubmitting}>
          {isSubmitting ? <Loader2 className='animate-spin' /> : <LogIn />}
          {isSubmitting ? t('signIn.submitting') : t('signIn.submit')}
        </Button>
      </form>
    </Form>
  )
}
