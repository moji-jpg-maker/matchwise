'use client'

import { LOCALES, useI18n } from '@/i18n'

export function LanguageSwitcher({ className = '' }: { className?: string }) {
  const { locale, setLocale, t } = useI18n()

  return (
    <div
      role="group"
      aria-label={t('common.interfaceLanguage')}
      className={`inline-flex rounded-md border border-gray-300 bg-white p-0.5 text-sm ${className}`}
    >
      {LOCALES.map((entry) => (
        <button
          key={entry.code}
          type="button"
          lang={entry.code}
          dir={entry.dir}
          aria-pressed={locale === entry.code}
          onClick={() => setLocale(entry.code)}
          className={`rounded px-3 py-1 transition-colors ${
            locale === entry.code ? 'bg-gray-900 text-white' : 'text-gray-700 hover:bg-gray-100'
          }`}
        >
          {entry.name}
        </button>
      ))}
    </div>
  )
}
