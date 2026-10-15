'use client';

import React, { Suspense, useCallback, useEffect, useMemo, useState } from 'react';
import { useRouter, useSearchParams } from 'next/navigation';
import { invoke } from '@tauri-apps/api/core';
import { ArrowLeft } from 'lucide-react';
import { toast } from 'sonner';
import { MatchStatus, MatchSummary } from '@/types/matchmaking';
import { PIPELINE, STATUS_CHIP, STATUS_KEY, TERMINAL, OUTCOME_KEY } from '@/lib/workflow';
import { useI18n } from '@/i18n';
import { normalizeSearch } from '@/i18n/format';

type Tab = 'active' | MatchStatus;

type SortKey = 'updated' | 'score';

function MatchesView() {
  const router = useRouter();
  const { t, fmt } = useI18n();
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
      toast.error(t('matches.loadToast', { error: `${e}` }));
    } finally {
      setLoading(false);
    }
  }, [tab, showHidden, profileId, t]);

  useEffect(() => { void load(); }, [load]);

  useEffect(() => {
    if (tab !== 'active' && Object.keys(counts).length > 0 && (counts[tab] ?? 0) === 0) setTab('active');
  }, [counts, tab]);

  const shown = useMemo(() => {
    const needle = normalizeSearch(query.trim());
    const filtered = needle
      ? rows.filter((m) => normalizeSearch(`${m.name_a ?? ''} ${m.name_b ?? ''}`).includes(needle))
      : rows;
    return [...filtered].sort((x, y) =>
      sortKey === 'score' ? (y.score ?? -1) - (x.score ?? -1) : y.updated_at.localeCompare(x.updated_at)
    );
  }, [rows, query, sortKey]);

  const activeCount = PIPELINE.reduce((n, s) => n + (counts[s] ?? 0), 0);
  const tabs: { key: Tab; label: string; n: number }[] = [
    { key: 'active', label: t('matches.inProgress'), n: activeCount },
    ...PIPELINE.filter((s) => (counts[s] ?? 0) > 0).map((s) => ({ key: s as Tab, label: t(STATUS_KEY[s]), n: counts[s] })),
    ...TERMINAL.filter((s) => (counts[s] ?? 0) > 0).map((s) => ({ key: s as Tab, label: t(STATUS_KEY[s]), n: counts[s] })),
  ];

  return (
    <div className="min-h-screen bg-gray-50">
      <div className="max-w-4xl mx-auto p-6 space-y-5">
        <div className="flex items-center gap-3">
          <button onClick={() => router.push('/')} className="p-2 rounded-lg hover:bg-gray-200" aria-label={t('profiles.back')}>
            <ArrowLeft className="w-5 h-5 rtl-mirror" />
          </button>
          <div>
            <h1 className="text-2xl font-semibold">{t('matches.title')}</h1>
            <p className="text-sm text-gray-500">{t('matches.intro')}</p>
          </div>
        </div>

        <div className="flex flex-wrap gap-2">
          {tabs.map((entry) => (
            <button key={entry.key} onClick={() => setTab(entry.key)} className={`px-3 py-1.5 rounded-full text-sm border ${tab === entry.key ? 'bg-blue-600 text-white border-blue-600' : 'bg-white hover:bg-gray-50'}`}>
              {entry.label} <span className={tab === entry.key ? 'text-blue-100' : 'text-gray-400'}>{fmt.number(entry.n)}</span>
            </button>
          ))}
        </div>
        {profileId && (
          <div className="flex items-center gap-2 text-sm bg-blue-50 border border-blue-200 rounded px-3 py-2">
            {t('matches.forOneProfile')}
            <button className="underline" onClick={() => router.push(`/profile?id=${encodeURIComponent(profileId)}`)}>{t('matches.openProfile')}</button>
            <button className="underline" onClick={() => router.push('/matches')}>{t('matches.showAll')}</button>
          </div>
        )}
        <div className="flex flex-wrap items-center gap-3">
          <input
            className="border rounded-md px-3 py-1.5 text-sm bg-white flex-1 min-w-[12rem]"
            placeholder={t('matches.findPlaceholder')}
            aria-label={t('matches.findAria')}
            dir="auto"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
          <label className="flex items-center gap-2 text-sm">
            {t('matches.sort')}
            <select className="border rounded-md px-2 py-1.5 text-sm bg-white" value={sortKey} onChange={(e) => setSortKey(e.target.value as SortKey)}>
              <option value="updated">{t('matches.sortUpdated')}</option>
              <option value="score">{t('matches.sortScore')}</option>
            </select>
          </label>
          <label className="flex items-center gap-2 text-sm">
            <input type="checkbox" checked={showHidden} onChange={(e) => setShowHidden(e.target.checked)} /> {t('matches.includeHidden')}
          </label>
          {(query || sortKey !== 'updated' || showHidden) && (
            <button className="text-sm underline" onClick={() => { setQuery(''); setSortKey('updated'); setShowHidden(false); }}>{t('matches.reset')}</button>
          )}
        </div>

        <div className="bg-white rounded-lg border divide-y">
          {loading && <div className="p-6 text-center text-sm text-gray-500">{t('common.loading')}</div>}
          {loadError && !loading && (
            <div className="p-6 text-center text-sm text-red-700">
              {t('matches.loadInline')} <button className="underline" onClick={() => void load()}>{t('matches.retry')}</button>
            </div>
          )}
          {shown.length === 0 && !loading && !loadError && (
            <div className="p-6 text-center text-sm text-gray-500">{query.trim() ? t('matches.emptyQuery') : t('matches.empty')}</div>
          )}
          {!loading && shown.map((m) => (
            <button key={m.id} onClick={() => router.push(`/match?id=${encodeURIComponent(m.id)}`)} className="w-full text-start px-4 py-3 hover:bg-gray-50 flex items-center gap-3">
              <div className="flex-1 min-w-0">
                <div className="font-medium truncate"><bdi>{t('matches.pair', { nameA: m.name_a || t('matches.unnamed'), nameB: m.name_b || t('matches.unnamed') })}</bdi></div>
                <div className="text-xs text-gray-500">{t('matches.updated', { date: fmt.date(m.updated_at) })}{m.outcome ? ` · ${t(OUTCOME_KEY[m.outcome])}` : ''}</div>
              </div>
              {m.on_hold && <span className="text-xs px-2 py-0.5 rounded bg-amber-100 text-amber-800">{t('matches.onHold')}</span>}
              {!m.eligible && !TERMINAL.includes(m.status) && <span className="text-xs px-2 py-0.5 rounded bg-red-100 text-red-800">{t('matches.rulesExclude')}</span>}
              {m.hidden && <span className="text-xs px-2 py-0.5 rounded bg-gray-200">{t('matches.hiddenChip')}</span>}
              <span className={`text-xs px-2 py-0.5 rounded ${STATUS_CHIP[m.status]}`}>{t(STATUS_KEY[m.status])}</span>
              <span className="w-16 text-end font-semibold">{m.score != null ? fmt.number(Math.round(m.score)) : t('matches.noScore')}</span>
            </button>
          ))}
        </div>
      </div>
    </div>
  );
}

export default function MatchesPage() {
  const { t } = useI18n();
  const fallbackLoading = t('common.loading');
  return (
    <Suspense fallback={<div className="p-6 text-gray-500">{fallbackLoading}</div>}>
      <MatchesView />
    </Suspense>
  );
}
