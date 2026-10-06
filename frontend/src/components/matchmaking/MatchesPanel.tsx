'use client';

import React, { useEffect, useState } from 'react';
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

  useEffect(() => {
    (async () => {
      try {
        const s = await invoke<RuleSetSummary[]>('mm_list_rule_sets', { includeArchived: false });
        setSets(s);
        if (s.length > 0) setSetId(s[0].id);
      } catch (e) {
        toast.error(`Could not load rule sets: ${e}`);
      }
    })();
  }, []);

  const run = async () => {
    if (!setId) return;
    setBusy(true);
    try {
      setRows(await invoke<MatchCandidate[]>('mm_find_matches', { profileId, ruleSetId: setId, limit: 50, includeIneligible: showExcluded }));
    } catch (e) {
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
        <select className="border rounded-md px-2 py-1.5 text-sm bg-white" value={setId} onChange={(e) => { setSetId(e.target.value); setRows(null); }}>
          {sets.map((s) => <option key={s.id} value={s.id}>{s.name}</option>)}
        </select>
        <label className="flex items-center gap-1 text-sm">
          <input type="checkbox" checked={showExcluded} onChange={(e) => { setShowExcluded(e.target.checked); setRows(null); }} /> show excluded
        </label>
        <Button size="sm" onClick={run} disabled={!setId || busy}>{busy ? 'Working…' : 'Find matches'}</Button>
      </div>

      {rows && rows.length === 0 && <p className="p-4 text-sm text-gray-500">No candidates{showExcluded ? '' : ' pass the rules'}.</p>}
      <div className="divide-y">
        {rows?.map((m) => (
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
    </div>
  );
}
