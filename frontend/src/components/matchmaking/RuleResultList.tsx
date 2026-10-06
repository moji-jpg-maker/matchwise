import React from 'react';
import { RuleResult } from '@/types/matchmaking';

const DIRECTION_LABEL = { pair: '', a_to_b: ' (A → B)', b_to_a: ' (B → A)' } as const;

export function ruleResultLabel(r: RuleResult): { text: string; style: string } {
  const text = !r.applicable
    ? 'skipped'
    : r.result === 'unknown'
      ? 'unknown'
      : r.kind === 'hard'
        ? r.result === 'true' ? 'ok' : 'violated'
        : r.result === 'true' ? 'met' : 'not met';
  const style = !r.applicable
    ? 'bg-gray-100 text-gray-600'
    : r.result === 'true'
      ? 'bg-green-100 text-green-800'
      : r.result === 'false'
        ? 'bg-red-100 text-red-800'
        : 'bg-amber-100 text-amber-800';
  return { text, style };
}

export function RuleResultList({ title, results }: { title: string; results: RuleResult[] }) {
  if (results.length === 0) return null;
  return (
    <div>
      <div className="font-medium mb-1">{title}</div>
      <ul className="space-y-1">
        {results.map((r, k) => {
          const { text, style } = ruleResultLabel(r);
          return (
            <li key={k} className="flex items-center gap-2">
              <span className={`px-1.5 py-0.5 rounded text-xs w-16 text-center ${style}`}>{text}</span>
              <span>{r.description}{DIRECTION_LABEL[r.direction]}</span>
              {r.kind === 'soft' && r.applicable && <span className="text-xs text-gray-400">weight {r.weight}</span>}
            </li>
          );
        })}
      </ul>
    </div>
  );
}
