'use client';

import React, { useCallback, useEffect, useState } from 'react';
import { useRouter } from 'next/navigation';
import { invoke } from '@tauri-apps/api/core';
import { ArrowLeft, Plus } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { OUTCOME_KEY } from '@/lib/workflow';
import { normalizeSearch } from '@/i18n/format';
import { useI18n } from '@/i18n';
import { ExposureSummary, OrderingResult, RankingComparison, RankingReadiness, ScoringConfigSummary } from '@/types/matchmaking';

export default function ScoringConfigsPage() {
  const { t, fmt } = useI18n();
  const router = useRouter();
  const [configs, setConfigs] = useState<ScoringConfigSummary[]>([]);
  const [readiness, setReadiness] = useState<RankingReadiness | null>(null);
  const [comparison, setComparison] = useState<RankingComparison | null>(null);
  const [exposure, setExposure] = useState<ExposureSummary | null>(null);
  const [showArchived, setShowArchived] = useState(false);
  const [query, setQuery] = useState('');
  const [loading, setLoading] = useState(true);

  const load = useCallback(async () => {
    try {
      setConfigs(await invoke<ScoringConfigSummary[]>('mm_list_scoring_configs', { includeArchived: showArchived }));
    } catch (e) {
      toast.error(t('scoring.loadError', { error: `${e}` }));
    } finally {
      setLoading(false);
    }
  }, [showArchived, t]);

  useEffect(() => { void load(); }, [load]);

  // The gate and the comparison are read-only and independent of the list, so a failure in either must not hide the
  // configs -- and the comparison is fetched alongside the gate rather than inside it, because the gate is a
  // threshold and this is a result.
  useEffect(() => {
    invoke<RankingReadiness>('mm_ranking_readiness').then(setReadiness).catch(() => setReadiness(null));
    invoke<RankingComparison>('mm_ranking_comparison').then(setComparison).catch(() => setComparison(null));
    invoke<ExposureSummary>('mm_exposure_summary').then(setExposure).catch(() => setExposure(null));
  }, []);

  const toggleArchive = async (c: ScoringConfigSummary) => {
    try {
      await invoke('mm_archive_scoring_config', { id: c.id, archived: !c.archived });
      await load();
    } catch (e) {
      toast.error(`${e}`);
    }
  };

  const needle = normalizeSearch(query.trim());
  const visible = needle
    ? configs.filter((c) => normalizeSearch(c.name).includes(needle) || normalizeSearch(c.description ?? '').includes(needle))
    : configs;

  return (
    <div className="min-h-screen bg-gray-50">
      <div className="max-w-4xl mx-auto p-6 space-y-5">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-3">
            <button onClick={() => router.push('/')} className="p-2 rounded-lg hover:bg-gray-200" aria-label={t('scoring.back')}>
              <ArrowLeft className="w-5 h-5 rtl-mirror" />
            </button>
            <div>
              <h1 className="text-2xl font-semibold">{t('scoring.title')}</h1>
              <p className="text-sm text-gray-500">
                {t('scoring.intro')}
              </p>
            </div>
          </div>
          <Button onClick={() => router.push('/scoring-config?id=new')}>
            <Plus className="w-4 h-4 me-1" /> {t('scoring.newConfig')}
          </Button>
        </div>

        <div className="flex items-center gap-4">
          <Input className="max-w-xs" dir="auto" placeholder={t('scoring.searchPlaceholder')} aria-label={t('scoring.searchPlaceholder')} value={query} onChange={(e) => setQuery(e.target.value)} />
          <label className="flex items-center gap-2 text-sm">
            <input type="checkbox" checked={showArchived} onChange={(e) => setShowArchived(e.target.checked)} />
            {t('scoring.showArchived')}
          </label>
          {query && (
            <Button variant="ghost" size="sm" onClick={() => setQuery('')}>{t('scoring.clear')}</Button>
          )}
        </div>

        <div className="bg-white rounded-lg border divide-y">
          {visible.length === 0 && !loading && (
            <div className="p-6 text-center text-sm text-gray-500">
              {configs.length === 0 ? t('scoring.emptyNone') : t('scoring.emptySearch')}
            </div>
          )}
          {visible.map((c) => (
            <div key={c.id} className="p-4 flex items-center gap-4">
              <button className="flex-1 min-w-0 text-start" onClick={() => router.push(`/scoring-config?id=${encodeURIComponent(c.id)}`)}>
                <div className="font-medium">
                  <bdi>{c.name}</bdi> {c.archived && <span className="ms-2 text-xs px-2 py-0.5 rounded bg-gray-200">{t('scoring.archived')}</span>}
                </div>
                <div className="text-sm text-gray-500 truncate"><bdi>{c.description || t('scoring.noDescription')}</bdi></div>
              </button>
              <div className="text-sm text-gray-600 w-52 text-end">
                {t('scoring.thresholds', { strong: fmt.number(c.definition.strong_threshold), mixed: fmt.number(c.definition.mixed_threshold), baseline: fmt.number(c.definition.prior_score) })}
              </div>
              <div className="text-sm text-gray-500 w-32 text-end">
                {t('scoring.versionUsed', { version: fmt.number(c.current_version), count: fmt.number(c.used_by) })}
              </div>
              <Button variant="ghost" size="sm" onClick={() => toggleArchive(c)}>
                {c.archived ? t('scoring.restore') : t('scoring.archive')}
              </Button>
            </div>
          ))}
        </div>

        <div className="bg-white rounded-lg border p-5 space-y-3">
          <div>
            <h2 className="font-medium">{t('scoring.gateTitle')}</h2>
            <p className="text-sm text-gray-500">
              {t('scoring.gateIntro')}
            </p>
          </div>
          {!readiness ? (
            <p className="text-sm text-gray-500">{t('scoring.reading')}</p>
          ) : (
            <>
              {exposure && (
                <p className="text-xs text-gray-500">
                  {t('scoring.exposureLine', {
                    searches: fmt.number(exposure.searches),
                    exposures: fmt.number(exposure.exposures),
                    since: exposure.first_at ? t('scoring.exposureSince', { date: fmt.date(exposure.first_at) }) : '',
                    last: exposure.last_at ? t('scoring.exposureLast', { date: fmt.date(exposure.last_at) }) : '',
                  })}
                </p>
              )}
              <p className={`text-sm ${readiness.ready ? 'text-green-800' : 'text-amber-700'}`}>{readiness.state}</p>
              <div className="grid grid-cols-2 sm:grid-cols-4 gap-3">
                <Stat label={t('scoring.statFinished')} value={fmt.number(readiness.finished)} />
                <Stat label={t('scoring.statNeeded')} value={fmt.number(readiness.minimum_finished)} />
                <Stat label={t('scoring.statSnapshot')} value={fmt.number(readiness.with_snapshot)} />
                <Stat label={t('scoring.statShown')} value={fmt.number(readiness.pairs_shown)} />
              </div>
              <p className="text-xs text-gray-500">
                {t('scoring.exposureNote', { exposures: fmt.number(readiness.exposures), shown: fmt.number(readiness.pairs_shown), withOutcome: fmt.number(readiness.pairs_with_outcome) })}
              </p>
              {Object.keys(readiness.outcome_counts).length > 0 && (
                <div>
                  <div className="text-sm font-medium mb-1">{t('scoring.outcomes')}</div>
                  <div className="flex flex-wrap gap-2 text-xs">
                    {Object.entries(readiness.outcome_counts).map(([k, n]) => (
                      <span key={k} className="px-2 py-1 rounded bg-gray-100">
                        {k in OUTCOME_KEY ? t(OUTCOME_KEY[k as keyof typeof OUTCOME_KEY]) : k.replace(/_/g, ' ')} <span className="text-gray-500">{fmt.number(n)}</span>
                      </span>
                    ))}
                  </div>
                </div>
              )}

              {comparison && <Comparison c={comparison} />}
            </>
          )}
        </div>
      </div>
    </div>
  );
}

/**
 * The two orderings against the recorded outcomes. Deliberately shows the counts beside every rate: a concordance of
 * 1.0 from three comparisons and one from three hundred are the same number and not the same evidence, and the gate
 * sentence above says whether this is yet enough to read as a finding.
 */
function Comparison({ c }: { c: RankingComparison }) {
  const { t, fmt } = useI18n();
  const rate = (r: number | null) => (r === null ? '—' : fmt.percent(r));
  const rows: { label: string; r: OrderingResult; inForce: boolean }[] = [
    { label: t('scoring.cmpAdjusted', { name: c.enforced }), r: c.baseline, inForce: true },
    { label: t('scoring.cmpRaw'), r: c.candidate, inForce: false },
  ];
  return (
    <div className="pt-3 border-t space-y-3">
      <div className="text-sm font-medium">{t('scoring.cmpTitle')}</div>
      <p className={`text-sm ${c.ready ? 'text-gray-500' : 'text-amber-700'}`}>{c.gate_note}</p>

      <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
        {rows.map(({ label, r, inForce }) => (
          <div key={label} className="rounded border p-3 space-y-1">
            <div className="flex items-center justify-between gap-2">
              <span className="text-sm font-medium">{label}</span>
              {inForce && <span className="text-xs px-2 py-0.5 rounded bg-gray-100">{t('scoring.inForce')}</span>}
            </div>
            <div className="text-2xl font-semibold">{rate(r.concordance)}</div>
            <div className="text-xs text-gray-500">
              {t(r.comparable_pairs === 1 ? 'scoring.cmpPairOne' : 'scoring.cmpPairMany', { count: fmt.number(r.comparable_pairs), agreed: fmt.number(r.concordant), discordant: fmt.number(r.discordant), tied: fmt.number(r.tied) })}
            </div>
            <div className="text-xs text-gray-500">
              {t('scoring.cmpTop', { progressed: fmt.number(r.progressed), topK: fmt.number(r.top_k), rate: rate(r.top_k_rate) })}
            </div>
          </div>
        ))}
      </div>

      <p className="text-xs text-gray-500">{c.classification}</p>
      <p className="text-xs text-gray-500">
        {t('scoring.cmpCoverage', { compared: fmt.number(c.matches_compared), total: fmt.number(c.matches_total), noOutcome: fmt.number(c.excluded.no_outcome), unknownOutcome: fmt.number(c.excluded.unknown_outcome), noSnapshot: fmt.number(c.excluded.no_snapshot), noScore: fmt.number(c.excluded.no_score) })}
      </p>
    </div>
  );
}

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded border bg-gray-50 p-3">
      <div className="text-xl font-semibold">{value}</div>
      <div className="text-xs text-gray-500">{label}</div>
    </div>
  );
}
