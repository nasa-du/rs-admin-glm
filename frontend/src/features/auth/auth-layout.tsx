import { useTranslation } from 'react-i18next'
import { Logo } from '@/assets/logo'
import { LanguageSwitcher } from '@/components/language-switcher'

type AuthLayoutProps = {
  children: React.ReactNode
}

export function AuthLayout({ children }: AuthLayoutProps) {
  const { t } = useTranslation()

  return (
    <div className='relative container grid h-svh max-w-none items-center justify-center'>
      {/* UI language is reachable from the very first screen (user story 2) */}
      <div className='absolute end-4 top-4'>
        <LanguageSwitcher />
      </div>
      <div className='mx-auto flex w-full flex-col justify-center space-y-2 py-8 sm:w-[480px] sm:p-8'>
        <div className='mb-4 flex items-center justify-center'>
          <Logo className='me-2' />
          <h1 className='text-xl font-medium'>{t('common.appName')}</h1>
        </div>
        {children}
      </div>
    </div>
  )
}
