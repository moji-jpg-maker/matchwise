import type { FieldDef } from '@/types/matchmaking';
import type { Locale } from '@/i18n/types';

export function fieldLabel(def: Pick<FieldDef, 'label' | 'label_fa'>, locale: Locale): string {
  const persian = def.label_fa?.trim();
  return locale === 'fa' && persian ? persian : def.label;
}
