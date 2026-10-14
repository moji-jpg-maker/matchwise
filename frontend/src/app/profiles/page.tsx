'use client';

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

const PAGE_SIZE = 50;
type SortKey = 'name' | 'completeness' | 'updated';

export default function ProfilesPage() {
  const router = useRouter();
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
    const needle = query.trim().toLowerCase();
    const filtered = needle
      ? results.filter((p) => [p.name, p.city, p.age != null ? String(p.age) : null].some((v) => v?.toLowerCase().includes(needle)))
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
      toast.error(`Could not load profiles: ${e}`);
    }
  }, [includeInactive]);

  useEffect(() => {
    (async () => {
      try {
        setFields((await invoke<FieldDef[]>('mm_list_fields')).filter(isConditionField));
      } catch (e) {
        toast.error(`Could not load fields: ${e}`);
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
        toast.error(`Unknown field '${r.field}'. Remove that condition.`);
        return;
      }
      const c = toCondition(r, def);
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
      });
      setResults(res);
      setIsFiltered(true);
    } catch (e) {
      toast.error(`Search failed: ${e}`);
    } finally {
      setSearching(false);
    }
  };

  const createProfile = async () => {
    try {
      const p = await invoke<ProfileView>('mm_create_profile', { values: {} });
      router.push(`/profile?id=${encodeURIComponent(p.id)}`);
    } catch (e) {
      toast.error(`Could not create profile: ${e}`);
    }
  };

  return (
    <div className="min-h-screen bg-gray-50">
      <div className="max-w-5xl mx-auto p-6 space-y-5">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-3">
            <button onClick={() => router.push('/')} className="p-2 rounded-lg hover:bg-gray-200" aria-label="Back">
              <ArrowLeft className="w-5 h-5" />
            </button>
            <h1 className="text-2xl font-semibold">Profiles</h1>
          </div>
          <div className="flex items-center gap-2">
            <Button variant="outline" onClick={() => router.push('/fields')}>Fields</Button>
            <Button onClick={createProfile}>
              <Plus className="w-4 h-4 mr-1" /> New profile
            </Button>
          </div>
        </div>

        <div className="bg-white rounded-lg border p-4 space-y-3">
          <div className="flex items-center justify-between">
            <h2 className="font-medium">Search</h2>
            <Button variant="outline" size="sm" onClick={addRow} disabled={fields.length === 0}>
              <Plus className="w-4 h-4 mr-1" /> Add condition
            </Button>
          </div>

          {rows.length === 0 && <p className="text-sm text-gray-500">No conditions: showing every profile.</p>}

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
                    <option key={f.key} value={f.key}>{f.label}</option>
                  ))}
                </select>
                <select
                  className="border rounded-md px-2 py-1.5 text-sm bg-white"
                  value={r.op}
                  onChange={(e) => update({ op: e.target.value as ConditionOp })}
                >
                  {ops.map((o) => (
                    <option key={o.op} value={o.op}>{o.label}</option>
                  ))}
                </select>

                {r.op !== 'exists' && kind === 'number' && (
                  <>
                    <Input className="w-24" type="number" value={r.a} onChange={(e) => update({ a: e.target.value })} />
                    {r.op === 'between' && (
                      <>
                        <span className="text-sm text-gray-500">and</span>
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
                    <option value="true">yes</option>
                    <option value="false">no</option>
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
                    <option value="">choose…</option>
                    {opts.map((o) => (
                      <option key={o} value={o}>{o}</option>
                    ))}
                  </select>
                )}
                <button onClick={() => setRows((rs) => rs.filter((_, j) => j !== i))} className="p-1 rounded hover:bg-gray-100" aria-label="Remove condition">
                  <X className="w-4 h-4" />
                </button>
              </div>
            );
          })}

          <div className="flex flex-wrap items-center gap-4 pt-1">
            <label className="flex items-center gap-2 text-sm">
              <input type="checkbox" checked={includeUnknown} onChange={(e) => { setIncludeUnknown(e.target.checked); if (isFiltered) void runSearch({ unknown: e.target.checked }); }} />
              Also show profiles with missing information
            </label>
            <label className="flex items-center gap-2 text-sm">
              <input type="checkbox" checked={includeInactive} onChange={(e) => { setIncludeInactive(e.target.checked); if (isFiltered) void runSearch({ inactive: e.target.checked }); }} />
              Include deactivated
            </label>
            <div className="flex-1" />
            {(isFiltered || rows.length > 0) && (
              <Button variant="ghost" size="sm" onClick={() => { setRows([]); setIncludeUnknown(false); setIsFiltered(false); }}>
                Clear
              </Button>
            )}
            <Button size="sm" onClick={() => runSearch()} disabled={searching || loading}>
              <Search className="w-4 h-4 mr-1" /> Search
            </Button>
          </div>
        </div>

        <div className="flex flex-wrap items-center gap-3">
          <div className="relative flex-1 min-w-[12rem]">
            <Search className="w-4 h-4 absolute left-3 top-1/2 -translate-y-1/2 text-gray-400" />
            <Input className="pl-9" placeholder="Find by name, city or age" aria-label="Find profiles" value={query} onChange={(e) => setQuery(e.target.value)} />
          </div>
          <label className="flex items-center gap-2 text-sm">
            Sort
            <select className="border rounded-md px-2 py-1.5 text-sm bg-white" value={sortKey} onChange={(e) => setSortKey(e.target.value as SortKey)}>
              <option value="name">Name</option>
              <option value="completeness">Most complete</option>
              <option value="updated">Recently updated</option>
            </select>
          </label>
        </div>

        <div className="bg-white rounded-lg border divide-y">
          <div className="px-4 py-2 text-sm text-gray-500">
            {shown.length} {shown.length === 1 ? 'profile' : 'profiles'}
            {isFiltered ? ' match' : ''}
            {query.trim() ? ` for “${query.trim()}”` : ''}
          </div>
          {loading && <div className="p-6 text-center text-gray-500 text-sm">Loading…</div>}
          {loadError && !loading && (
            <div className="p-6 text-center text-sm text-red-700">
              Could not load profiles. <button className="underline" onClick={() => void loadAll()}>Retry</button>
            </div>
          )}
          {shown.length === 0 && !loading && !loadError && (
            <div className="p-6 text-center text-gray-500 text-sm">
              {query.trim()
                ? 'No profiles match this text.'
                : isFiltered
                  ? 'No profiles match these conditions.'
                  : 'No profiles yet. Create the first one.'}
            </div>
          )}
          {shown.slice(0, visible).map((p) => (
            <button
              key={p.id}
              onClick={() => router.push(`/profile?id=${encodeURIComponent(p.id)}`)}
              className="w-full text-left px-4 py-3 hover:bg-gray-50 flex items-center gap-4"
            >
              <div className="flex-1 min-w-0">
                <div className="font-medium truncate">{p.name || 'Unnamed profile'}</div>
                <div className="text-sm text-gray-500">
                  {[p.age != null ? `${p.age}` : null, p.city].filter(Boolean).join(' · ') || 'No basic details yet'}
                </div>
              </div>
              {p.match_result === 'unknown' && (
                <span className="text-xs px-2 py-0.5 rounded bg-amber-100 text-amber-800">missing info</span>
              )}
              {p.status === 'deactivated' && (
                <span className="text-xs px-2 py-0.5 rounded bg-gray-200 text-gray-700">deactivated</span>
              )}
              <div className="w-24 text-right text-sm text-gray-600">
                {p.completeness != null ? `${Math.round(p.completeness * 100)}% complete` : ''}
              </div>
            </button>
          ))}
          {shown.length > visible && (
            <div className="p-3 text-center">
              <Button variant="ghost" size="sm" onClick={() => setVisible((v) => v + PAGE_SIZE)}>
                Show more ({shown.length - visible} left)
              </Button>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
