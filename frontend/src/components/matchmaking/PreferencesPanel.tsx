'use client';

import { fieldLabel } from '@/lib/field-label';
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
import { useI18n } from '@/i18n';
import type { MessageKey } from '@/i18n/types';
import { ConditionOp } from '@/types/matchmaking';

const STRENGTHS: { value: Strength; label: MessageKey; help: MessageKey }[] = [
  { value: 'required', label: 'prefsPanel.strengthRequired', help: 'prefsPanel.strengthRequiredHelp' },
  { value: 'deal_breaker', label: 'prefsPanel.strengthDealBreaker', help: 'prefsPanel.strengthDealBreakerHelp' },
  { value: 'preferred', label: 'prefsPanel.strengthPreferred', help: 'prefsPanel.strengthPreferredHelp' },
  { value: 'flexible', label: 'prefsPanel.strengthFlexible', help: 'prefsPanel.strengthFlexibleHelp' },
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
  const { t, fmt, locale } = useI18n();
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
      toast.error(t('prefsPanel.loadError', { error: `${e}` }));
    }
  }, [profileId, byKey, t]);

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
      const p = toPreference(r, def, t, locale);
      if (typeof p === 'string') {
        toast.error(p);
        return;
      }
      items.push(p);
    }
    setSaving(true);
    try {
      const out = await invoke<Preference[]>('mm_save_preferences', { profileId, items, locale });
      setSaved(out);
      setRows(out.map((p) => fromPreference(p, byKey)));
      setDirty(false);
      setOutcome(null);
      toast.success(t('prefsPanel.saved'));
    } catch (e) {
      toast.error(`${e}`);
    } finally {
      setSaving(false);
    }
  };

  const check = async () => {
    if (!candidateId) return;
    try {
      setOutcome(await invoke<MatchOutcome>('mm_evaluate_preferences', { profileId, candidateId, locale }));
    } catch (e) {
      toast.error(`${e}`);
    }
  };

  const label = (ruleId: string) => {
    const p = saved.find((x) => x.id === ruleId);
    return p ? describePreference(p, byKey, t, locale) : ruleId;
  };

  const sel = 'border rounded-md px-2 py-1.5 text-sm bg-white';

  return (
    <div className="bg-white rounded-lg border">
      <div className="p-4 flex items-center justify-between border-b">
        <div>
          <h2 className="font-medium">{t('prefsPanel.title')}</h2>
          <p className="text-xs text-gray-500">{t('prefsPanel.intro')}</p>
        </div>
        <div className="flex gap-2">
          <Button variant="outline" size="sm" onClick={addRow} disabled={searchable.length === 0}>
            <Plus className="w-4 h-4 me-1" /> {t('prefsPanel.add')}
          </Button>
          <Button size="sm" onClick={save} disabled={!dirty || saving}>
            {saving ? t('prefsPanel.saving') : t('prefsPanel.save')}
          </Button>
        </div>
      </div>

      {rows.length === 0 && <p className="p-4 text-sm text-gray-500">{t('prefsPanel.none')}</p>}

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
                <span className="text-sm text-gray-500">{t('prefsPanel.partners')}</span>
                <select
                  className={sel}
                  value={r.field}
                  onChange={(e) => {
                    const nd = byKey[e.target.value];
                    update(i, { field: e.target.value, op: defaultOp(nd), a: '', b: '', multi: [], rel: false, relField: e.target.value });
                  }}
                >
                  {searchable.map((f) => (
                    <option key={f.key} value={f.key}>{fieldLabel(f, locale)}</option>
                  ))}
                </select>
                <select className={sel} value={r.op} onChange={(e) => update(i, { op: e.target.value as ConditionOp })}>
                  {ops.map((o) => (
                    <option key={o.op} value={o.op}>{t(o.labelKey)}</option>
                  ))}
                </select>

                {showRel && (
                  <label className="flex items-center gap-1 text-sm">
                    <input type="checkbox" checked={r.rel} onChange={(e) => update(i, { rel: e.target.checked, a: '', b: '' })} />
                    {t('prefsPanel.relative')}
                  </label>
                )}
                {showRel && r.rel && (
                  <select className={sel} value={r.relField} onChange={(e) => update(i, { relField: e.target.value })}>
                    {numeric.map((f) => (
                      <option key={f.key} value={f.key}>{fieldLabel(f, locale)}</option>
                    ))}
                  </select>
                )}

                {r.op !== 'exists' && kind === 'number' && (
                  <>
                    {r.rel && <span className="text-sm text-gray-500">+</span>}
                    <Input className="w-24" dir="ltr" type="number" placeholder={r.rel ? t('prefsPanel.offset') : ''} value={r.a} onChange={(e) => update(i, { a: e.target.value })} />
                    {r.op === 'between' && (
                      <>
                        <span className="text-sm text-gray-500">{t('prefsPanel.and')}{r.rel ? ' +' : ''}</span>
                        <Input className="w-24" dir="ltr" type="number" placeholder={r.rel ? t('prefsPanel.offset') : ''} value={r.b} onChange={(e) => update(i, { b: e.target.value })} />
                      </>
                    )}
                  </>
                )}
                {r.op !== 'exists' && kind === 'text' && (
                  <Input className="w-56" dir="auto" value={r.a} onChange={(e) => update(i, { a: e.target.value })} />
                )}
                {r.op !== 'exists' && kind === 'bool' && (
                  <select className={sel} value={r.a || 'true'} onChange={(e) => update(i, { a: e.target.value })}>
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
                          onChange={(e) => update(i, { multi: e.target.checked ? [...r.multi, o] : r.multi.filter((x) => x !== o) })}
                        />
                        {o.replace(/_/g, ' ')}
                      </label>
                    ))}
                  </div>
                )}
                {r.op === 'ne' && kind === 'choice' && (
                  <select className={sel} value={r.a} onChange={(e) => update(i, { a: e.target.value })}>
                    <option value="">{t('profiles.choose')}</option>
                    {opts.map((o) => (
                      <option key={o} value={o}>{o.replace(/_/g, ' ')}</option>
                    ))}
                  </select>
                )}
                <div className="flex-1" />
                <button
                  className="p-1 rounded hover:bg-gray-100"
                  aria-label={t('prefsPanel.remove')}
                  onClick={() => {
                    setRows((rs) => rs.filter((_, j) => j !== i));
                    setDirty(true);
                  }}
                >
                  <X className="w-4 h-4" />
                </button>
              </div>

              <div className="flex flex-wrap items-center gap-3">
                <select className={sel} value={r.strength} onChange={(e) => update(i, { strength: e.target.value as Strength })} title={t(STRENGTHS.find((s) => s.value === r.strength)?.help ?? 'prefsPanel.strengthRequiredHelp')}>
                  {STRENGTHS.map((s) => (
                    <option key={s.value} value={s.value}>{t(s.label)}</option>
                  ))}
                </select>
                {(r.strength === 'preferred' || r.strength === 'flexible') && (
                  <label className="flex items-center gap-1 text-sm">
                    {t('prefsPanel.importance')}
                    <select className={sel} value={r.importance} onChange={(e) => update(i, { importance: Number(e.target.value) })}>
                      {[1, 2, 3, 4, 5].map((n) => (
                        <option key={n} value={n}>{fmt.number(n)}</option>
                      ))}
                    </select>
                  </label>
                )}
                <Input className="flex-1 min-w-[12rem]" dir="auto" placeholder={t('prefsPanel.notePlaceholder')} value={r.note} onChange={(e) => update(i, { note: e.target.value })} />
              </div>
            </div>
          );
        })}
      </div>

      <div className="p-4 border-t space-y-3">
        <div className="flex flex-wrap items-center gap-2">
          <span className="text-sm font-medium">{t('prefsPanel.checkTitle')}</span>
          <select className={sel} value={candidateId} onChange={(e) => { setCandidateId(e.target.value); setOutcome(null); }}>
            <option value="">{t('profiles.choose')}</option>
            {candidates.map((c) => (
              <option key={c.id} value={c.id}><bdi>{c.name || t('profiles.unnamed')}</bdi>{c.age != null ? `${t('prefsPanel.ageSeparator')}${fmt.number(c.age)}` : ''}</option>
            ))}
          </select>
          <Button variant="outline" size="sm" onClick={check} disabled={!candidateId || dirty}>{t('prefsPanel.check')}</Button>
          {dirty && <span className="text-xs text-gray-500">{t('prefsPanel.saveFirst')}</span>}
        </div>

        {outcome && (
          <div className="text-sm space-y-2">
            <div className="flex flex-wrap items-center gap-3">
              <span className={`px-2 py-0.5 rounded ${outcome.eligible ? 'bg-green-100 text-green-800' : 'bg-red-100 text-red-800'}`}>
                {outcome.eligible ? t('prefsPanel.eligible') : t('prefsPanel.excluded')}
              </span>
              <span>{outcome.soft_score != null ? t('prefsPanel.score', { score: fmt.number(Math.round(outcome.soft_score)) }) : t('prefsPanel.scoreNone')}</span>
              {outcome.needs_info.length > 0 && <span className="text-amber-700">{t('prefsPanel.needsInfo', { count: fmt.number(outcome.needs_info.length) })}</span>}
            </div>
            <ul className="space-y-1">
              {outcome.results.map((res) => (
                <li key={res.rule_id} className="flex items-center gap-2">
                  <span className={`px-1.5 py-0.5 rounded text-xs ${RESULT_STYLE[res.result]}`}>
                    {res.result === 'unknown' ? t('ruleResult.unknown') : res.kind === 'hard' ? (res.result === 'true' ? t('ruleResult.ok') : t('ruleResult.violated')) : res.result === 'true' ? t('ruleResult.met') : t('ruleResult.notMet')}
                  </span>
                  <span dir="auto">{label(res.rule_id)}</span>
                </li>
              ))}
            </ul>
          </div>
        )}
      </div>
    </div>
  );
}
