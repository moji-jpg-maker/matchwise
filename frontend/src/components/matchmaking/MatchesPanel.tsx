'use client';

import React, { useCallback, useEffect, useState } from 'react';
import { useRouter } from 'next/navigation';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { MatchCandidate, RuleSetSummary } from '@/types/matchmaking';

export function MatchesPanel({ profileId }: { profileId: string }) {
  const router = useRouter();
  const [sets, setSets] = useState<RuleSetSummary[]>([]);
  const [setId, setSetId] = useState('');
  const [showExcluded, setShowExcluded] = useState(false);
  const [rows, setRows] = useState<MatchCandidate[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [limit, setLimit] = useState(50);
  const [total, setTotal] = useState<number | null>(null);
  const [filter, setFilter] = useState('');
  const [setsLoaded, setSetsLoaded] = useState(false);
  const [setsError, setSetsError] = useState<string | null>(null);
  const [findError, setFindError] = useState<string | null>(null);

  const loadSets = useCallback(async () => {
    setSetsError(null);
    try {
      const s = await invoke<RuleSetSummary[]>('mm_list_rule_sets', { includeArchived: false });
      setSets(s);
      if (s.length > 0) setSetId((current) => current || s[0].id);
    } catch (e) {
      setSetsError(`${e}`);
    } finally {
      setSetsLoaded(true);
    }
  }, []);

  useEffect(() => { void loadSets(); }, [loadSets]);

  const run = async (nextLimit = 50) => {
    if (!setId) return;
    setBusy(true);
    setLimit(nextLimit);
    setFindError(null);
    try {
      const res = await invoke<{ candidates: MatchCandidate[]; total: number }>('mm_find_matches', { profileId, ruleSetId: setId, limit: nextLimit, includeIneligible: showExcluded });
      setRows(res.candidates);
      setTotal(res.total);
    } catch (e) {
      setFindError(`${e}`);
      toast.error(`${e}`);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="bg-white rounded-lg border">
      <div className="p-4 border-b flex flex-wrap items-center gap-3">
        <div className="flex-1">
          <h2 className="font-medium">Potential matches</h2>
          <p className="text-xs text-gray-500">Ranked by the confidence-adjusted score from a rule set plus both people's partner preferences. Click a row for the full match view. Save any edits above first.</p>
        </div>
        <select className="border rounded-md px-2 py-1.5 text-sm bg-white" value={setId} onChange={(e) => { setSetId(e.target.value); setRows(null); setFindError(null); }}>
          {sets.map((s) => <option key={s.id} value={s.id}>{s.name}</option>)}
        </select>
        <label className="flex items-center gap-1 text-sm">
          <input type="checkbox" checked={showExcluded} onChange={(e) => { setShowExcluded(e.target.checked); setRows(null); setFindError(null); }} /> show excluded
        </label>
        <Button size="sm" onClick={() => run()} disabled={!setId || busy}>{busy ? 'Working…' : 'Find matches'}</Button>
      </div>

      {setsError && (
        <div className="p-4 flex items-center gap-3 text-sm text-red-700 bg-red-50 border-b border-red-200">
          <span className="flex-1">Could not load rule sets: {setsError}</span>
          <Button size="sm" variant="outline" onClick={() => void loadSets()}>Retry</Button>
        </div>
      )}
      {findError && (
        <div className="p-4 flex items-center gap-3 text-sm text-red-700 bg-red-50 border-b border-red-200">
          <span className="flex-1">Could not find matches: {findError}</span>
          <Button size="sm" variant="outline" onClick={() => run(limit)} disabled={busy}>Retry</Button>
        </div>
      )}
      {setsLoaded && !setsError && sets.length === 0 && (
        <p className="p-4 text-sm text-gray-500">No rule sets yet. <button className="underline" onClick={() => router.push('/rules')}>Create one</button> to find matches.</p>
      )}
      {rows && rows.length > 0 && (
        <div className="p-3 border-b">
          <input className="border rounded-md px-3 py-1.5 text-sm w-full" placeholder="Filter candidates by name or city" aria-label="Filter candidates" value={filter} onChange={(e) => setFilter(e.target.value)} />
        </div>
      )}
      {rows && rows.length === 0 && <p className="p-4 text-sm text-gray-500">No candidates{showExcluded ? '' : ' pass the rules'}.</p>}
      <div className="divide-y">
        {rows?.filter((m) => !filter.trim() || `${m.name ?? ''} ${m.city ?? ''}`.toLowerCase().includes(filter.trim().toLowerCase())).map((m) => (
          <button key={m.id} onClick={() => router.push(`/match?a=${encodeURIComponent(profileId)}&b=${encodeURIComponent(m.id)}&set=${encodeURIComponent(setId)}`)} className="w-full text-left p-4 hover:bg-gray-50" title="Open the match view">
            <div className="flex items-center gap-3">
              <div className="flex-1 min-w-0">
                <div className="font-medium truncate">{m.name || 'Unnamed profile'}</div>
                <div className="text-sm text-gray-500">{[m.age != null ? `${m.age}` : null, m.city].filter(Boolean).join(' · ')}</div>
              </div>
              {!m.eligible && <span className="text-xs px-2 py-0.5 rounded bg-red-100 text-red-800">excluded</span>}
              {m.needs_info && <span className="text-xs px-2 py-0.5 rounded bg-amber-100 text-amber-800">missing info</span>}
              <div className="text-right w-44">
                <div className="font-semibold">{m.score != null ? `${Math.round(m.score)}/100` : 'n/a'}</div>
                <div className="text-xs text-gray-500">confidence {m.confidence != null ? Math.round(m.confidence * 100) : 0}%</div>
              </div>
            </div>
            {(m.strengths.length > 0 || m.concerns.length > 0) && (
              <div className="mt-1 text-xs text-gray-600 space-y-0.5">
                {m.strengths.map((t, k) => <div key={`s${k}`}>✓ {t}</div>)}
                {m.concerns.map((t, k) => <div key={`c${k}`} className="text-amber-700">⚠ {t}</div>)}
              </div>
            )}
            {m.blocking.length > 0 && (
              <ul className="mt-2 text-sm text-red-700 list-disc pl-5">
                {m.blocking.map((b, k) => <li key={k}>{b}</li>)}
              </ul>
            )}
          </button>
        ))}
      </div>
      {rows && total !== null && rows.length < total && limit < 500 && (
        <div className="p-3 text-center border-t">
          <p className="text-xs text-gray-500 mb-1">Showing {rows.length} of {total}.</p>
          <Button size="sm" variant="ghost" disabled={busy} onClick={() => run(Math.min(limit + 50, 500))}>Load more</Button>
        </div>
      )}
      {rows && total !== null && rows.length < total && limit >= 500 && (
        <p className="p-3 text-center border-t text-xs text-gray-500">Showing {rows.length} of {total}; 500 is the most that can be shown.</p>
      )}
    </div>
  );
}
