'use client';

import React from 'react';
import { Plus, X } from 'lucide-react';
import { Input } from '@/components/ui/input';
import { CmpOp, Expr, FieldDef, Operand, Side, Value, kindName, kindOptions } from '@/types/matchmaking';
import { CMP_LABEL, NODE_TYPES, NodeType, convertNode, fieldOperand, hintField, litOperand, newNode } from '@/lib/rules';
import { isConditionField } from '@/lib/conditions';

const sel = 'border rounded-md px-2 py-1.5 text-sm bg-white';

interface FieldsProps {
  fields: FieldDef[];
  byKey: Record<string, FieldDef>;
}

/** Which person a field is read from. Directional rules read "A's partner is B". */
const SIDE_LABEL: Record<Side, string> = { a: 'Person A', b: 'Person B' };

type LitKind = 'number' | 'text' | 'bool' | 'list';
const litKind = (v: Value): LitKind => (typeof v === 'number' ? 'number' : typeof v === 'boolean' ? 'bool' : Array.isArray(v) ? 'list' : 'text');

function OperandEditor({
  operand,
  onChange,
  fields,
  byKey,
  other,
  allowLiteral = true,
  allowOffset = true,
  numericOnly = false,
}: FieldsProps & {
  operand: Operand;
  onChange: (o: Operand) => void;
  other?: Operand;
  allowLiteral?: boolean;
  allowOffset?: boolean;
  numericOnly?: boolean;
}) {
  const usable = fields.filter(isConditionField).filter((f) => !numericOnly || kindName(f.kind) === 'number');
  const kind: 'field' | 'lit' | 'offset' = 'field' in operand ? 'field' : 'lit' in operand ? 'lit' : 'offset';
  const firstKey = usable[0]?.key ?? '';

  const fieldPicker = (of: Side, key: string, set: (of: Side, key: string) => void) => (
    <>
      <select className={sel} value={of} onChange={(e) => set(e.target.value as Side, key)}>
        <option value="a">{SIDE_LABEL.a}</option>
        <option value="b">{SIDE_LABEL.b}</option>
      </select>
      <select className={sel} value={key} onChange={(e) => set(of, e.target.value)}>
        {usable.map((f) => (
          <option key={f.key} value={f.key}>{f.label}</option>
        ))}
      </select>
    </>
  );

  let body: React.ReactNode = null;
  if ('field' in operand) {
    body = fieldPicker(operand.field.of, operand.field.key, (of, key) => onChange(fieldOperand(of, key)));
  } else if ('lit' in operand) {
    const v = operand.lit.value;
    const k = litKind(v);
    const hint = hintField(other, byKey);
    const set = (x: Value) => onChange(litOperand(x));
    body = (
      <>
        {!hint && (
          <select
            className={sel}
            value={k}
            onChange={(e) => {
              const nk = e.target.value as LitKind;
              set(nk === 'number' ? 0 : nk === 'bool' ? true : nk === 'list' ? [] : '');
            }}
          >
            <option value="number">number</option>
            <option value="text">text</option>
            <option value="bool">yes / no</option>
            <option value="list">list</option>
          </select>
        )}
        {k === 'number' && <Input className="w-24" type="number" value={String(v)} onChange={(e) => set(e.target.value === '' ? 0 : Number(e.target.value))} />}
        {k === 'bool' && (
          <select className={sel} value={String(v)} onChange={(e) => set(e.target.value === 'true')}>
            <option value="true">yes</option>
            <option value="false">no</option>
          </select>
        )}
        {k === 'text' &&
          (hint ? (
            <select className={sel} value={String(v)} onChange={(e) => set(e.target.value)}>
              <option value="">choose…</option>
              {kindOptions(hint.kind).map((o) => (
                <option key={o} value={o}>{o.replace(/_/g, ' ')}</option>
              ))}
            </select>
          ) : (
            <Input className="w-44" value={String(v)} onChange={(e) => set(e.target.value)} />
          ))}
        {k === 'list' &&
          (hint ? (
            <span className="flex flex-wrap gap-2">
              {kindOptions(hint.kind).map((o) => (
                <label key={o} className="flex items-center gap-1 text-sm">
                  <input
                    type="checkbox"
                    checked={(v as string[]).includes(o)}
                    onChange={(e) => set(e.target.checked ? [...(v as string[]), o] : (v as string[]).filter((x) => x !== o))}
                  />
                  {o.replace(/_/g, ' ')}
                </label>
              ))}
            </span>
          ) : (
            <Input className="w-56" placeholder="comma separated" value={(v as string[]).join(', ')}
              onChange={(e) => set(e.target.value.split(',').map((x) => x.trim()).filter(Boolean))} />
          ))}
      </>
    );
  } else {
    const base = operand.offset.base;
    const bf = 'field' in base ? base.field : { of: 'a' as Side, key: firstKey };
    body = (
      <>
        {fieldPicker(bf.of, bf.key, (of, key) => onChange({ offset: { base: fieldOperand(of, key), by: operand.offset.by } }))}
        <span className="text-sm text-gray-500">+</span>
        <Input className="w-20" type="number" value={String(operand.offset.by)}
          onChange={(e) => onChange({ offset: { base, by: e.target.value === '' ? 0 : Number(e.target.value) } })} />
      </>
    );
  }

  return (
    <span className="inline-flex flex-wrap items-center gap-1.5">
      {(allowLiteral || allowOffset) && (
        <select
          className={sel}
          value={kind}
          onChange={(e) => {
            const k = e.target.value;
            if (k === 'field') onChange(fieldOperand('b', firstKey));
            else if (k === 'lit') onChange(litOperand(numericOnly ? 0 : ''));
            else onChange({ offset: { base: fieldOperand('a', firstKey), by: 0 } });
          }}
        >
          <option value="field">field</option>
          {allowLiteral && <option value="lit">value</option>}
          {allowOffset && <option value="offset">field ± number</option>}
        </select>
      )}
      {body}
    </span>
  );
}

interface Props extends FieldsProps {
  expr: Expr;
  onChange: (e: Expr) => void;
  onRemove?: () => void;
  depth?: number;
}

/** Recursive editor for a rule condition tree. */
export function ExprEditor({ expr, onChange, onRemove, fields, byKey, depth = 0 }: Props) {
  const common = { fields, byKey };
  const typeSelect = (
    <select
      className={`${sel} font-medium`}
      value={expr.op}
      onChange={(e) => onChange(convertNode(expr, e.target.value as NodeType, fields))}
    >
      {NODE_TYPES.map((t) => (
        <option key={t.value} value={t.value}>{t.label}</option>
      ))}
    </select>
  );
  const remove = onRemove && (
    <button type="button" className="p-1 rounded hover:bg-gray-200 ml-auto" aria-label="Remove condition" onClick={onRemove}>
      <X className="w-4 h-4" />
    </button>
  );
  const box = `rounded-md border ${depth % 2 === 0 ? 'bg-gray-50' : 'bg-white'} p-2`;

  switch (expr.op) {
    case 'cmp':
      return (
        <div className={`${box} flex flex-wrap items-center gap-2`}>
          {typeSelect}
          <OperandEditor {...common} operand={expr.left} other={expr.right} onChange={(left) => onChange({ ...expr, left })} />
          <select className={sel} value={expr.cmp} onChange={(e) => onChange({ ...expr, cmp: e.target.value as CmpOp })}>
            {(Object.keys(CMP_LABEL) as CmpOp[]).map((c) => (
              <option key={c} value={c}>{CMP_LABEL[c]}</option>
            ))}
          </select>
          <OperandEditor {...common} operand={expr.right} other={expr.left} onChange={(right) => onChange({ ...expr, right })} />
          {remove}
        </div>
      );
    case 'between':
      return (
        <div className={`${box} flex flex-wrap items-center gap-2`}>
          {typeSelect}
          <OperandEditor {...common} operand={expr.value} allowLiteral={false} allowOffset={false} numericOnly onChange={(value) => onChange({ ...expr, value })} />
          <span className="text-sm text-gray-500">between</span>
          <OperandEditor {...common} operand={expr.lo} numericOnly onChange={(lo) => onChange({ ...expr, lo })} />
          <span className="text-sm text-gray-500">and</span>
          <OperandEditor {...common} operand={expr.hi} numericOnly onChange={(hi) => onChange({ ...expr, hi })} />
          {remove}
        </div>
      );
    case 'exists':
      return (
        <div className={`${box} flex flex-wrap items-center gap-2`}>
          {typeSelect}
          <OperandEditor {...common} operand={expr.value} allowLiteral={false} allowOffset={false} onChange={(value) => onChange({ ...expr, value })} />
          <span className="text-sm text-gray-500">is filled in</span>
          {remove}
        </div>
      );
    case 'not':
      return (
        <div className={`${box} space-y-2`}>
          <div className="flex items-center gap-2">{typeSelect}<span className="text-sm text-gray-500">this must NOT hold:</span>{remove}</div>
          <div className="pl-4">
            <ExprEditor {...common} expr={expr.arg} depth={depth + 1} onChange={(arg) => onChange({ ...expr, arg })} />
          </div>
        </div>
      );
    case 'if':
      return (
        <div className={`${box} space-y-2`}>
          <div className="flex items-center gap-2">{typeSelect}{remove}</div>
          <div className="pl-4 space-y-2">
            <div className="text-xs font-semibold text-gray-500">IF</div>
            <ExprEditor {...common} expr={expr.when} depth={depth + 1} onChange={(when) => onChange({ ...expr, when })} />
            <div className="text-xs font-semibold text-gray-500">THEN</div>
            <ExprEditor {...common} expr={expr.then} depth={depth + 1} onChange={(then) => onChange({ ...expr, then })} />
          </div>
        </div>
      );
    case 'and':
    case 'or': {
      const args = expr.args;
      const setArgs = (next: Expr[]) => onChange({ ...expr, args: next });
      return (
        <div className={`${box} space-y-2`}>
          <div className="flex items-center gap-2">
            {typeSelect}
            <span className="text-sm text-gray-500">{expr.op === 'and' ? 'every one of these must hold:' : 'at least one of these must hold:'}</span>
            {remove}
          </div>
          <div className="pl-4 space-y-2">
            {args.map((a, i) => (
              <ExprEditor
                key={i}
                {...common}
                expr={a}
                depth={depth + 1}
                onChange={(n) => setArgs(args.map((x, j) => (j === i ? n : x)))}
                onRemove={args.length > 1 ? () => setArgs(args.filter((_, j) => j !== i)) : undefined}
              />
            ))}
            <div className="flex gap-2">
              <button type="button" className="text-sm text-blue-600 hover:underline flex items-center" onClick={() => setArgs([...args, newNode('cmp', fields)])}>
                <Plus className="w-3.5 h-3.5 mr-0.5" /> condition
              </button>
              <button type="button" className="text-sm text-blue-600 hover:underline flex items-center" onClick={() => setArgs([...args, newNode('or', fields)])}>
                <Plus className="w-3.5 h-3.5 mr-0.5" /> group
              </button>
            </div>
          </div>
        </div>
      );
    }
  }
}
