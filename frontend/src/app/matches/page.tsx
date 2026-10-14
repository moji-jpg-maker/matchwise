'use client';

import React, { Suspense, useCallback, useEffect, useMemo, useState } from 'react';
import { useRouter, useSearchParams } from 'next/navigation';
import { invoke } from '@tauri-apps/api/core';
import { ArrowLeft } from 'lucide-react';
import { toast } from 'sonner';
import { MatchStatus, MatchSummary } from '@/types/matchmaking';
import { PIPELINE, STATUS_CHIP, STATUS_LABEL, TERMINAL, OUTCOME_LABEL } from '@/lib/workflow';

type Tab = 'active' | MatchStatus;

type SortKey = 'updated' | 'score';

function MatchesView() {
  const router = useRouter();
  const profileId = useSearchParams().get('profile');
  const [tab, setTab] = useState<Tab>('active');
  const [rows, setRows] = useState<MatchSummary[]>([]);
  const [counts, setCounts] = useState<Record<string, number>>({});
  const [showHidden, setShowHidden] = useState(false);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [query, setQuery] = useState('');
  const [sortKey, setSortKey] = useState<SortKey>('updated');

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const [list, c] = await Promise.all([
        invoke<MatchSummary[]>('mm_list_matches', {
          status: tab === 'active' ? null : tab,
          profileId,
          includeFinished: tab !== 'active',
          includeHidden: showHidden,
        }),
        invoke<Record<string, number>>('mm_match_counts'),
      ]);
      setRows(list);
      setCounts(c);
      setLoadError(null);
    } catch (e) {
      setRows([]);
      setLoadError(`${e}`);
      toast.error(`Could not load matches: ${e}`);
    } finally {
      setLoading(false);
    }
  }, [tab, showHidden, profileId]);

  useEffect(() => { void load(); }, [load]);

  useEffect(() => {
    if (tab !== 'active' && Object.keys(counts).length > 0 && (counts[tab] ?? 0) === 0) setTab('active');
  }, [counts, tab]);

  const shown = useMemo(() => {
    const needle = query.trim().toLowerCase();
    const filtered = needle
      ? rows.filter((m) => `${m.name_a ?? ''} ${m.name_b ?? ''}`.toLowerCase().includes(needle))
      : rows;
    return [...filtered].sort((x, y) =>
      sortKey === 'score' ? (y.score ?? -1) - (x.score ?? -1) : y.updated_at.localeCompare(x.updated_at)
    );
  }, [rows, query, sortKey]);

  const activeCount = PIPELINE.reduce((n, s) => n + (counts[s] ?? 0), 0);
  const tabs: { key: Tab; label: string; n: number }[] = [
    { key: 'active', label: 'In progress', n: activeCount },
    ...PIPELINE.filter((s) => (counts[s] ?? 0) > 0).map((s) => ({ key: s as Tab, label: STATUS_LABEL[s], n: counts[s] })),
    ...TERMINAL.filter((s) => (counts[s] ?? 0) > 0).map((s) => ({ key: s as Tab, label: STATUS_LABEL[s], n: counts[s] })),
  ];

  return (
    <div className="min-h-screen bg-gray-50">
      <div className="max-w-4xl mx-auto p-6 space-y-5">
        <div className="flex items-center gap-3">
          <button onClick={() => router.push('/')} className="p-2 rounded-lg hover:bg-gray-200" aria-label="Back">
            <ArrowLeft className="w-5 h-5" />
          </button>
          <div>
            <h1 className="text-2xl font-semibold">Matches</h1>
            <p className="text-sm text-gray-500">Start from a profile’s “Potential matches”, then follow each pair through to an outcome.</p>
          </div>
        </div>

        <div className="flex flex-wrap gap-2">
          {tabs.map((t) => (
            <button key={t.key} onClick={() => setTab(t.key)} className={`px-3 py-1.5 rounded-full text-sm border ${tab === t.key ? 'bg-blue-600 text-white border-blue-600' : 'bg-white hover:bg-gray-50'}`}>
              {t.label} <span className={tab === t.key ? 'text-blue-100' : 'text-gray-400'}>{t.n}</span>
            </button>
          ))}
        </div>
        {profileId && (
          <div className="flex items-center gap-2 text-sm bg-blue-50 border border-blue-200 rounded px-3 py-2">
            Showing matches for one profile.
            <button className="underline" onClick={() => router.push(`/profile?id=${encodeURIComponent(profileId)}`)}>Open profile</button>
            <button className="underline" onClick={() => router.push('/matches')}>Show all</button>
          </div>
        )}
        <div className="flex flex-wrap items-center gap-3">
          <input
            className="border rounded-md px-3 py-1.5 text-sm bg-white flex-1 min-w-[12rem]"
            placeholder="Find by name"
            aria-label="Find matches"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
          <label className="flex items-center gap-2 text-sm">
            Sort
            <select className="border rounded-md px-2 py-1.5 text-sm bg-white" value={sortKey} onChange={(e) => setSortKey(e.target.value as SortKey)}>
              <option value="updated">Recently updated</option>
              <option value="score">Highest score</option>
            </select>
          </label>
          <label className="flex items-center gap-2 text-sm">
            <input type="checkbox" checked={showHidden} onChange={(e) => setShowHidden(e.target.checked)} /> Include hidden matches
          </label>
          {(query || sortKey !== 'updated' || showHidden) && (
            <button className="text-sm underline" onClick={() => { setQuery(''); setSortKey('updated'); setShowHidden(false); }}>Reset</button>
          )}
        </div>

        <div className="bg-white rounded-lg border divide-y">
          {loading && <div className="p-6 text-center text-sm text-gray-500">Loading…</div>}
          {loadError && !loading && (
            <div className="p-6 text-center text-sm text-red-700">
              Could not load matches. <button className="underline" onClick={() => void load()}>Retry</button>
            </div>
          )}
          {shown.length === 0 && !loading && !loadError && (
            <div className="p-6 text-center text-sm text-gray-500">{query.trim() ? 'No matches for this name.' : 'No matches here yet.'}</div>
          )}
          {!loading && shown.map((m) => (
            <button key={m.id} onClick={() => router.push(`/match?id=${encodeURIComponent(m.id)}`)} className="w-full text-left px-4 py-3 hover:bg-gray-50 flex items-center gap-3">
              <div className="flex-1 min-w-0">
                <div className="font-medium truncate">{m.name_a || 'Unnamed'} ↔ {m.name_b || 'Unnamed'}</div>
                <div className="text-xs text-gray-500">Updated {new Date(m.updated_at).toLocaleDateString()}{m.outcome ? ` · ${OUTCOME_LABEL[m.outcome]}` : ''}</div>
              </div>
              {m.on_hold && <span className="text-xs px-2 py-0.5 rounded bg-amber-100 text-amber-800">waiting for info</span>}
              {!m.eligible && !TERMINAL.includes(m.status) && <span className="text-xs px-2 py-0.5 rounded bg-red-100 text-red-800">rules exclude</span>}
              {m.hidden && <span className="text-xs px-2 py-0.5 rounded bg-gray-200">hidden</span>}
              <span className={`text-xs px-2 py-0.5 rounded ${STATUS_CHIP[m.status]}`}>{STATUS_LABEL[m.status]}</span>
              <span className="w-16 text-right font-semibold">{m.score != null ? Math.round(m.score) : '–'}</span>
            </button>
          ))}
        </div>
      </div>
    </div>
  );
}

export default function MatchesPage() {
  return (
    <Suspense fallback={<div className="p-6 text-gray-500">Loading…</div>}>
      <MatchesView />
    </Suspense>
  );
}
