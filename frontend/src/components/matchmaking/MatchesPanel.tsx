'use client';

import React, { useCallback, useEffect, useState } from 'react';
import { useRouter } from 'next/navigation';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { MatchCandidate, RuleSetSummary } from '@/types/matchmaking';
import { normalizeSearch } from '@/i18n/format';
import { useI18n } from '@/i18n';

export function MatchesPanel({ profileId }: { profileId: string }) {
  const { t, fmt } = useI18n();
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
          <h2 className="font-medium">{t('matchesPanel.title')}</h2>
          <p className="text-xs text-gray-500">{t('matchesPanel.intro')}</p>
        </div>
        <select className="border rounded-md px-2 py-1.5 text-sm bg-white" value={setId} onChange={(e) => { setSetId(e.target.value); setRows(null); setFindError(null); }}>
          {sets.map((s) => <option key={s.id} value={s.id}>{s.name}</option>)}
        </select>
        <label className="flex items-center gap-1 text-sm">
          <input type="checkbox" checked={showExcluded} onChange={(e) => { setShowExcluded(e.target.checked); setRows(null); setFindError(null); }} /> {t('matchesPanel.showExcluded')}
        </label>
        <Button size="sm" onClick={() => run()} disabled={!setId || busy}>{busy ? t('matchesPanel.working') : t('matchesPanel.find')}</Button>
      </div>

      {setsError && (
        <div className="p-4 flex items-center gap-3 text-sm text-red-700 bg-red-50 border-b border-red-200">
          <span className="flex-1">{t('matchesPanel.loadSetsError', { error: setsError })}</span>
          <Button size="sm" variant="outline" onClick={() => void loadSets()}>{t('profiles.retry')}</Button>
        </div>
      )}
      {findError && (
        <div className="p-4 flex items-center gap-3 text-sm text-red-700 bg-red-50 border-b border-red-200">
          <span className="flex-1">{t('matchesPanel.findError', { error: findError })}</span>
          <Button size="sm" variant="outline" onClick={() => run(limit)} disabled={busy}>{t('profiles.retry')}</Button>
        </div>
      )}
      {setsLoaded && !setsError && sets.length === 0 && (
        <p className="p-4 text-sm text-gray-500">{t('matchesPanel.noSetsPre')} <button className="underline" onClick={() => router.push('/rules')}>{t('matchesPanel.createOne')}</button> {t('matchesPanel.noSetsPost')}</p>
      )}
      {rows && rows.length > 0 && (
        <div className="p-3 border-b">
          <input className="border rounded-md px-3 py-1.5 text-sm w-full" dir="auto" placeholder={t('matchesPanel.filterPlaceholder')} aria-label={t('matchesPanel.filterAria')} value={filter} onChange={(e) => setFilter(e.target.value)} />
        </div>
      )}
      {rows && rows.length === 0 && <p className="p-4 text-sm text-gray-500">{showExcluded ? t('matchesPanel.noCandidatesAll') : t('matchesPanel.noCandidatesPass')}</p>}
      <div className="divide-y">
        {rows?.filter((m) => !filter.trim() || normalizeSearch(`${m.name ?? ''} ${m.city ?? ''}`).includes(normalizeSearch(filter.trim()))).map((m) => (
          <button key={m.id} onClick={() => router.push(`/match?a=${encodeURIComponent(profileId)}&b=${encodeURIComponent(m.id)}&set=${encodeURIComponent(setId)}`)} className="w-full text-start p-4 hover:bg-gray-50" title={t('matchesPanel.openTitle')}>
            <div className="flex items-center gap-3">
              <div className="flex-1 min-w-0">
                <div className="font-medium truncate"><bdi>{m.name || t('matchesPanel.unnamed')}</bdi></div>
                <div className="text-sm text-gray-500">{[m.age != null ? fmt.number(m.age) : null, m.city].filter(Boolean).join(' · ')}</div>
              </div>
              {!m.eligible && <span className="text-xs px-2 py-0.5 rounded bg-red-100 text-red-800">{t('matchesPanel.excluded')}</span>}
              {m.needs_info && <span className="text-xs px-2 py-0.5 rounded bg-amber-100 text-amber-800">{t('matchesPanel.missingInfo')}</span>}
              <div className="text-end w-44">
                <div className="font-semibold">{m.score != null ? t('matchesPanel.score', { score: fmt.number(Math.round(m.score)) }) : t('scoreCard.na')}</div>
                <div className="text-xs text-gray-500">{t('matchesPanel.confidence', { percent: fmt.percent(m.confidence ?? 0) })}</div>
              </div>
            </div>
            {(m.strengths.length > 0 || m.concerns.length > 0) && (
              <div className="mt-1 text-xs text-gray-600 space-y-0.5">
                {m.strengths.map((text, k) => <div key={`s${k}`} dir="auto">✓ {text}</div>)}
                {m.concerns.map((text, k) => <div key={`c${k}`} dir="auto" className="text-amber-700">⚠ {text}</div>)}
              </div>
            )}
            {m.blocking.length > 0 && (
              <ul className="mt-2 text-sm text-red-700 list-disc ps-5">
                {m.blocking.map((b, k) => <li key={k} dir="auto">{b}</li>)}
              </ul>
            )}
          </button>
        ))}
      </div>
      {rows && total !== null && rows.length < total && limit < 500 && (
        <div className="p-3 text-center border-t">
          <p className="text-xs text-gray-500 mb-1">{t('matchesPanel.showing', { count: fmt.number(rows.length), total: fmt.number(total) })}</p>
          <Button size="sm" variant="ghost" disabled={busy} onClick={() => run(Math.min(limit + 50, 500))}>{t('matchesPanel.loadMore')}</Button>
        </div>
      )}
      {rows && total !== null && rows.length < total && limit >= 500 && (
        <p className="p-3 text-center border-t text-xs text-gray-500">{t('matchesPanel.showingMax', { count: fmt.number(rows.length), total: fmt.number(total), max: fmt.number(500) })}</p>
      )}
    </div>
  );
}
