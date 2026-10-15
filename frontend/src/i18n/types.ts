import type { en } from './en'

export type Catalog = typeof en
export type DeepString<T> = { [K in keyof T]: T[K] extends string ? string : DeepString<T[K]> }
export type Locale = 'en' | 'fa'
export const LOCALES: { code: Locale; name: string; dir: 'ltr' | 'rtl' }[] = [
  { code: 'en', name: 'English', dir: 'ltr' },
  { code: 'fa', name: 'فارسی', dir: 'rtl' },
]

type Paths<T, P extends string = ''> = {
  [K in keyof T & string]: T[K] extends string ? `${P}${K}` : Paths<T[K], `${P}${K}.`>
}[keyof T & string]
export type MessageKey = Paths<Catalog>
