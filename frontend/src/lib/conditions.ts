import { Condition, ConditionOp, FieldDef, Preference, Strength, Value, kindName } from '@/types/matchmaking';

/** Operators offered per field kind (records fields are never searchable). */
export const OPS_BY_KIND: Record<string, { op: ConditionOp; label: string }[]> = {
  number: [
    { op: 'between', label: 'between' },
    { op: 'eq', label: '=' },
    { op: 'ge', label: '≥' },
    { op: 'le', label: '≤' },
    { op: 'exists', label: 'is filled in' },
  ],
  text: [
    { op: 'eq', label: 'is' },
    { op: 'ne', label: 'is not' },
    { op: 'exists', label: 'is filled in' },
  ],
  bool: [
    { op: 'eq', label: 'is' },
    { op: 'exists', label: 'is filled in' },
  ],
  choice: [
    { op: 'in', label: 'is any of' },
    { op: 'ne', label: 'is not' },
    { op: 'exists', label: 'is filled in' },
  ],
  multi_choice: [
    { op: 'in', label: 'includes any of' },
    { op: 'exists', label: 'is filled in' },
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
export function toCondition(r: Row, def: FieldDef): Condition | string {
  const kind = kindName(def.kind);
  if (r.op === 'exists') return { field: r.field, op: 'exists' };
  if (kind === 'number') {
    if (r.a.trim() === '' || Number.isNaN(Number(r.a))) return `${def.label}: enter a number`;
    if (r.op === 'between') {
      if (r.b.trim() === '' || Number.isNaN(Number(r.b))) return `${def.label}: enter an upper bound`;
      return { field: r.field, op: 'between', value: Number(r.a), value2: Number(r.b) };
    }
    return { field: r.field, op: r.op, value: Number(r.a) };
  }
  if (kind === 'bool') return { field: r.field, op: r.op, value: r.a === 'true' };
  if (kind === 'choice' || kind === 'multi_choice') {
    if (r.op === 'in') {
      if (r.multi.length === 0) return `${def.label}: pick at least one option`;
      return { field: r.field, op: 'in', value: r.multi };
    }
    if (!r.a) return `${def.label}: pick an option`;
    return { field: r.field, op: r.op, value: r.a };
  }
  if (r.a.trim() === '') return `${def.label}: enter a value`;
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

export function toPreference(r: PrefRow, def: FieldDef): Preference | string {
  let condition: Condition | string;
  if (r.rel && kindName(def.kind) === 'number' && r.op !== 'exists') {
    if (Number.isNaN(toNum(r.a)) || (r.op === 'between' && (r.b.trim() === '' || Number.isNaN(Number(r.b)))))
      return `${def.label}: offsets must be numbers`;
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
    condition = toCondition(r, def);
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
export function describePreference(p: Preference, defs: Record<string, FieldDef>): string {
  const c = p.condition;
  const label = defs[c.field]?.label ?? c.field;
  const opLabel = (OPS_BY_KIND[defs[c.field] ? kindName(defs[c.field].kind) : 'text'] ?? []).find((o) => o.op === c.op)?.label ?? c.op;
  const rel = (r: { field: string; offset: number }) =>
    `own ${defs[r.field]?.label ?? r.field}${r.offset === 0 ? '' : r.offset > 0 ? ` + ${r.offset}` : ` − ${Math.abs(r.offset)}`}`;
  const val = (v: Value | null | undefined) => (Array.isArray(v) ? (v as string[]).join(', ') : String(v ?? ''));
  const lo = c.value_rel ? rel(c.value_rel) : val(c.value);
  const hi = c.value2_rel ? rel(c.value2_rel) : val(c.value2);
  const body = c.op === 'exists' ? '' : c.op === 'between' ? ` ${lo} and ${hi}` : ` ${lo}`;
  const prefix = p.strength === 'deal_breaker' ? 'Deal-breaker: partner’s ' : 'Partner’s ';
  return `${prefix}${label} ${opLabel}${body}`;
}
