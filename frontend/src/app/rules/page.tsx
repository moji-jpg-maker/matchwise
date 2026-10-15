'use client';

import React, { useCallback, useEffect, useState } from 'react';
import { useRouter } from 'next/navigation';
import { invoke } from '@tauri-apps/api/core';
import { ArrowLeft, Plus } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { normalizeSearch } from '@/i18n/format';
import { useI18n } from '@/i18n';
import { RuleSetSummary } from '@/types/matchmaking';

export default function RuleSetsPage() {
  const { t, fmt } = useI18n();
  const router = useRouter();
  const [sets, setSets] = useState<RuleSetSummary[]>([]);
  const [showArchived, setShowArchived] = useState(false);
  const [query, setQuery] = useState('');
  const [loading, setLoading] = useState(true);

  const load = useCallback(async () => {
    try {
      setSets(await invoke<RuleSetSummary[]>('mm_list_rule_sets', { includeArchived: showArchived }));
    } catch (e) {
      toast.error(t('rules.loadError', { error: `${e}` }));
    } finally {
      setLoading(false);
    }
  }, [showArchived, t]);

  useEffect(() => { void load(); }, [load]);

  const toggleArchive = async (s: RuleSetSummary) => {
    try {
      await invoke('mm_archive_rule_set', { id: s.id, archived: !s.archived });
      await load();
    } catch (e) {
      toast.error(`${e}`);
    }
  };

  const needle = normalizeSearch(query.trim());
  const visible = needle
    ? sets.filter((s) => normalizeSearch(s.name).includes(needle) || normalizeSearch(s.description ?? '').includes(needle))
    : sets;

  return (
    <div className="min-h-screen bg-gray-50">
      <div className="max-w-4xl mx-auto p-6 space-y-5">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-3">
            <button onClick={() => router.push('/')} className="p-2 rounded-lg hover:bg-gray-200" aria-label={t('rules.back')}>
              <ArrowLeft className="w-5 h-5 rtl-mirror" />
            </button>
            <div>
              <h1 className="text-2xl font-semibold">{t('rules.title')}</h1>
              <p className="text-sm text-gray-500">{t('rules.intro')}</p>
            </div>
          </div>
          <Button onClick={() => router.push('/rule-set?id=new')}>
            <Plus className="w-4 h-4 me-1" /> {t('rules.newSet')}
          </Button>
        </div>

        <div className="flex items-center gap-4">
          <Input className="max-w-xs" dir="auto" placeholder={t('rules.search')} aria-label={t('rules.search')} value={query} onChange={(e) => setQuery(e.target.value)} />
          <label className="flex items-center gap-2 text-sm">
            <input type="checkbox" checked={showArchived} onChange={(e) => setShowArchived(e.target.checked)} />
            {t('rules.showArchived')}
          </label>
          {query && (
            <Button variant="ghost" size="sm" onClick={() => setQuery('')}>{t('rules.clear')}</Button>
          )}
        </div>

        <div className="bg-white rounded-lg border divide-y">
          {visible.length === 0 && !loading && (
            <div className="p-6 text-center text-sm text-gray-500">
              {sets.length === 0 ? t('rules.emptyNone') : t('rules.emptySearch')}
            </div>
          )}
          {visible.map((s) => (
            <div key={s.id} className="p-4 flex items-center gap-4">
              <button className="flex-1 min-w-0 text-start" onClick={() => router.push(`/rule-set?id=${encodeURIComponent(s.id)}`)}>
                <div className="font-medium">
                  <bdi>{s.name}</bdi> {s.archived && <span className="ms-2 text-xs px-2 py-0.5 rounded bg-gray-200">{t('rules.archived')}</span>}
                </div>
                <div className="text-sm text-gray-500 truncate"><bdi>{s.description || t('rules.noDescription')}</bdi></div>
              </button>
              <div className="text-sm text-gray-600 w-40 text-end">
                {t(s.rule_count === 1 ? 'rules.versionRulesOne' : 'rules.versionRulesMany', { version: fmt.number(s.current_version), count: fmt.number(s.rule_count) })}
              </div>
              <Button variant="ghost" size="sm" onClick={() => toggleArchive(s)}>
                {s.archived ? t('rules.restore') : t('rules.archive')}
              </Button>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
