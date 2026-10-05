'use client';

import React, { useCallback, useEffect, useMemo, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Plus, X } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import {
  FieldDef,
  MatchOutcome,
  Preference,
  ProfileSummary,
  Strength,
  kindName,
  kindOptions,
} from '@/types/matchmaking';
import {
  OPS_BY_KIND,
  PrefRow,
  defaultOp,
  describePreference,
  fromPreference,
  isConditionField,
  toPreference,
} from '@/lib/conditions';
import { ConditionOp } from '@/types/matchmaking';

const STRENGTHS: { value: Strength; label: string; help: string }[] = [
  { value: 'required', label: 'Must have', help: 'Candidates that do not satisfy this are excluded' },
  { value: 'deal_breaker', label: 'Deal-breaker', help: 'Candidates that match this are excluded' },
  { value: 'preferred', label: 'Preferred', help: 'Raises the score; weight = importance' },
  { value: 'flexible', label: 'Flexible', help: 'Nice to have; counts half' },
];

const RESULT_STYLE: Record<string, string> = {
  true: 'bg-green-100 text-green-800',
  false: 'bg-red-100 text-red-800',
  unknown: 'bg-amber-100 text-amber-800',
};

interface Props {
  profileId: string;
  fields: FieldDef[];
}

export function PreferencesPanel({ profileId, fields }: Props) {
  const searchable = useMemo(() => fields.filter(isConditionField), [fields]);
  const byKey = useMemo(() => Object.fromEntries(fields.map((f) => [f.key, f])), [fields]);
  const numeric = useMemo(() => searchable.filter((f) => kindName(f.kind) === 'number'), [searchable]);

  const [rows, setRows] = useState<PrefRow[]>([]);
  const [saved, setSaved] = useState<Preference[]>([]);
  const [dirty, setDirty] = useState(false);
  const [saving, setSaving] = useState(false);
  const [candidates, setCandidates] = useState<ProfileSummary[]>([]);
  const [candidateId, setCandidateId] = useState('');
  const [outcome, setOutcome] = useState<MatchOutcome | null>(null);

  const load = useCallback(async () => {
    try {
      const [prefs, list] = await Promise.all([
        invoke<Preference[]>('mm_get_preferences', { profileId }),
        invoke<ProfileSummary[]>('mm_list_profiles', { includeDeactivated: false }),
      ]);
      setSaved(prefs);
      setRows(prefs.map((p) => fromPreference(p, byKey)));
      setCandidates(list.filter((p) => p.id !== profileId));
      setDirty(false);
    } catch (e) {
      toast.error(`Could not load preferences: ${e}`);
    }
  }, [profileId, byKey]);

  useEffect(() => {
    if (fields.length > 0) void load();
  }, [load, fields.length]);

  const update = (i: number, patch: Partial<PrefRow>) => {
    setRows((rs) => rs.map((r, j) => (j === i ? { ...r, ...patch } : r)));
    setDirty(true);
  };

  const addRow = () => {
    const first = searchable[0];
    if (!first) return;
    setRows((rs) => [
      ...rs,
      {
        id: '',
        field: first.key,
        op: defaultOp(first),
        a: '',
        b: '',
        multi: [],
        strength: 'preferred',
        importance: 3,
        note: '',
        rel: false,
        relField: first.key,
      },
    ]);
    setDirty(true);
  };

  const save = async () => {
    const items: Preference[] = [];
    for (const r of rows) {
      const def = byKey[r.field];
      if (!def) continue;
      const p = toPreference(r, def);
      if (typeof p === 'string') {
        toast.error(p);
        return;
      }
      items.push(p);
    }
    setSaving(true);
    try {
      const out = await invoke<Preference[]>('mm_save_preferences', { profileId, items });
      setSaved(out);
      setRows(out.map((p) => fromPreference(p, byKey)));
      setDirty(false);
      setOutcome(null);
      toast.success('Preferences saved');
    } catch (e) {
      toast.error(`${e}`);
    } finally {
      setSaving(false);
    }
  };

  const check = async () => {
    if (!candidateId) return;
    try {
      setOutcome(await invoke<MatchOutcome>('mm_evaluate_preferences', { profileId, candidateId }));
    } catch (e) {
      toast.error(`${e}`);
    }
  };

  const label = (ruleId: string) => {
    const p = saved.find((x) => x.id === ruleId);
    return p ? describePreference(p, byKey) : ruleId;
  };

  const sel = 'border rounded-md px-2 py-1.5 text-sm bg-white';

  return (
    <div className="bg-white rounded-lg border">
      <div className="p-4 flex items-center justify-between border-b">
        <div>
          <h2 className="font-medium">Partner preferences</h2>
          <p className="text-xs text-gray-500">What this person is looking for. Matching uses these as data, not as code.</p>
        </div>
        <div className="flex gap-2">
          <Button variant="outline" size="sm" onClick={addRow} disabled={searchable.length === 0}>
            <Plus className="w-4 h-4 mr-1" /> Add preference
          </Button>
          <Button size="sm" onClick={save} disabled={!dirty || saving}>
            {saving ? 'Saving…' : 'Save preferences'}
          </Button>
        </div>
      </div>

      {rows.length === 0 && <p className="p-4 text-sm text-gray-500">No preferences yet.</p>}

      <div className="divide-y">
        {rows.map((r, i) => {
          const def = byKey[r.field];
          if (!def) return null;
          const kind = kindName(def.kind);
          const ops = OPS_BY_KIND[kind] ?? [];
          const opts = kindOptions(def.kind);
          const showRel = kind === 'number' && r.op !== 'exists';
          return (
            <div key={i} className="p-4 space-y-2">
              <div className="flex flex-wrap items-center gap-2">
                <span className="text-sm text-gray-500">Partner’s</span>
                <select
                  className={sel}
                  value={r.field}
                  onChange={(e) => {
                    const nd = byKey[e.target.value];
                    update(i, { field: e.target.value, op: defaultOp(nd), a: '', b: '', multi: [], rel: false, relField: e.target.value });
                  }}
                >
                  {searchable.map((f) => (
                    <option key={f.key} value={f.key}>{f.label}</option>
                  ))}
                </select>
                <select className={sel} value={r.op} onChange={(e) => update(i, { op: e.target.value as ConditionOp })}>
                  {ops.map((o) => (
                    <option key={o.op} value={o.op}>{o.label}</option>
                  ))}
                </select>

                {showRel && (
                  <label className="flex items-center gap-1 text-sm">
                    <input type="checkbox" checked={r.rel} onChange={(e) => update(i, { rel: e.target.checked, a: '', b: '' })} />
                    relative to their own
                  </label>
                )}
                {showRel && r.rel && (
                  <select className={sel} value={r.relField} onChange={(e) => update(i, { relField: e.target.value })}>
                    {numeric.map((f) => (
                      <option key={f.key} value={f.key}>{f.label}</option>
                    ))}
                  </select>
                )}

                {r.op !== 'exists' && kind === 'number' && (
                  <>
                    {r.rel && <span className="text-sm text-gray-500">+</span>}
                    <Input className="w-24" type="number" placeholder={r.rel ? 'offset' : ''} value={r.a} onChange={(e) => update(i, { a: e.target.value })} />
                    {r.op === 'between' && (
                      <>
                        <span className="text-sm text-gray-500">and{r.rel ? ' +' : ''}</span>
                        <Input className="w-24" type="number" placeholder={r.rel ? 'offset' : ''} value={r.b} onChange={(e) => update(i, { b: e.target.value })} />
                      </>
                    )}
                  </>
                )}
                {r.op !== 'exists' && kind === 'text' && (
                  <Input className="w-56" value={r.a} onChange={(e) => update(i, { a: e.target.value })} />
                )}
                {r.op !== 'exists' && kind === 'bool' && (
                  <select className={sel} value={r.a || 'true'} onChange={(e) => update(i, { a: e.target.value })}>
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
                          onChange={(e) => update(i, { multi: e.target.checked ? [...r.multi, o] : r.multi.filter((x) => x !== o) })}
                        />
                        {o.replace(/_/g, ' ')}
                      </label>
                    ))}
                  </div>
                )}
                {r.op === 'ne' && kind === 'choice' && (
                  <select className={sel} value={r.a} onChange={(e) => update(i, { a: e.target.value })}>
                    <option value="">choose…</option>
                    {opts.map((o) => (
                      <option key={o} value={o}>{o.replace(/_/g, ' ')}</option>
                    ))}
                  </select>
                )}
                <div className="flex-1" />
                <button
                  className="p-1 rounded hover:bg-gray-100"
                  aria-label="Remove preference"
                  onClick={() => {
                    setRows((rs) => rs.filter((_, j) => j !== i));
                    setDirty(true);
                  }}
                >
                  <X className="w-4 h-4" />
                </button>
              </div>

              <div className="flex flex-wrap items-center gap-3">
                <select className={sel} value={r.strength} onChange={(e) => update(i, { strength: e.target.value as Strength })} title={STRENGTHS.find((s) => s.value === r.strength)?.help}>
                  {STRENGTHS.map((s) => (
                    <option key={s.value} value={s.value}>{s.label}</option>
                  ))}
                </select>
                {(r.strength === 'preferred' || r.strength === 'flexible') && (
                  <label className="flex items-center gap-1 text-sm">
                    importance
                    <select className={sel} value={r.importance} onChange={(e) => update(i, { importance: Number(e.target.value) })}>
                      {[1, 2, 3, 4, 5].map((n) => (
                        <option key={n} value={n}>{n}</option>
                      ))}
                    </select>
                  </label>
                )}
                <Input className="flex-1 min-w-[12rem]" placeholder="Note for the matchmaker (optional)" value={r.note} onChange={(e) => update(i, { note: e.target.value })} />
              </div>
            </div>
          );
        })}
      </div>

      <div className="p-4 border-t space-y-3">
        <div className="flex flex-wrap items-center gap-2">
          <span className="text-sm font-medium">Check against a candidate</span>
          <select className={sel} value={candidateId} onChange={(e) => { setCandidateId(e.target.value); setOutcome(null); }}>
            <option value="">choose…</option>
            {candidates.map((c) => (
              <option key={c.id} value={c.id}>{c.name || 'Unnamed profile'}{c.age != null ? `, ${c.age}` : ''}</option>
            ))}
          </select>
          <Button variant="outline" size="sm" onClick={check} disabled={!candidateId || dirty}>Check</Button>
          {dirty && <span className="text-xs text-gray-500">Save first to check the saved preferences.</span>}
        </div>

        {outcome && (
          <div className="text-sm space-y-2">
            <div className="flex flex-wrap items-center gap-3">
              <span className={`px-2 py-0.5 rounded ${outcome.eligible ? 'bg-green-100 text-green-800' : 'bg-red-100 text-red-800'}`}>
                {outcome.eligible ? 'Eligible' : 'Excluded by a must-have or deal-breaker'}
              </span>
              <span>Score: {outcome.soft_score != null ? `${Math.round(outcome.soft_score)}/100` : 'n/a (nothing to score yet)'}</span>
              {outcome.needs_info.length > 0 && <span className="text-amber-700">{outcome.needs_info.length} must-have(s) cannot be decided: information missing</span>}
            </div>
            <ul className="space-y-1">
              {outcome.results.map((res) => (
                <li key={res.rule_id} className="flex items-center gap-2">
                  <span className={`px-1.5 py-0.5 rounded text-xs ${RESULT_STYLE[res.result]}`}>
                    {res.result === 'unknown' ? 'unknown' : res.kind === 'hard' ? (res.result === 'true' ? 'ok' : 'violated') : res.result === 'true' ? 'met' : 'not met'}
                  </span>
                  <span>{label(res.rule_id)}</span>
                </li>
              ))}
            </ul>
          </div>
        )}
      </div>
    </div>
  );
}
