import React from 'react';
import { DimStatus, DimensionScore, Finding, ScoreCard } from '@/types/matchmaking';
import { useI18n } from '@/i18n';
import type { MessageKey } from '@/i18n/types';

const STATUS: Record<DimStatus, { label: MessageKey; chip: string; bar: string }> = {
  strong: { label: 'scoreCard.strong', chip: 'bg-green-100 text-green-800', bar: 'bg-green-500' },
  mixed: { label: 'scoreCard.mixed', chip: 'bg-yellow-100 text-yellow-800', bar: 'bg-yellow-500' },
  concern: { label: 'scoreCard.concern', chip: 'bg-red-100 text-red-800', bar: 'bg-red-500' },
  clear: { label: 'scoreCard.clear', chip: 'bg-green-100 text-green-800', bar: 'bg-green-300' },
  unknown: { label: 'scoreCard.unknown', chip: 'bg-amber-100 text-amber-800', bar: 'bg-amber-300' },
  not_applicable: { label: 'scoreCard.notApplicable', chip: 'bg-gray-100 text-gray-600', bar: 'bg-gray-300' },
  not_assessed: { label: 'scoreCard.notAssessed', chip: 'bg-gray-100 text-gray-500', bar: 'bg-gray-200' },
};

function Missing({ f, nameA, nameB }: { f: Finding; nameA: string; nameB: string }) {
  const { t } = useI18n();
  if (f.missing.length === 0) return null;
  const items = f.missing.map((m) => t('scoreCard.missingItem', { name: m.who === 'a' ? nameA : nameB, field: m.field.replace(/_/g, ' ') })).join('، ');
  return (
    <span className="text-xs text-amber-700">
      {' '}<bdi>{t('scoreCard.missing', { items })}</bdi>
    </span>
  );
}

function DimensionRow({ d, labelOf, pct }: { d: DimensionScore; labelOf: (k: string) => string; pct: (x: number | null) => string }) {
  const { t, fmt } = useI18n();
  const st = STATUS[d.status];
  const hasScore = d.score != null;
  return (
    <div className="flex items-center gap-3 py-1.5" title={d.description}>
      <div className="w-56 text-sm">{labelOf(d.key)}</div>
      <div className="flex-1 h-2.5 bg-gray-100 rounded overflow-hidden">
        {hasScore && <div className={`h-full ${st.bar}`} style={{ width: `${Math.max(2, Math.min(100, d.score ?? 0))}%` }} />}
      </div>
      <div className="w-10 text-end text-sm font-medium">{pct(d.score)}</div>
      <span className={`w-28 text-center text-xs px-2 py-0.5 rounded ${st.chip}`}>{t(st.label)}</span>
      <div className="w-24 text-xs text-gray-500 text-end">
        {d.coverage != null && d.status !== 'not_assessed' ? t('scoreCard.known', { percent: fmt.percent(d.coverage) }) : ''}
      </div>
    </div>
  );
}

/** Dimension-by-dimension compatibility, with strengths, concerns, possible deal-breakers and gaps in information. */
export function ScoreCardView({ card, nameA, nameB }: { card: ScoreCard; nameA: string; nameB: string }) {
  const { t, fmt } = useI18n();
  const pct = (x: number | null) => (x == null ? t('scoreCard.na') : fmt.number(Math.round(x)));
  const labelOf = (k: string) => card.dimensions.find((d) => d.key === k)?.label ?? k;
  const hc = card.hard_constraints;
  const hcStyle = hc.status === 'pass' ? 'bg-green-100 text-green-800' : hc.status === 'fail' ? 'bg-red-100 text-red-800' : 'bg-amber-100 text-amber-800';
  const hcText = hc.status === 'pass' ? t('scoreCard.pass') : hc.status === 'fail' ? t('scoreCard.fail') : t('scoreCard.needsInfo');
  const src = (f: Finding) => (f.source === 'preference_a' ? ` ${t('scoreCard.prefOf', { name: nameA })}` : f.source === 'preference_b' ? ` ${t('scoreCard.prefOf', { name: nameB })}` : f.direction === 'a_to_b' ? ` ${t('scoreCard.sideOf', { name: nameA })}` : f.direction === 'b_to_a' ? ` ${t('scoreCard.sideOf', { name: nameB })}` : '');

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-end gap-6">
        <div>
          <div className="text-xs text-gray-500">{t('scoreCard.matchScore')}</div>
          <div className="text-4xl font-semibold">{pct(card.ranking_score)}<span className="text-lg text-gray-400">{t('scoreCard.outOf', { max: fmt.number(100) })}</span></div>
        </div>
        <div className="text-sm text-gray-600 space-y-0.5">
          <div>{t('scoreCard.rawAverage', { value: pct(card.overall) })}</div>
          <div>{t('scoreCard.confidence', { value: card.confidence != null ? fmt.percent(card.confidence) : t('scoreCard.na') })} <span className="text-gray-400">{t('scoreCard.confidenceHint')}</span></div>
        </div>
        <div className="flex items-center gap-2">
          <span className="text-sm">{t('scoreCard.hardConstraints')}</span>
          <span className={`px-2 py-0.5 rounded text-sm font-medium ${hcStyle}`}>{hcText}</span>
          {card.meets_threshold === false && <span className="text-sm text-red-700">{t('scoreCard.belowMin')}</span>}
        </div>
      </div>

      <div className="bg-white rounded-lg border p-4">
        <div className="font-medium mb-1">{t('scoreCard.dimensions')}</div>
        <div className="divide-y">
          {card.dimensions.map((d) => <DimensionRow key={d.key} d={d} labelOf={labelOf} pct={pct} />)}
        </div>
      </div>

      <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
        <div className="bg-white rounded-lg border p-4">
          <div className="font-medium mb-1 text-green-800">{t('scoreCard.strengths')}</div>
          {card.strengths.length === 0 && <p className="text-sm text-gray-500">{t('scoreCard.nothingScored')}</p>}
          <ul className="space-y-1 text-sm">
            {card.strengths.map((f, i) => <li key={i} dir="auto">✓ {f.description}<span className="text-gray-400">{src(f)} · {labelOf(f.dimension)}</span></li>)}
          </ul>
        </div>
        <div className="bg-white rounded-lg border p-4">
          <div className="font-medium mb-1 text-amber-800">{t('scoreCard.concerns')}</div>
          {card.concerns.length === 0 && <p className="text-sm text-gray-500">{t('scoreCard.noneFound')}</p>}
          <ul className="space-y-1 text-sm">
            {card.concerns.map((f, i) => <li key={i} dir="auto">⚠ {f.description}<span className="text-gray-400">{src(f)} · {labelOf(f.dimension)}</span></li>)}
          </ul>
        </div>
        <div className="bg-white rounded-lg border p-4">
          <div className="font-medium mb-1 text-red-800">{t('scoreCard.dealBreakers')}</div>
          {hc.violations.length === 0 && <p className="text-sm text-gray-500">{t('scoreCard.noneFound')}</p>}
          <ul className="space-y-1 text-sm">
            {hc.violations.map((f, i) => <li key={i} dir="auto">✗ {f.description}<span className="text-gray-400">{src(f)}</span></li>)}
          </ul>
        </div>
        <div className="bg-white rounded-lg border p-4">
          <div className="font-medium mb-1">{t('scoreCard.unknowns')}</div>
          {card.unknowns.length === 0 && <p className="text-sm text-gray-500">{t('scoreCard.nothingMissing')}</p>}
          <ul className="space-y-1 text-sm">
            {card.unknowns.map((f, i) => (
              <li key={i} dir="auto">? {f.description}<span className="text-gray-400">{src(f)}</span><Missing f={f} nameA={nameA} nameB={nameB} /></li>
            ))}
          </ul>
        </div>
      </div>
    </div>
  );
}
