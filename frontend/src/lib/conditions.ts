import { fieldLabel } from '@/lib/field-label';
import type { Locale } from '@/i18n/types';
import type { MessageKey } from '@/i18n';
import { Condition, ConditionOp, FieldDef, Preference, Strength, Value, kindName } from '@/types/matchmaking';

/** Operators offered per field kind (records fields are never searchable). */
export const OPS_BY_KIND: Record<string, { op: ConditionOp; labelKey: MessageKey }[]> = {
  number: [
    { op: 'between', labelKey: 'conditions.opBetween' },
    { op: 'eq', labelKey: 'conditions.opEq' },
    { op: 'ge', labelKey: 'conditions.opGe' },
    { op: 'le', labelKey: 'conditions.opLe' },
    { op: 'exists', labelKey: 'conditions.opExists' },
  ],
  text: [
    { op: 'eq', labelKey: 'conditions.opIs' },
    { op: 'ne', labelKey: 'conditions.opIsNot' },
    { op: 'exists', labelKey: 'conditions.opExists' },
  ],
  bool: [
    { op: 'eq', labelKey: 'conditions.opIs' },
    { op: 'exists', labelKey: 'conditions.opExists' },
  ],
  choice: [
    { op: 'in', labelKey: 'conditions.opAnyOf' },
    { op: 'ne', labelKey: 'conditions.opIsNot' },
    { op: 'exists', labelKey: 'conditions.opExists' },
  ],
  multi_choice: [
    { op: 'in', labelKey: 'conditions.opIncludesAny' },
    { op: 'exists', labelKey: 'conditions.opExists' },
  ],
};

export interface Row {
  field: string;
  op: ConditionOp;
  a: string;
  b: string;
  multi: string[];
}

export const isConditionField = (f: FieldDef) => kindName(f.kind) !== 'records';

export function defaultOp(def: FieldDef): ConditionOp {
  return OPS_BY_KIND[kindName(def.kind)][0].op;
}

/** Turn a form row into a condition, or return a message describing what is missing. */
export type Translate = (key: MessageKey, params?: Record<string, string | number>) => string;

export function toCondition(r: Row, def: FieldDef, t: Translate, locale: Locale = 'en'): Condition | string {
  const fieldName = fieldLabel(def, locale);
  const kind = kindName(def.kind);
  if (r.op === 'exists') return { field: r.field, op: 'exists' };
  if (kind === 'number') {
    if (r.a.trim() === '' || Number.isNaN(Number(r.a))) return t('conditions.enterNumber', { field: fieldName });
    if (r.op === 'between') {
      if (r.b.trim() === '' || Number.isNaN(Number(r.b))) return t('conditions.enterUpper', { field: fieldName });
      return { field: r.field, op: 'between', value: Number(r.a), value2: Number(r.b) };
    }
    return { field: r.field, op: r.op, value: Number(r.a) };
  }
  if (kind === 'bool') return { field: r.field, op: r.op, value: r.a === 'true' };
  if (kind === 'choice' || kind === 'multi_choice') {
    if (r.op === 'in') {
      if (r.multi.length === 0) return t('conditions.pickAtLeastOne', { field: fieldName });
      return { field: r.field, op: 'in', value: r.multi };
    }
    if (!r.a) return t('conditions.pickOption', { field: fieldName });
    return { field: r.field, op: r.op, value: r.a };
  }
  if (r.a.trim() === '') return t('conditions.enterValue', { field: fieldName });
  return { field: r.field, op: r.op, value: r.a.trim() as Value };
}

/** A preference row in the editor: a condition row plus strength, importance and an optional "relative to own field". */
export interface PrefRow extends Row {
  id: string;
  strength: Strength;
  importance: number;
  note: string;
  rel: boolean;
  relField: string;
}

const toNum = (s: string) => (s.trim() === '' ? 0 : Number(s));

export function toPreference(r: PrefRow, def: FieldDef, t: Translate, locale: Locale = 'en'): Preference | string {
  const fieldName = fieldLabel(def, locale);
  let condition: Condition | string;
  if (r.rel && kindName(def.kind) === 'number' && r.op !== 'exists') {
    if (Number.isNaN(toNum(r.a)) || (r.op === 'between' && (r.b.trim() === '' || Number.isNaN(Number(r.b)))))
      return t('conditions.offsetsNumbers', { field: fieldName });
    condition =
      r.op === 'between'
        ? {
            field: r.field,
            op: 'between',
            value_rel: { field: r.relField, offset: toNum(r.a) },
            value2_rel: { field: r.relField, offset: Number(r.b) },
          }
        : { field: r.field, op: r.op, value_rel: { field: r.relField, offset: toNum(r.a) } };
  } else {
    condition = toCondition(r, def, t, locale);
  }
  if (typeof condition === 'string') return condition;
  return { id: r.id, condition, strength: r.strength, importance: r.importance, note: r.note.trim() || null };
}

export function fromPreference(p: Preference, defs: Record<string, FieldDef>): PrefRow {
  const c = p.condition;
  const def = defs[c.field];
  const kind = def ? kindName(def.kind) : 'text';
  const row: PrefRow = {
    id: p.id,
    field: c.field,
    op: c.op,
    a: '',
    b: '',
    multi: [],
    strength: p.strength,
    importance: p.importance,
    note: p.note ?? '',
    rel: false,
    relField: c.field,
  };
  if (c.value_rel) {
    row.rel = true;
    row.relField = c.value_rel.field;
    row.a = String(c.value_rel.offset);
    if (c.value2_rel) row.b = String(c.value2_rel.offset);
    return row;
  }
  const v = c.value;
  if (kind === 'choice' || kind === 'multi_choice') {
    if (c.op === 'in') row.multi = Array.isArray(v) ? (v as string[]) : typeof v === 'string' ? [v] : [];
    else row.a = typeof v === 'string' ? v : '';
  } else if (v !== undefined && v !== null) {
    row.a = String(v);
  }
  if (c.value2 !== undefined && c.value2 !== null) row.b = String(c.value2);
  return row;
}

/** Human-readable one-line description, used when showing evaluation results. */
export function describePreference(p: Preference, defs: Record<string, FieldDef>, t: Translate, locale: Locale = 'en'): string {
  const c = p.condition;
  const label = defs[c.field] ? fieldLabel(defs[c.field], locale) : c.field;
  const opLabel = (OPS_BY_KIND[defs[c.field] ? kindName(defs[c.field].kind) : 'text'] ?? []).find((o) => o.op === c.op)?.labelKey ?? null;
  const opText = opLabel ? t(opLabel) : c.op;
  const rel = (r: { field: string; offset: number }) =>
    `${t('conditions.own', { field: defs[r.field] ? fieldLabel(defs[r.field], locale) : r.field })}${r.offset === 0 ? '' : r.offset > 0 ? ` + ${r.offset}` : ` − ${Math.abs(r.offset)}`}`;
  const val = (v: Value | null | undefined) => (Array.isArray(v) ? (v as string[]).join(', ') : String(v ?? ''));
  const lo = c.value_rel ? rel(c.value_rel) : val(c.value);
  const hi = c.value2_rel ? rel(c.value2_rel) : val(c.value2);
  const body = c.op === 'exists' ? '' : c.op === 'between' ? ` ${lo} ${t('conditions.and')} ${hi}` : ` ${lo}`;
  return t(p.strength === 'deal_breaker' ? 'conditions.dealBreaker' : 'conditions.partner', { field: label, operator: opText, body });
}
