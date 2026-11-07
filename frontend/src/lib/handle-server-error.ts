import i18n from '@/i18n'
import { toast } from 'sonner'
import { getApiErrorMessage } from '@/lib/api-error'

export function handleServerError(error: unknown) {
  // eslint-disable-next-line no-console
  console.log(error)

  toast.error(getApiErrorMessage(error, i18n.t))
}
