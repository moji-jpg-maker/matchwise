import { CmpOp, Expr, FieldDef, Operand, Rule, RuleSet, Side, Value, kindName } from '@/types/matchmaking';
import { isConditionField } from '@/lib/conditions';

export const CMP_LABEL: Record<CmpOp, string> = {
  eq: '=',
  ne: '≠',
  lt: '<',
  le: '≤',
  gt: '>',
  ge: '≥',
  in: 'is one of',
};

export const NODE_TYPES = [
  { value: 'cmp', label: 'compare' },
  { value: 'between', label: 'between' },
  { value: 'exists', label: 'is filled in' },
  { value: 'and', label: 'all of' },
  { value: 'or', label: 'any of' },
  { value: 'not', label: 'not' },
  { value: 'if', label: 'if … then' },
] as const;

export type NodeType = (typeof NODE_TYPES)[number]['value'];

export const fieldOperand = (of: Side, key: string): Operand => ({ field: { of, key } });
export const litOperand = (value: Value): Operand => ({ lit: { value } });

export function firstField(fields: FieldDef[]): string {
  return fields.find(isConditionField)?.key ?? '';
}

/** A fresh node of the given type with sensible defaults. */
export function newNode(type: NodeType, fields: FieldDef[]): Expr {
  const key = firstField(fields);
  switch (type) {
    case 'cmp':
      return { op: 'cmp', left: fieldOperand('b', key), cmp: 'eq', right: litOperand('') };
    case 'between':
      return { op: 'between', value: fieldOperand('b', key), lo: litOperand(0), hi: litOperand(0) };
    case 'exists':
      return { op: 'exists', value: fieldOperand('b', key) };
    case 'and':
      return { op: 'and', args: [newNode('cmp', fields)] };
    case 'or':
      return { op: 'or', args: [newNode('cmp', fields)] };
    case 'not':
      return { op: 'not', arg: newNode('cmp', fields) };
    case 'if':
      return { op: 'if', when: newNode('cmp', fields), then: newNode('cmp', fields) };
  }
}

export function newRule(existing: Rule[], fields: FieldDef[]): Rule {
  let n = existing.length + 1;
  const ids = new Set(existing.map((r) => r.id));
  while (ids.has(`rule_${n}`)) n++;
  return {
    id: `rule_${n}`,
    description: '',
    kind: 'soft',
    weight: 1,
    expr: newNode('cmp', fields),
    when: null,
    group: null,
    priority: 0,
    scope: 'pair',
    enabled: true,
  };
}

export function emptyRuleSet(): RuleSet {
  return { name: '', version: 0, rules: [], group_weights: {}, min_score: null, prior_score: null };
}

/** Convert a node to another type, keeping children where it makes sense. */
export function convertNode(e: Expr, to: NodeType, fields: FieldDef[]): Expr {
  if (e.op === to) return e;
  const fresh = newNode(to, fields);
  if ((to === 'and' || to === 'or') && (e.op === 'and' || e.op === 'or')) return { op: to, args: e.args };
  if ((to === 'and' || to === 'or') && e.op !== 'and' && e.op !== 'or') return { op: to, args: [e] };
  if (to === 'not') return { op: 'not', arg: e };
  return fresh;
}

const operandText = (o: Operand, byKey: Record<string, FieldDef>): string => {
  if ('field' in o) {
    const who = o.field.of === 'a' ? 'A' : 'B';
    return `${who}.${byKey[o.field.key]?.label ?? o.field.key}`;
  }
  if ('lit' in o) return Array.isArray(o.lit.value) ? (o.lit.value as string[]).join(', ') : String(o.lit.value);
  const base = operandText(o.offset.base, byKey);
  return o.offset.by === 0 ? base : `${base} ${o.offset.by > 0 ? '+' : '−'} ${Math.abs(o.offset.by)}`;
};

/** One-line summary of an expression for collapsed rule cards. */
export function describeExpr(e: Expr, byKey: Record<string, FieldDef>): string {
  switch (e.op) {
    case 'and':
      return e.args.map((a) => describeExpr(a, byKey)).join(' AND ');
    case 'or':
      return '(' + e.args.map((a) => describeExpr(a, byKey)).join(' OR ') + ')';
    case 'not':
      return `NOT (${describeExpr(e.arg, byKey)})`;
    case 'if':
      return `IF ${describeExpr(e.when, byKey)} THEN ${describeExpr(e.then, byKey)}`;
    case 'exists':
      return `${operandText(e.value, byKey)} is filled in`;
    case 'between':
      return `${operandText(e.value, byKey)} between ${operandText(e.lo, byKey)} and ${operandText(e.hi, byKey)}`;
    case 'cmp':
      return `${operandText(e.left, byKey)} ${CMP_LABEL[e.cmp]} ${operandText(e.right, byKey)}`;
  }
}

/** For a literal next to a choice field, offer that field's options. */
export function hintField(other: Operand | undefined, byKey: Record<string, FieldDef>): FieldDef | undefined {
  if (!other || !('field' in other)) return undefined;
  const d = byKey[other.field.key];
  return d && ['choice', 'multi_choice'].includes(kindName(d.kind)) ? d : undefined;
}
