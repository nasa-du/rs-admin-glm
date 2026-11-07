/**
 * i18next assembly (ADR 0002).
 *
 * Detection chain: remembered choice (localStorage) → browser language →
 * fallback zh-CN. The UI language never enters a cookie. All user-visible
 * copy lives in the two key-aligned locale files; components carry no bare
 * strings.
 */
import i18n from 'i18next'
import LanguageDetector from 'i18next-browser-languagedetector'
import { initReactI18next } from 'react-i18next'
import en from './locales/en.json'
import zhCN from './locales/zh-CN.json'

export const SUPPORTED_LANGUAGES = ['zh-CN', 'en'] as const
export type AppLanguage = (typeof SUPPORTED_LANGUAGES)[number]

/** localStorage key the detector remembers the chosen UI language under. */
export const LANGUAGE_STORAGE_KEY = 'rs_admin_lang'

void i18n
  .use(LanguageDetector)
  .use(initReactI18next)
  .init({
    resources: {
      'zh-CN': { translation: zhCN },
      en: { translation: en },
    },
    supportedLngs: [...SUPPORTED_LANGUAGES],
    fallbackLng: 'zh-CN',
    // A browser set to plain `zh` (or `zh-TW`) resolves to the supported
    // zh-CN through i18next's best-match logic. Do NOT set
    // `nonExplicitSupportedLngs`: it compares the *base* language (`zh`)
    // against this list and would reject `zh-CN` itself.
    detection: {
      order: ['localStorage', 'navigator'],
      lookupLocalStorage: LANGUAGE_STORAGE_KEY,
      caches: ['localStorage'],
    },
    interpolation: {
      // React already escapes; i18next escaping would double-encode.
      escapeValue: false,
    },
    returnEmptyString: false,
  })

export default i18n
