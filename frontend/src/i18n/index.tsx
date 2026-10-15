'use client'

import React, { createContext, useCallback, useContext, useEffect, useMemo, useState } from 'react'
import { Direction } from 'radix-ui'
import { en } from './en'
import { fa } from './fa'
import { createFormatters, interpolate, type Formatters } from './format'
import { LOCALES, type Locale, type MessageKey } from './types'

const CATALOGS: Record<Locale, unknown> = { en, fa }
const STORAGE_KEY = 'matchwise.locale'

export function lookup(catalog: unknown, key: string): string | undefined {
  let node: unknown = catalog
  for (const part of key.split('.')) {
    if (node && typeof node === 'object' && part in (node as Record<string, unknown>)) {
      node = (node as Record<string, unknown>)[part]
    } else {
      return undefined
    }
  }
  return typeof node === 'string' ? node : undefined
}

export function translate(locale: Locale, key: MessageKey, params?: Record<string, string | number>): string {
  const text = lookup(CATALOGS[locale], key) ?? lookup(CATALOGS.en, key) ?? key
  return interpolate(text, params)
}

export function directionOf(locale: Locale): 'ltr' | 'rtl' {
  return LOCALES.find((entry) => entry.code === locale)?.dir ?? 'ltr'
}

function detectInitialLocale(): Locale {
  if (typeof window === 'undefined') return 'en'
  try {
    const stored = window.localStorage.getItem(STORAGE_KEY)
    if (stored === 'en' || stored === 'fa') return stored
  } catch {}
  return typeof navigator !== 'undefined' && navigator.language?.toLowerCase().startsWith('fa') ? 'fa' : 'en'
}

export function currentLocale(): Locale {
  return detectInitialLocale()
}

interface I18nContextValue {
  locale: Locale
  dir: 'ltr' | 'rtl'
  setLocale: (locale: Locale) => void
  t: (key: MessageKey, params?: Record<string, string | number>) => string
  fmt: Formatters
}

const FALLBACK_CONTEXT: I18nContextValue = {
  locale: 'en',
  dir: 'ltr',
  setLocale: () => {},
  t: (key, params) => translate('en', key, params),
  fmt: createFormatters('en'),
}

const I18nContext = createContext<I18nContextValue>(FALLBACK_CONTEXT)

export function I18nProvider({ children }: { children: React.ReactNode }) {
  const [locale, setLocaleState] = useState<Locale>('en')

  useEffect(() => {
    setLocaleState(detectInitialLocale())
  }, [])

  const dir = directionOf(locale)

  useEffect(() => {
    document.documentElement.lang = locale
    document.documentElement.dir = dir
  }, [locale, dir])

  useEffect(() => {
    if (typeof window === 'undefined' || !('__TAURI_INTERNALS__' in window)) return
    import('@tauri-apps/api/core')
      .then(({ invoke }) => invoke('set_ui_locale', { locale }))
      .catch(() => {})
  }, [locale])

  const setLocale = useCallback((next: Locale) => {
    setLocaleState(next)
    try {
      window.localStorage.setItem(STORAGE_KEY, next)
    } catch {}
  }, [])

  const value = useMemo<I18nContextValue>(
    () => ({
      locale,
      dir,
      setLocale,
      t: (key, params) => translate(locale, key, params),
      fmt: createFormatters(locale),
    }),
    [locale, dir, setLocale],
  )

  return (
    <I18nContext.Provider value={value}>
      <Direction.DirectionProvider dir={dir}>{children}</Direction.DirectionProvider>
    </I18nContext.Provider>
  )
}

export function useI18n(): I18nContextValue {
  return useContext(I18nContext)
}

export function useT() {
  return useI18n().t
}

export { LOCALES }
export type { Locale, MessageKey }
