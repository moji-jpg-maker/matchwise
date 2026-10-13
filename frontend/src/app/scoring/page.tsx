'use client';

import React, { useCallback, useEffect, useState } from 'react';
import { useRouter } from 'next/navigation';
import { invoke } from '@tauri-apps/api/core';
import { ArrowLeft, Plus } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { RankingReadiness, ScoringConfigSummary } from '@/types/matchmaking';

export default function ScoringConfigsPage() {
  const router = useRouter();
  const [configs, setConfigs] = useState<ScoringConfigSummary[]>([]);
  const [readiness, setReadiness] = useState<RankingReadiness | null>(null);
  const [showArchived, setShowArchived] = useState(false);
  const [loading, setLoading] = useState(true);

  const load = useCallback(async () => {
    try {
      setConfigs(await invoke<ScoringConfigSummary[]>('mm_list_scoring_configs', { includeArchived: showArchived }));
    } catch (e) {
      toast.error(`Could not load scoring configs: ${e}`);
    } finally {
      setLoading(false);
    }
  }, [showArchived]);

  useEffect(() => { void load(); }, [load]);

  // The gate is read-only and independent of the list, so a failure here must not hide the configs.
  useEffect(() => {
    invoke<RankingReadiness>('mm_ranking_readiness').then(setReadiness).catch(() => setReadiness(null));
  }, []);

  const toggleArchive = async (c: ScoringConfigSummary) => {
    try {
      await invoke('mm_archive_scoring_config', { id: c.id, archived: !c.archived });
      await load();
    } catch (e) {
      toast.error(`${e}`);
    }
  };

  return (
    <div className="min-h-screen bg-gray-50">
      <div className="max-w-4xl mx-auto p-6 space-y-5">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-3">
            <button onClick={() => router.push('/')} className="p-2 rounded-lg hover:bg-gray-200" aria-label="Back">
              <ArrowLeft className="w-5 h-5" />
            </button>
            <div>
              <h1 className="text-2xl font-semibold">Scoring</h1>
              <p className="text-sm text-gray-500">
                How dimension scores are read: the strong and mixed cutoffs, and the neutral baseline thin evidence is
                pulled towards. A rule set says what to measure; a scoring config says how to read the result.
              </p>
            </div>
          </div>
          <Button onClick={() => router.push('/scoring-config?id=new')}>
            <Plus className="w-4 h-4 mr-1" /> New config
          </Button>
        </div>

        <label className="flex items-center gap-2 text-sm">
          <input type="checkbox" checked={showArchived} onChange={(e) => setShowArchived(e.target.checked)} />
          Show archived
        </label>

        <div className="bg-white rounded-lg border divide-y">
          {configs.length === 0 && !loading && <div className="p-6 text-center text-sm text-gray-500">No scoring configs yet.</div>}
          {configs.map((c) => (
            <div key={c.id} className="p-4 flex items-center gap-4">
              <button className="flex-1 min-w-0 text-left" onClick={() => router.push(`/scoring-config?id=${encodeURIComponent(c.id)}`)}>
                <div className="font-medium">
                  {c.name} {c.archived && <span className="ml-2 text-xs px-2 py-0.5 rounded bg-gray-200">archived</span>}
                </div>
                <div className="text-sm text-gray-500 truncate">{c.description || 'No description'}</div>
              </button>
              <div className="text-sm text-gray-600 w-52 text-right">
                strong ≥ {c.definition.strong_threshold} · mixed ≥ {c.definition.mixed_threshold} · baseline {c.definition.prior_score}
              </div>
              <div className="text-sm text-gray-500 w-32 text-right">
                v{c.current_version} · used by {c.used_by}
              </div>
              <Button variant="ghost" size="sm" onClick={() => toggleArchive(c)}>
                {c.archived ? 'Restore' : 'Archive'}
              </Button>
            </div>
          ))}
        </div>

        <div className="bg-white rounded-lg border p-5 space-y-3">
          <div>
            <h2 className="font-medium">Ranking gate</h2>
            <p className="text-sm text-gray-500">
              This reports only. Nothing here produces an ordering, fits a model or re-orders a search — the ranking
              stays fully deterministic until the gate is reached and a learned ranking can be compared against it.
            </p>
          </div>
          {!readiness ? (
            <p className="text-sm text-gray-500">Reading…</p>
          ) : (
            <>
              <p className={`text-sm ${readiness.ready ? 'text-green-800' : 'text-amber-700'}`}>{readiness.state}</p>
              <div className="grid grid-cols-2 sm:grid-cols-4 gap-3">
                <Stat label="Finished matches" value={readiness.finished} />
                <Stat label="Needed for the gate" value={readiness.minimum_finished} />
                <Stat label="With a stored scorecard" value={readiness.with_snapshot} />
                <Stat label="Candidates shown" value={readiness.pairs_shown} />
              </div>
              <p className="text-xs text-gray-500">
                {readiness.exposures} exposure rows across {readiness.pairs_shown} shown pairs; {readiness.pairs_with_outcome} of
                those pairs later produced an outcome. Exposures are recorded so what was on screen is known, not only what
                was decided — deciding from outcomes alone would teach the matchmaker&apos;s browsing order as if it were
                preference.
              </p>
              {Object.keys(readiness.outcome_counts).length > 0 && (
                <div>
                  <div className="text-sm font-medium mb-1">Outcomes</div>
                  <div className="flex flex-wrap gap-2 text-xs">
                    {Object.entries(readiness.outcome_counts).map(([k, n]) => (
                      <span key={k} className="px-2 py-1 rounded bg-gray-100">
                        {k.replace(/_/g, ' ')} <span className="text-gray-500">{n}</span>
                      </span>
                    ))}
                  </div>
                </div>
              )}
            </>
          )}
        </div>
      </div>
    </div>
  );
}

function Stat({ label, value }: { label: string; value: number }) {
  return (
    <div className="rounded border bg-gray-50 p-3">
      <div className="text-xl font-semibold">{value}</div>
      <div className="text-xs text-gray-500">{label}</div>
    </div>
  );
}
