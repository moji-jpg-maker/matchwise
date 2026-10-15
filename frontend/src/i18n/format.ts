import type { Locale } from './types'

const INTL_TAG: Record<Locale, string> = {
  en: 'en-US',
  fa: 'fa-IR-u-ca-persian-nu-arabext',
}

export function interpolate(template: string, params?: Record<string, string | number>): string {
  if (!params) return template
  return template.replace(/\{(\w+)\}/g, (match, name) => (name in params ? String(params[name]) : match))
}

export function createFormatters(locale: Locale) {
  const tag = INTL_TAG[locale]
  const toDate = (value: Date | string | number) => (value instanceof Date ? value : new Date(value))
  return {
    date: (value: Date | string | number) =>
      new Intl.DateTimeFormat(tag, { dateStyle: 'medium' }).format(toDate(value)),
    dateTime: (value: Date | string | number) =>
      new Intl.DateTimeFormat(tag, { dateStyle: 'medium', timeStyle: 'short' }).format(toDate(value)),
    relativeTime: (value: Date | string | number, now: number = Date.now()) => {
      const seconds = Math.round((toDate(value).getTime() - now) / 1000)
      const units: [Intl.RelativeTimeFormatUnit, number][] = [
        ['year', 31536000],
        ['month', 2592000],
        ['day', 86400],
        ['hour', 3600],
        ['minute', 60],
      ]
      const formatter = new Intl.RelativeTimeFormat(tag, { numeric: 'auto' })
      for (const [unit, size] of units) {
        if (Math.abs(seconds) >= size) return formatter.format(Math.round(seconds / size), unit)
      }
      return formatter.format(seconds, 'second')
    },
    number: (value: number, options?: Intl.NumberFormatOptions) =>
      new Intl.NumberFormat(tag, options).format(value),
    percent: (fraction: number) =>
      new Intl.NumberFormat(tag, { style: 'percent', maximumFractionDigits: 0 }).format(fraction),
    languageName: (code: string) => {
      try {
        return new Intl.DisplayNames([locale], { type: 'language' }).of(code) ?? code
      } catch {
        return code
      }
    },
  }
}

export type Formatters = ReturnType<typeof createFormatters>

const ARABIC_DIGITS = '٠١٢٣٤٥٦٧٨٩'
const PERSIAN_DIGITS = '۰۱۲۳۴۵۶۷۸۹'

export function normalizeSearch(value: string): string {
  return value
    .replace(/ي/g, 'ی')
    .replace(/ك/g, 'ک')
    .replace(/[\u200c\u064b-\u065f]/g, '')
    .replace(/[٠-٩]/g, (digit) => String(ARABIC_DIGITS.indexOf(digit)))
    .replace(/[۰-۹]/g, (digit) => String(PERSIAN_DIGITS.indexOf(digit)))
    .toLowerCase()
}
