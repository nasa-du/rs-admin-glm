import { useState } from 'react'
import { Check, Copy } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'

type OneTimePasswordProps = {
  /** The one-time initial password; lives only in the caller's state. */
  password: string
}

/**
 * The one-time initial password display (spec §2.4 / §6.3): shown exactly
 * once with a copy button, never stored anywhere that outlives the dialog.
 * Shared by the create and password-reset dialogs.
 */
export function OneTimePassword({ password }: OneTimePasswordProps) {
  const { t } = useTranslation()
  const [copied, setCopied] = useState(false)

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(password)
      setCopied(true)
      toast.success(t('users.initialPassword.copied'))
    } catch {
      toast.error(t('users.initialPassword.copyFailed'))
    }
  }

  return (
    <div className='space-y-3'>
      <p className='text-sm font-medium'>{t('users.initialPassword.label')}</p>
      <div className='flex items-center gap-2'>
        <code className='bg-muted flex-1 overflow-x-auto rounded-md px-3 py-2 font-mono text-sm'>
          {password}
        </code>
        <Button
          type='button'
          variant='outline'
          size='icon'
          aria-label={t('users.initialPassword.copy')}
          title={t('users.initialPassword.copy')}
          onClick={() => void copy()}
        >
          {copied ? <Check /> : <Copy />}
        </Button>
      </div>
      <p className='text-muted-foreground text-sm'>
        {t('users.initialPassword.shownOnce')}
      </p>
    </div>
  )
}
