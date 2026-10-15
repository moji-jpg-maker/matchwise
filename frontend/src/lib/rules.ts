import { CmpOp, Expr, FieldDef, Operand, Rule, RuleSet, Side, Value, kindName } from '@/types/matchmaking';
import { isConditionField } from '@/lib/conditions';
import type { MessageKey } from '@/i18n/types';

export type Translate = (key: MessageKey, params?: Record<string, string | number>) => string;

export const CMP_LABEL: Record<CmpOp, string> = {
  eq: '=',
  ne: '≠',
  lt: '<',
  le: '≤',
  gt: '>',
  ge: '≥',
  in: '',
};

export const cmpLabel = (op: CmpOp, t: Translate): string => (op === 'in' ? t('expr.cmpIn') : CMP_LABEL[op]);

export const NODE_TYPES = [
  { value: 'cmp', labelKey: 'expr.nodeCmp' },
  { value: 'between', labelKey: 'expr.nodeBetween' },
  { value: 'exists', labelKey: 'expr.nodeExists' },
  { value: 'and', labelKey: 'expr.nodeAnd' },
  { value: 'or', labelKey: 'expr.nodeOr' },
  { value: 'not', labelKey: 'expr.nodeNot' },
  { value: 'if', labelKey: 'expr.nodeIf' },
] as const satisfies readonly { value: string; labelKey: MessageKey }[];

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

const operandText = (o: Operand, byKey: Record<string, FieldDef>, t: Translate): string => {
  if ('field' in o) {
    const field = byKey[o.field.key]?.label ?? o.field.key;
    return t(o.field.of === 'a' ? 'expr.sideA' : 'expr.sideB', { field });
  }
  if ('lit' in o) return Array.isArray(o.lit.value) ? (o.lit.value as string[]).join(', ') : String(o.lit.value);
  const base = operandText(o.offset.base, byKey, t);
  return o.offset.by === 0 ? base : `${base} ${o.offset.by > 0 ? '+' : '−'} ${Math.abs(o.offset.by)}`;
};

/** One-line summary of an expression for collapsed rule cards. */
export function describeExpr(e: Expr, byKey: Record<string, FieldDef>, t: Translate): string {
  switch (e.op) {
    case 'and':
      return e.args.map((a) => describeExpr(a, byKey, t)).join(` ${t('expr.joinAnd')} `);
    case 'or':
      return '(' + e.args.map((a) => describeExpr(a, byKey, t)).join(` ${t('expr.joinOr')} `) + ')';
    case 'not':
      return t('expr.descNot', { arg: describeExpr(e.arg, byKey, t) });
    case 'if':
      return t('expr.descIf', { when: describeExpr(e.when, byKey, t), then: describeExpr(e.then, byKey, t) });
    case 'exists':
      return t('expr.descExists', { value: operandText(e.value, byKey, t) });
    case 'between':
      return t('expr.descBetween', { value: operandText(e.value, byKey, t), lo: operandText(e.lo, byKey, t), hi: operandText(e.hi, byKey, t) });
    case 'cmp':
      return t('expr.descCmp', { left: operandText(e.left, byKey, t), op: cmpLabel(e.cmp, t), right: operandText(e.right, byKey, t) });
  }
}

/** For a literal next to a choice field, offer that field's options. */
export function hintField(other: Operand | undefined, byKey: Record<string, FieldDef>): FieldDef | undefined {
  if (!other || !('field' in other)) return undefined;
  const d = byKey[other.field.key];
  return d && ['choice', 'multi_choice'].includes(kindName(d.kind)) ? d : undefined;
}
