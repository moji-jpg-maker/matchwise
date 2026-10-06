import React from 'react';
import { DimStatus, DimensionScore, Finding, ScoreCard } from '@/types/matchmaking';

const STATUS: Record<DimStatus, { label: string; chip: string; bar: string }> = {
  strong: { label: 'Strong', chip: 'bg-green-100 text-green-800', bar: 'bg-green-500' },
  mixed: { label: 'Mixed', chip: 'bg-yellow-100 text-yellow-800', bar: 'bg-yellow-500' },
  concern: { label: 'Concern', chip: 'bg-red-100 text-red-800', bar: 'bg-red-500' },
  clear: { label: 'No problems', chip: 'bg-green-100 text-green-800', bar: 'bg-green-300' },
  unknown: { label: 'Unknown', chip: 'bg-amber-100 text-amber-800', bar: 'bg-amber-300' },
  not_applicable: { label: 'Not applicable', chip: 'bg-gray-100 text-gray-600', bar: 'bg-gray-300' },
  not_assessed: { label: 'Not assessed', chip: 'bg-gray-100 text-gray-500', bar: 'bg-gray-200' },
};

const pct = (x: number | null) => (x == null ? 'n/a' : `${Math.round(x)}`);

function Missing({ f, nameA, nameB }: { f: Finding; nameA: string; nameB: string }) {
  if (f.missing.length === 0) return null;
  return (
    <span className="text-xs text-amber-700">
      {' '}missing: {f.missing.map((m) => `${m.who === 'a' ? nameA : nameB}'s ${m.field.replace(/_/g, ' ')}`).join(', ')}
    </span>
  );
}

function DimensionRow({ d, labelOf }: { d: DimensionScore; labelOf: (k: string) => string }) {
  const st = STATUS[d.status];
  const hasScore = d.score != null;
  return (
    <div className="flex items-center gap-3 py-1.5" title={d.description}>
      <div className="w-56 text-sm">{labelOf(d.key)}</div>
      <div className="flex-1 h-2.5 bg-gray-100 rounded overflow-hidden">
        {hasScore && <div className={`h-full ${st.bar}`} style={{ width: `${Math.max(2, Math.min(100, d.score ?? 0))}%` }} />}
      </div>
      <div className="w-10 text-right text-sm font-medium">{pct(d.score)}</div>
      <span className={`w-28 text-center text-xs px-2 py-0.5 rounded ${st.chip}`}>{st.label}</span>
      <div className="w-24 text-xs text-gray-500 text-right">
        {d.coverage != null && d.status !== 'not_assessed' ? `${Math.round(d.coverage * 100)}% known` : ''}
      </div>
    </div>
  );
}

/** Dimension-by-dimension compatibility, with strengths, concerns, possible deal-breakers and gaps in information. */
export function ScoreCardView({ card, nameA, nameB }: { card: ScoreCard; nameA: string; nameB: string }) {
  const labelOf = (k: string) => card.dimensions.find((d) => d.key === k)?.label ?? k;
  const hc = card.hard_constraints;
  const hcStyle = hc.status === 'pass' ? 'bg-green-100 text-green-800' : hc.status === 'fail' ? 'bg-red-100 text-red-800' : 'bg-amber-100 text-amber-800';
  const hcText = hc.status === 'pass' ? 'PASS' : hc.status === 'fail' ? 'FAIL' : 'NEEDS INFO';
  const src = (f: Finding) => (f.source === 'preference_a' ? ` [${nameA}'s preference]` : f.source === 'preference_b' ? ` [${nameB}'s preference]` : f.direction === 'a_to_b' ? ` (${nameA}'s side)` : f.direction === 'b_to_a' ? ` (${nameB}'s side)` : '');

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-end gap-6">
        <div>
          <div className="text-xs text-gray-500">Match score</div>
          <div className="text-4xl font-semibold">{pct(card.ranking_score)}<span className="text-lg text-gray-400">/100</span></div>
        </div>
        <div className="text-sm text-gray-600 space-y-0.5">
          <div>Raw average of the dimensions: {pct(card.overall)}</div>
          <div>Confidence: {card.confidence != null ? `${Math.round(card.confidence * 100)}%` : 'n/a'} <span className="text-gray-400">(scores resting on little information are pulled towards neutral)</span></div>
        </div>
        <div className="flex items-center gap-2">
          <span className="text-sm">Hard constraints</span>
          <span className={`px-2 py-0.5 rounded text-sm font-medium ${hcStyle}`}>{hcText}</span>
          {card.meets_threshold === false && <span className="text-sm text-red-700">below the minimum score</span>}
        </div>
      </div>

      <div className="bg-white rounded-lg border p-4">
        <div className="font-medium mb-1">Dimensions</div>
        <div className="divide-y">
          {card.dimensions.map((d) => <DimensionRow key={d.key} d={d} labelOf={labelOf} />)}
        </div>
      </div>

      <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
        <div className="bg-white rounded-lg border p-4">
          <div className="font-medium mb-1 text-green-800">Strong compatibility</div>
          {card.strengths.length === 0 && <p className="text-sm text-gray-500">Nothing scored yet.</p>}
          <ul className="space-y-1 text-sm">
            {card.strengths.map((f, i) => <li key={i}>✓ {f.description}<span className="text-gray-400">{src(f)} · {labelOf(f.dimension)}</span></li>)}
          </ul>
        </div>
        <div className="bg-white rounded-lg border p-4">
          <div className="font-medium mb-1 text-amber-800">Potential concerns</div>
          {card.concerns.length === 0 && <p className="text-sm text-gray-500">None found.</p>}
          <ul className="space-y-1 text-sm">
            {card.concerns.map((f, i) => <li key={i}>⚠ {f.description}<span className="text-gray-400">{src(f)} · {labelOf(f.dimension)}</span></li>)}
          </ul>
        </div>
        <div className="bg-white rounded-lg border p-4">
          <div className="font-medium mb-1 text-red-800">Possible deal-breakers</div>
          {hc.violations.length === 0 && <p className="text-sm text-gray-500">None found.</p>}
          <ul className="space-y-1 text-sm">
            {hc.violations.map((f, i) => <li key={i}>✗ {f.description}<span className="text-gray-400">{src(f)}</span></li>)}
          </ul>
        </div>
        <div className="bg-white rounded-lg border p-4">
          <div className="font-medium mb-1">Unknown information</div>
          {card.unknowns.length === 0 && <p className="text-sm text-gray-500">Nothing missing.</p>}
          <ul className="space-y-1 text-sm">
            {card.unknowns.map((f, i) => (
              <li key={i}>? {f.description}<span className="text-gray-400">{src(f)}</span><Missing f={f} nameA={nameA} nameB={nameB} /></li>
            ))}
          </ul>
        </div>
      </div>
    </div>
  );
}
