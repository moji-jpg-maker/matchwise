'use client';

import { fieldLabel } from '@/lib/field-label';
import React, { useCallback, useEffect, useMemo, useState } from 'react';
import { useRouter } from 'next/navigation';
import { invoke } from '@tauri-apps/api/core';
import { ArrowLeft, Plus, Search, X } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import {
  Condition,
  ConditionOp,
  FieldDef,
  ProfileSummary,
  ProfileView,
  SearchResultRow,
  kindName,
  kindOptions,
} from '@/types/matchmaking';
import { OPS_BY_KIND, Row, defaultOp, isConditionField, toCondition } from '@/lib/conditions';
import { useI18n } from '@/i18n';
import { normalizeSearch } from '@/i18n/format';

const PAGE_SIZE = 50;
type SortKey = 'name' | 'completeness' | 'updated';

export default function ProfilesPage() {
  const router = useRouter();
  const { t, fmt, locale } = useI18n();
  const [fields, setFields] = useState<FieldDef[]>([]);
  const [rows, setRows] = useState<Row[]>([]);
  const [includeUnknown, setIncludeUnknown] = useState(false);
  const [includeInactive, setIncludeInactive] = useState(false);
  const [results, setResults] = useState<(ProfileSummary & { match_result?: string })[]>([]);
  const [searching, setSearching] = useState(false);
  const [isFiltered, setIsFiltered] = useState(false);
  const [loading, setLoading] = useState(true);
  const [query, setQuery] = useState('');
  const [sortKey, setSortKey] = useState<SortKey>('name');
  const [visible, setVisible] = useState(PAGE_SIZE);
  const [loadError, setLoadError] = useState<string | null>(null);

  const fieldByKey = useMemo(() => Object.fromEntries(fields.map((f) => [f.key, f])), [fields]);

  const shown = useMemo(() => {
    const needle = normalizeSearch(query.trim());
    const filtered = needle
      ? results.filter((p) => [p.name, p.city, p.age != null ? String(p.age) : null].some((v) => v != null && normalizeSearch(v).includes(needle)))
      : results;
    return [...filtered].sort((x, y) => {
      if (sortKey === 'completeness') return (y.completeness ?? -1) - (x.completeness ?? -1);
      if (sortKey === 'updated') return y.updated_at.localeCompare(x.updated_at);
      return (x.name || '~').localeCompare(y.name || '~');
    });
  }, [results, query, sortKey]);

  const loadAll = useCallback(async () => {
    try {
      const list = await invoke<ProfileSummary[]>('mm_list_profiles', { includeDeactivated: includeInactive });
      setResults(list);
      setIsFiltered(false);
      setLoadError(null);
    } catch (e) {
      setLoadError(`${e}`);
      toast.error(t('profiles.loadFailed', { error: `${e}` }));
    }
  }, [includeInactive]);

  useEffect(() => {
    (async () => {
      try {
        setFields((await invoke<FieldDef[]>('mm_list_fields')).filter(isConditionField));
      } catch (e) {
        toast.error(t('profiles.fieldsLoadFailed', { error: `${e}` }));
      } finally {
        setLoading(false);
      }
    })();
  }, []);

  useEffect(() => {
    if (!isFiltered) void loadAll();
  }, [loadAll, isFiltered]);

  const addRow = () => {
    const first = fields[0];
    if (!first) return;
    setRows((r) => [...r, { field: first.key, op: defaultOp(first), a: '', b: '', multi: [] }]);
  };

  useEffect(() => { setVisible(PAGE_SIZE); }, [query, sortKey, results]);

  const runSearch = async (overrides?: { unknown?: boolean; inactive?: boolean }) => {
    const conditions: Condition[] = [];
    for (const r of rows) {
      const def = fieldByKey[r.field];
      if (!def) {
        toast.error(t('profiles.unknownField', { field: r.field }));
        return;
      }
      const c = toCondition(r, def, t, locale);
      if (typeof c === 'string') {
        toast.error(c);
        return;
      }
      conditions.push(c);
    }
    setSearching(true);
    try {
      const res = await invoke<SearchResultRow[]>('mm_search_profiles', {
        conditions,
        includeUnknown: overrides?.unknown ?? includeUnknown,
        includeDeactivated: overrides?.inactive ?? includeInactive,
        locale,
      });
      setResults(res);
      setIsFiltered(true);
    } catch (e) {
      toast.error(t('profiles.searchFailed', { error: `${e}` }));
    } finally {
      setSearching(false);
    }
  };

  const createProfile = async () => {
    try {
      const p = await invoke<ProfileView>('mm_create_profile', { values: {}, locale });
      router.push(`/profile?id=${encodeURIComponent(p.id)}`);
    } catch (e) {
      toast.error(t('profiles.createFailed', { error: `${e}` }));
    }
  };

  return (
    <div className="min-h-screen bg-gray-50">
      <div className="max-w-5xl mx-auto p-6 space-y-5">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-3">
            <button onClick={() => router.push('/')} className="p-2 rounded-lg hover:bg-gray-200" aria-label={t('profiles.back')}>
              <ArrowLeft className="w-5 h-5 rtl-mirror" />
            </button>
            <h1 className="text-2xl font-semibold">{t('profiles.title')}</h1>
          </div>
          <div className="flex items-center gap-2">
            <Button variant="outline" onClick={() => router.push('/fields')}>{t('profiles.fields')}</Button>
            <Button onClick={createProfile}>
              <Plus className="w-4 h-4 me-1" /> {t('profiles.newProfile')}
            </Button>
          </div>
        </div>

        <div className="bg-white rounded-lg border p-4 space-y-3">
          <div className="flex items-center justify-between">
            <h2 className="font-medium">{t('profiles.search')}</h2>
            <Button variant="outline" size="sm" onClick={addRow} disabled={fields.length === 0}>
              <Plus className="w-4 h-4 me-1" /> {t('profiles.addCondition')}
            </Button>
          </div>

          {rows.length === 0 && <p className="text-sm text-gray-500">{t('profiles.noConditions')}</p>}

          {rows.map((r, i) => {
            const def = fieldByKey[r.field];
            const kind = def ? kindName(def.kind) : 'text';
            const ops = OPS_BY_KIND[kind];
            const opts = def ? kindOptions(def.kind) : [];
            const update = (patch: Partial<Row>) => setRows((rs) => rs.map((x, j) => (j === i ? { ...x, ...patch } : x)));
            return (
              <div key={i} className="flex flex-wrap items-center gap-2">
                <select
                  className="border rounded-md px-2 py-1.5 text-sm bg-white"
                  value={r.field}
                  onChange={(e) => {
                    const nd = fieldByKey[e.target.value];
                    update({ field: e.target.value, op: OPS_BY_KIND[kindName(nd.kind)][0].op, a: '', b: '', multi: [] });
                  }}
                >
                  {fields.map((f) => (
                    <option key={f.key} value={f.key}>{fieldLabel(f, locale)}</option>
                  ))}
                </select>
                <select
                  className="border rounded-md px-2 py-1.5 text-sm bg-white"
                  value={r.op}
                  onChange={(e) => update({ op: e.target.value as ConditionOp })}
                >
                  {ops.map((o) => (
                    <option key={o.op} value={o.op}>{t(o.labelKey)}</option>
                  ))}
                </select>

                {r.op !== 'exists' && kind === 'number' && (
                  <>
                    <Input className="w-24" type="number" value={r.a} onChange={(e) => update({ a: e.target.value })} />
                    {r.op === 'between' && (
                      <>
                        <span className="text-sm text-gray-500">{t('profiles.and')}</span>
                        <Input className="w-24" type="number" value={r.b} onChange={(e) => update({ b: e.target.value })} />
                      </>
                    )}
                  </>
                )}
                {r.op !== 'exists' && kind === 'text' && (
                  <Input className="w-56" value={r.a} onChange={(e) => update({ a: e.target.value })} />
                )}
                {r.op !== 'exists' && kind === 'bool' && (
                  <select className="border rounded-md px-2 py-1.5 text-sm bg-white" value={r.a || 'true'} onChange={(e) => update({ a: e.target.value })}>
                    <option value="true">{t('profiles.yes')}</option>
                    <option value="false">{t('profiles.no')}</option>
                  </select>
                )}
                {r.op === 'in' && (kind === 'choice' || kind === 'multi_choice') && (
                  <div className="flex flex-wrap gap-2">
                    {opts.map((o) => (
                      <label key={o} className="flex items-center gap-1 text-sm">
                        <input
                          type="checkbox"
                          checked={r.multi.includes(o)}
                          onChange={(e) => update({ multi: e.target.checked ? [...r.multi, o] : r.multi.filter((x) => x !== o) })}
                        />
                        {o}
                      </label>
                    ))}
                  </div>
                )}
                {r.op === 'ne' && kind === 'choice' && (
                  <select className="border rounded-md px-2 py-1.5 text-sm bg-white" value={r.a} onChange={(e) => update({ a: e.target.value })}>
                    <option value="">{t('profiles.choose')}</option>
                    {opts.map((o) => (
                      <option key={o} value={o}>{o}</option>
                    ))}
                  </select>
                )}
                <button onClick={() => setRows((rs) => rs.filter((_, j) => j !== i))} className="p-1 rounded hover:bg-gray-100" aria-label={t('profiles.removeCondition')}>
                  <X className="w-4 h-4" />
                </button>
              </div>
            );
          })}

          <div className="flex flex-wrap items-center gap-4 pt-1">
            <label className="flex items-center gap-2 text-sm">
              <input type="checkbox" checked={includeUnknown} onChange={(e) => { setIncludeUnknown(e.target.checked); if (isFiltered) void runSearch({ unknown: e.target.checked }); }} />
              {t('profiles.includeUnknown')}
            </label>
            <label className="flex items-center gap-2 text-sm">
              <input type="checkbox" checked={includeInactive} onChange={(e) => { setIncludeInactive(e.target.checked); if (isFiltered) void runSearch({ inactive: e.target.checked }); }} />
              {t('profiles.includeDeactivated')}
            </label>
            <div className="flex-1" />
            {(isFiltered || rows.length > 0) && (
              <Button variant="ghost" size="sm" onClick={() => { setRows([]); setIncludeUnknown(false); setIsFiltered(false); }}>
                {t('profiles.clear')}
              </Button>
            )}
            <Button size="sm" onClick={() => runSearch()} disabled={searching || loading}>
              <Search className="w-4 h-4 me-1" /> {t('profiles.search')}
            </Button>
          </div>
        </div>

        <div className="flex flex-wrap items-center gap-3">
          <div className="relative flex-1 min-w-[12rem]">
            <Search className="w-4 h-4 absolute start-3 top-1/2 -translate-y-1/2 text-gray-400" />
            <Input className="ps-9" placeholder={t('profiles.findPlaceholder')} aria-label={t('profiles.findAria')} value={query} onChange={(e) => setQuery(e.target.value)} />
          </div>
          <label className="flex items-center gap-2 text-sm">
            {t('profiles.sort')}
            <select className="border rounded-md px-2 py-1.5 text-sm bg-white" value={sortKey} onChange={(e) => setSortKey(e.target.value as SortKey)}>
              <option value="name">{t('profiles.sortName')}</option>
              <option value="completeness">{t('profiles.sortComplete')}</option>
              <option value="updated">{t('profiles.sortUpdated')}</option>
            </select>
          </label>
        </div>

        <div className="bg-white rounded-lg border divide-y">
          <div className="px-4 py-2 text-sm text-gray-500">
            {t(isFiltered ? (shown.length === 1 ? 'profiles.countOneMatch' : 'profiles.countManyMatch') : (shown.length === 1 ? 'profiles.countOne' : 'profiles.countMany'), { count: fmt.number(shown.length) })}
            {query.trim() ? t('profiles.forQuery', { query: query.trim() }) : ''}
          </div>
          {loading && <div className="p-6 text-center text-gray-500 text-sm">{t('profiles.loading')}</div>}
          {loadError && !loading && (
            <div className="p-6 text-center text-sm text-red-700">
              {t('profiles.loadErrorInline')} <button className="underline" onClick={() => void loadAll()}>{t('profiles.retry')}</button>
            </div>
          )}
          {shown.length === 0 && !loading && !loadError && (
            <div className="p-6 text-center text-gray-500 text-sm">
              {query.trim()
                ? t('profiles.emptyQuery')
                : isFiltered
                  ? t('profiles.emptyFiltered')
                  : t('profiles.emptyNone')}
            </div>
          )}
          {shown.slice(0, visible).map((p) => (
            <button
              key={p.id}
              onClick={() => router.push(`/profile?id=${encodeURIComponent(p.id)}`)}
              className="w-full text-start px-4 py-3 hover:bg-gray-50 flex items-center gap-4"
            >
              <div className="flex-1 min-w-0">
                <div className="font-medium truncate"><bdi>{p.name || t('profiles.unnamed')}</bdi></div>
                <div className="text-sm text-gray-500">
                  {[p.age != null ? fmt.number(p.age) : null, p.city].filter(Boolean).join(' · ') || t('profiles.noDetails')}
                </div>
              </div>
              {p.match_result === 'unknown' && (
                <span className="text-xs px-2 py-0.5 rounded bg-amber-100 text-amber-800">{t('profiles.missingInfo')}</span>
              )}
              {p.status === 'deactivated' && (
                <span className="text-xs px-2 py-0.5 rounded bg-gray-200 text-gray-700">{t('profiles.deactivated')}</span>
              )}
              <div className="w-24 text-end text-sm text-gray-600">
                {p.completeness != null ? t('profiles.complete', { percent: fmt.percent(p.completeness) }) : ''}
              </div>
            </button>
          ))}
          {shown.length > visible && (
            <div className="p-3 text-center">
              <Button variant="ghost" size="sm" onClick={() => setVisible((v) => v + PAGE_SIZE)}>
                {t('profiles.showMore', { count: fmt.number(shown.length - visible) })}
              </Button>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
