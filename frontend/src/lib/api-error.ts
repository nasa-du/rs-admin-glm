import axios, { type AxiosError } from 'axios'
import type { TFunction } from 'i18next'

/**
 * Map an API error to user-visible text in the current UI language
 * (spec §5.2 / ADR 0002): the backend's stable code → `errors` locale
 * namespace. A field-level code (422) is more specific than the envelope's
 * `VALIDATION_ERROR`, so it wins; unmapped codes fall back to the English
 * `title` the backend always sends. Transport failures get their own code.
 *
 * `t` is passed in rather than imported as a singleton, so the mapper has no
 * hidden dependency on global i18n state and works with any namespace.
 */
export function getApiErrorMessage(error: unknown, t: TFunction): string {
  if (isApiError(error)) {
    const body = error.response?.data
    const fieldCode = body?.errors
      ? Object.values(body.errors).flat()[0]
      : undefined
    const code = fieldCode ?? body?.code

    if (code) {
      // defaultValue = the backend's English title, exactly the spec's
      // "no mapping → fall back to title" rule.
      const message = t(`errors.${code}`, { defaultValue: body?.title ?? '' })
      if (message) {
        return message
      }
    }
    if (body?.title) {
      return body.title
    }
    if (!error.response) {
      return t('errors.NETWORK_ERROR')
    }
  }
  return t('errors.INTERNAL_ERROR')
}

/** The backend error body (spec §5.2). */
export interface ApiErrorBody {
  code?: string
  title?: string
  errors?: Record<string, string[]>
}

/** `instanceof` cannot carry generics; axios's own guard can. */
function isApiError(error: unknown): error is AxiosError<ApiErrorBody> {
  return axios.isAxiosError(error)
}

/**
 * Field-level codes of a 422 body as `{field: firstCode}` — the input
 * `form.setError` consumes. The change-password form lives on this contract:
 * a wrong current password lands on the field, not as a toast (issue #19).
 */
export function getApiFieldErrors(error: unknown): Record<string, string> {
  if (!isApiError(error)) return {}
  const errors = error.response?.data?.errors ?? {}
  return Object.fromEntries(
    Object.entries(errors).map(([field, codes]) => [field, codes[0]])
  )
}
