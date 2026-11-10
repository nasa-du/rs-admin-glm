import { ChevronDownIcon } from '@radix-ui/react-icons'
import { useTranslation } from 'react-i18next'
import { fonts } from '@/config/fonts'
import { cn } from '@/lib/utils'
import { LanguageSwitcher } from '@/components/language-switcher'
import { useFont } from '@/context/font-provider'
import { useTheme } from '@/context/theme-provider'
import { buttonVariants } from '@/components/ui/button'
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from '@/components/ui/card'
import { Label } from '@/components/ui/label'
import { RadioGroup, RadioGroupItem } from '@/components/ui/radio-group'
import { Separator } from '@/components/ui/separator'

/**
 * Appearance preferences (spec §6.4): template theme + font, plus the UI
 * language — the same switcher the sign-in page carries. Every control
 * applies immediately and remembers itself (theme/font via the template's
 * cookie, language via the i18next detector's localStorage, ADR 0002);
 * there is nothing to submit.
 */
export function AppearanceForm() {
  const { t } = useTranslation()
  const { font, setFont } = useFont()
  const { theme, setTheme } = useTheme()

  return (
    <div className='space-y-6'>
      <div className='space-y-2'>
        <Label>{t('settings.appearance.theme')}</Label>
        <p className='text-muted-foreground text-sm'>
          {t('settings.appearance.themeHint')}
        </p>
        <RadioGroup
          value={theme}
          onValueChange={setTheme}
          className='grid max-w-md grid-cols-2 gap-8 pt-2'
        >
          <Label className='[&:has([data-state=checked])>div]:border-primary'>
            <RadioGroupItem value='light' className='sr-only' />
            <div className='border-muted hover:border-accent items-center rounded-md border-2 p-1'>
              <div className='space-y-2 rounded-sm bg-[#ecedef] p-2'>
                <div className='space-y-2 rounded-md bg-white p-2 shadow-xs'>
                  <div className='h-2 w-[80px] rounded-lg bg-[#ecedef]' />
                  <div className='h-2 w-[100px] rounded-lg bg-[#ecedef]' />
                </div>
                <div className='flex items-center space-x-2 rounded-md bg-white p-2 shadow-xs'>
                  <div className='h-4 w-4 rounded-full bg-[#ecedef]' />
                  <div className='h-2 w-[100px] rounded-lg bg-[#ecedef]' />
                </div>
                <div className='flex items-center space-x-2 rounded-md bg-white p-2 shadow-xs'>
                  <div className='h-4 w-4 rounded-full bg-[#ecedef]' />
                  <div className='h-2 w-[100px] rounded-lg bg-[#ecedef]' />
                </div>
              </div>
            </div>
            <span className='block w-full p-2 text-center font-normal'>
              {t('settings.appearance.light')}
            </span>
          </Label>
          <Label className='[&:has([data-state=checked])>div]:border-primary'>
            <RadioGroupItem value='dark' className='sr-only' />
            <div className='border-muted bg-popover hover:bg-accent hover:text-accent-foreground items-center rounded-md border-2 p-1'>
              <div className='space-y-2 rounded-sm bg-slate-950 p-2'>
                <div className='space-y-2 rounded-md bg-slate-800 p-2 shadow-xs'>
                  <div className='h-2 w-[80px] rounded-lg bg-slate-400' />
                  <div className='h-2 w-[100px] rounded-lg bg-slate-400' />
                </div>
                <div className='flex items-center space-x-2 rounded-md bg-slate-800 p-2 shadow-xs'>
                  <div className='h-4 w-4 rounded-full bg-slate-400' />
                  <div className='h-2 w-[100px] rounded-lg bg-slate-400' />
                </div>
                <div className='flex items-center space-x-2 rounded-md bg-slate-800 p-2 shadow-xs'>
                  <div className='h-4 w-4 rounded-full bg-slate-400' />
                  <div className='h-2 w-[100px] rounded-lg bg-slate-400' />
                </div>
              </div>
            </div>
            <span className='block w-full p-2 text-center font-normal'>
              {t('settings.appearance.dark')}
            </span>
          </Label>
        </RadioGroup>
      </div>

      <div className='space-y-2'>
        <Label htmlFor='font'>{t('settings.appearance.font')}</Label>
        <p className='text-muted-foreground text-sm'>
          {t('settings.appearance.fontHint')}
        </p>
        <div className='relative w-max'>
          <select
            id='font'
            className={cn(
              buttonVariants({ variant: 'outline' }),
              'w-[200px] appearance-none font-normal capitalize',
              'dark:bg-background dark:hover:bg-background'
            )}
            value={font}
            onChange={(event) => setFont(event.target.value as typeof font)}
          >
            {fonts.map((font) => (
              <option key={font} value={font}>
                {font}
              </option>
            ))}
          </select>
          <ChevronDownIcon className='absolute end-3 top-2.5 h-4 w-4 opacity-50' />
        </div>
      </div>

      <Separator />

      <Card className='max-w-md'>
        <CardHeader>
          <CardTitle>{t('settings.appearance.language')}</CardTitle>
          <CardDescription>
            {t('settings.appearance.languageHint')}
          </CardDescription>
        </CardHeader>
        <CardContent>
          <LanguageSwitcher />
        </CardContent>
      </Card>
    </div>
  )
}
