import { SUPPORTED_LANGUAGES, type AppLanguage } from '@/i18n'
import { Check, Languages } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { Button } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'

/** Each language is named in itself — the one label never translated. */
const LANGUAGE_LABELS: Record<AppLanguage, string> = {
  'zh-CN': '中文',
  en: 'English',
}

/**
 * UI-language switcher (ADR 0002). The choice persists via the i18next
 * detector's localStorage cache; no cookies involved.
 */
export function LanguageSwitcher() {
  const { i18n, t } = useTranslation()
  const current = i18n.language

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          variant='ghost'
          size='icon'
          aria-label={t('languageSwitcher.label')}
          title={t('languageSwitcher.label')}
        >
          <Languages />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align='end'>
        {SUPPORTED_LANGUAGES.map((language) => (
          <DropdownMenuItem
            key={language}
            onClick={() => void i18n.changeLanguage(language)}
          >
            <Check className={language === current ? '' : 'invisible'} />
            {LANGUAGE_LABELS[language]}
          </DropdownMenuItem>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  )
}
