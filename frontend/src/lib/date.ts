import { format } from 'date-fns'
import { enUS, zhCN } from 'date-fns/locale'

/**
 * Format a timestamp for the current UI language (spec §6.5):
 * zh-CN → `2026-11-06 14:30`, en → `Nov 6, 2026`.
 * Invalid input renders as an empty string rather than `Invalid Date`.
 */
export function formatDateTime(
  value: string | Date,
  language: string
): string {
  const date = typeof value === 'string' ? new Date(value) : value
  if (Number.isNaN(date.getTime())) return ''

  if (language.startsWith('zh')) {
    return format(date, 'yyyy-MM-dd HH:mm', { locale: zhCN })
  }
  return format(date, 'MMM d, yyyy', { locale: enUS })
}
