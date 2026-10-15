import React from 'react';
import { RuleResult } from '@/types/matchmaking';
import { useI18n } from '@/i18n';
import type { MessageKey } from '@/i18n/types';

const DIRECTION_KEY: Record<RuleResult['direction'], MessageKey | null> = { pair: null, a_to_b: 'ruleResult.aToB', b_to_a: 'ruleResult.bToA' };

export function ruleResultLabel(r: RuleResult): { key: MessageKey; style: string } {
  const key: MessageKey = !r.applicable
    ? 'ruleResult.skipped'
    : r.result === 'unknown'
      ? 'ruleResult.unknown'
      : r.kind === 'hard'
        ? r.result === 'true' ? 'ruleResult.ok' : 'ruleResult.violated'
        : r.result === 'true' ? 'ruleResult.met' : 'ruleResult.notMet';
  const style = !r.applicable
    ? 'bg-gray-100 text-gray-600'
    : r.result === 'true'
      ? 'bg-green-100 text-green-800'
      : r.result === 'false'
        ? 'bg-red-100 text-red-800'
        : 'bg-amber-100 text-amber-800';
  return { key, style };
}

export function RuleResultList({ title, results }: { title: string; results: RuleResult[] }) {
  const { t, fmt } = useI18n();
  if (results.length === 0) return null;
  return (
    <div>
      <div className="font-medium mb-1">{title}</div>
      <ul className="space-y-1">
        {results.map((r, k) => {
          const { key, style } = ruleResultLabel(r);
          const directionKey = DIRECTION_KEY[r.direction];
          return (
            <li key={k} className="flex items-center gap-2">
              <span className={`px-1.5 py-0.5 rounded text-xs min-w-16 px-2 text-center ${style}`}>{t(key)}</span>
              <span dir="auto">{r.description}{directionKey ? ` ${t(directionKey)}` : ''}</span>
              {r.kind === 'soft' && r.applicable && <span className="text-xs text-gray-400">{t('ruleResult.weight', { weight: fmt.number(r.weight) })}</span>}
            </li>
          );
        })}
      </ul>
    </div>
  );
}
