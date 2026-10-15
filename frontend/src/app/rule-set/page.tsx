'use client';

import React, { Suspense, useCallback, useEffect, useMemo, useState } from 'react';
import { useRouter, useSearchParams } from 'next/navigation';
import { invoke } from '@tauri-apps/api/core';
import { ArrowLeft, ChevronDown, ChevronRight, Plus, Trash2 } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { ExprEditor } from '@/components/matchmaking/ExprEditor';
import { RuleResultList } from '@/components/matchmaking/RuleResultList';
import { useI18n } from '@/i18n';
import { isConditionField } from '@/lib/conditions';
import { describeExpr, emptyRuleSet, newNode, newRule } from '@/lib/rules';
import { DimensionDef, FieldDef, MatchEvaluation, ProfileSummary, Rule, RuleIssue, RuleSetView, ScoringConfigSummary } from '@/types/matchmaking';

const sel = 'border rounded-md px-2 py-1.5 text-sm bg-white';

function Editor() {
  const { t, fmt, locale } = useI18n();
  const router = useRouter();
  const idParam = useSearchParams().get('id');
  const isNew = idParam === 'new' || !idParam;

  const [fields, setFields] = useState<FieldDef[]>([]);
  const [dims, setDims] = useState<DimensionDef[]>([]);
  const [priorScore, setPriorScore] = useState('');
  const [meta, setMeta] = useState<RuleSetView | null>(null);
  const [name, setName] = useState('');
  const [description, setDescription] = useState('');
  const [configs, setConfigs] = useState<ScoringConfigSummary[]>([]);
  const [scoringConfigId, setScoringConfigId] = useState('');
  const [rules, setRules] = useState<Rule[]>([]);
  const [minScore, setMinScore] = useState('');
  const [groupWeights, setGroupWeights] = useState<{ group: string; weight: string }[]>([]);
  const [note, setNote] = useState('');
  const [open, setOpen] = useState<Record<number, boolean>>({});
  const [issues, setIssues] = useState<RuleIssue[]>([]);
  const [dirty, setDirty] = useState(false);
  const [saving, setSaving] = useState(false);
  const [profiles, setProfiles] = useState<ProfileSummary[]>([]);
  const [pa, setPa] = useState('');
  const [pb, setPb] = useState('');
  const [evalResult, setEvalResult] = useState<MatchEvaluation | null>(null);

  const usable = useMemo(() => fields.filter(isConditionField), [fields]);
  const byKey = useMemo(() => Object.fromEntries(fields.map((f) => [f.key, f])), [fields]);

  const apply = useCallback((v: RuleSetView) => {
    setMeta(v);
    setName(v.name);
    setDescription(v.description);
    setScoringConfigId(v.scoring_config_id ?? '');
    setRules(v.definition.rules);
    setMinScore(v.definition.min_score == null ? '' : String(v.definition.min_score));
    setPriorScore(v.definition.prior_score == null ? '' : String(v.definition.prior_score));
    setGroupWeights(Object.entries(v.definition.group_weights).map(([group, w]) => ({ group, weight: String(w) })));
    setNote('');
    setDirty(false);
    setEvalResult(null);
  }, []);

  const load = useCallback(
    async (version?: number) => {
      try {
        const [f, plist, catalog, cfgs] = await Promise.all([
          invoke<FieldDef[]>('mm_list_fields'),
          invoke<ProfileSummary[]>('mm_list_profiles', { includeDeactivated: false }),
          invoke<DimensionDef[]>('mm_dimension_catalog', { locale }),
          invoke<ScoringConfigSummary[]>('mm_list_scoring_configs', { includeArchived: false }),
        ]);
        setFields(f);
        setDims(catalog);
        setProfiles(plist);
        setConfigs(cfgs);
        if (isNew) {
          const e = emptyRuleSet();
          setMeta(null);
          setName('');
          setDescription('');
          setScoringConfigId('');
          setRules(e.rules);
          setMinScore('');
          setPriorScore('');
          setGroupWeights([]);
          setDirty(false);
        } else {
          apply(await invoke<RuleSetView>('mm_get_rule_set', { id: idParam, version: version ?? null }));
          if (version) setDirty(true); // viewing an old version: saving makes it the new current version
        }
      } catch (e) {
        toast.error(t('ruleSet.loadError', { error: `${e}` }));
      }
    },
    [apply, idParam, isNew, t]
  );

  useEffect(() => { void load(); }, [load]);
  useEffect(() => {
    invoke<DimensionDef[]>('mm_dimension_catalog', { locale }).then(setDims).catch(() => {});
  }, [locale]);

  const buildDefinition = useCallback(() => {
    const gw: Record<string, number> = {};
    for (const g of groupWeights) if (g.group.trim() !== '') gw[g.group.trim()] = Number(g.weight);
    return {
      name: name.trim(),
      version: meta?.current_version ?? 0,
      rules,
      group_weights: gw,
      min_score: minScore.trim() === '' ? null : Number(minScore),
      prior_score: priorScore.trim() === '' ? null : Number(priorScore),
    };
  }, [groupWeights, meta, minScore, name, priorScore, rules]);

  // live validation (debounced); the backend is the single source of truth for what is valid
  useEffect(() => {
    if (fields.length === 0) return;
    const timer = setTimeout(async () => {
      try {
        setIssues(await invoke<RuleIssue[]>('mm_validate_rule_set', { definition: buildDefinition(), locale }));
      } catch {
        /* validation is advisory; saving re-checks */
      }
    }, 400);
    return () => clearTimeout(timer);
  }, [buildDefinition, fields.length, locale]);

  const touch = () => setDirty(true);
  const updateRule = (i: number, patch: Partial<Rule>) => {
    setRules((rs) => rs.map((r, j) => (j === i ? { ...r, ...patch } : r)));
    touch();
  };

  const save = async () => {
    setSaving(true);
    try {
      const v = await invoke<RuleSetView>('mm_save_rule_set', {
        id: isNew ? null : idParam,
        name: name.trim(),
        description,
        definition: buildDefinition(),
        scoringConfigId: scoringConfigId === '' ? null : scoringConfigId,
        note: note || null,
        locale,
      });
      toast.success(t('ruleSet.saved', { version: fmt.number(v.current_version) }));
      if (isNew) router.replace(`/rule-set?id=${encodeURIComponent(v.id)}`);
      else apply(v);
    } catch (e) {
      toast.error(`${e}`);
    } finally {
      setSaving(false);
    }
  };

  const evaluate = async () => {
    if (!meta || !pa || !pb) return;
    try {
      setEvalResult(await invoke<MatchEvaluation>('mm_evaluate_match', { ruleSetId: meta.id, profileA: pa, profileB: pb, version: null }));
    } catch (e) {
      toast.error(`${e}`);
    }
  };

  const setupIssues = issues.filter((i) => i.rule_id === null);
  const nameOf = (id: string) => profiles.find((p) => p.id === id)?.name || t('ruleSet.unnamed');

  return (
    <div className="min-h-screen bg-gray-50">
      <div className="max-w-4xl mx-auto p-6 space-y-5">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-3">
            <button onClick={() => router.push('/rules')} className="p-2 rounded-lg hover:bg-gray-200" aria-label={t('ruleSet.back')}>
              <ArrowLeft className="w-5 h-5 rtl-mirror" />
            </button>
            <h1 className="text-2xl font-semibold">{isNew ? t('ruleSet.newTitle') : name ? <bdi>{name}</bdi> : t('ruleSet.fallbackTitle')}</h1>
            {meta && <span className="text-sm text-gray-500">{meta.version !== meta.current_version ? t('ruleSet.showingCurrent', { version: fmt.number(meta.version), current: fmt.number(meta.current_version) }) : t('ruleSet.showing', { version: fmt.number(meta.version) })}</span>}
          </div>
          <Button onClick={save} disabled={saving || (!dirty && !isNew) || name.trim() === ''}>
            {saving ? t('ruleSet.saving') : isNew ? t('ruleSet.create') : t('ruleSet.saveVersion')}
          </Button>
        </div>

        {meta && meta.version !== meta.current_version && (
          <div className="bg-amber-50 border border-amber-200 rounded-lg p-3 text-sm">
            {t('ruleSet.olderVersion')}
          </div>
        )}

        <div className="bg-white rounded-lg border p-4 space-y-3">
          <div className="grid grid-cols-3 gap-3 items-center">
            <label className="text-sm font-medium">{t('ruleSet.name')}</label>
            <Input className="col-span-2" dir="auto" value={name} onChange={(e) => { setName(e.target.value); touch(); }} />
            <label className="text-sm font-medium">{t('ruleSet.description')}</label>
            <Input className="col-span-2" dir="auto" value={description} onChange={(e) => { setDescription(e.target.value); touch(); }} />
            <label className="text-sm font-medium" title={t('ruleSet.minScoreTip')}>{t('ruleSet.minScore')}</label>
            <Input className="w-28" dir="ltr" type="number" placeholder={t('ruleSet.minScorePlaceholder')} value={minScore} onChange={(e) => { setMinScore(e.target.value); touch(); }} />
            <label className="text-sm font-medium" title={t('ruleSet.baselineTip')}>{t('ruleSet.baseline')}</label>
            <Input className="w-28" dir="ltr" type="number" placeholder="50" value={priorScore} onChange={(e) => { setPriorScore(e.target.value); touch(); }} />
          </div>

          <div className="border-t pt-3">
            <div className="flex items-center gap-2">
              <label className="text-sm font-medium">{t('ruleSet.scoringConfig')}</label>
              <select className={sel} value={scoringConfigId} onChange={(e) => { setScoringConfigId(e.target.value); touch(); }}>
                <option value="">{t('ruleSet.noConfig')}</option>
                {configs.map((c) => (
                  <option key={c.id} value={c.id}>
                    {t('ruleSet.configOption', { name: c.name, version: fmt.number(c.current_version) })}
                  </option>
                ))}
              </select>
              <button className="text-sm text-blue-600 hover:underline" onClick={() => router.push('/scoring')}>
                {t('ruleSet.editConfigs')}
              </button>
            </div>
            <p className="text-xs text-gray-500 mt-1">
              {t('ruleSet.configNote')}
              {isNew ? t('ruleSet.configNoteNew') : ''}
            </p>
          </div>

          <div>
            <div className="flex items-center justify-between">
              <span className="text-sm font-medium">{t('ruleSet.dimWeights')}</span>
              <button className="text-sm text-blue-600 hover:underline" onClick={() => { setGroupWeights((g) => [...g, { group: '', weight: '1' }]); touch(); }}>
                {t('ruleSet.add')}
              </button>
            </div>
            <p className="text-xs text-gray-500">{t('ruleSet.dimWeightsNote')}</p>
            {groupWeights.map((g, i) => (
              <div key={i} className="flex items-center gap-2 mt-1">
                <select className={sel} value={g.group} onChange={(e) => { setGroupWeights((gs) => gs.map((x, j) => (j === i ? { ...x, group: e.target.value } : x))); touch(); }}>
                  <option value="">{t('ruleSet.chooseDimension')}</option>
                  {dims.map((d) => <option key={d.key} value={d.key}>{d.label}</option>)}
                  {g.group !== '' && !dims.some((d) => d.key === g.group) && <option value={g.group}>{t('ruleSet.customDimension', { group: g.group })}</option>}
                </select>
                <span className="text-sm">×</span>
                <Input className="w-24" dir="ltr" type="number" step="0.1" value={g.weight} onChange={(e) => { setGroupWeights((gs) => gs.map((x, j) => (j === i ? { ...x, weight: e.target.value } : x))); touch(); }} />
                <button className="p-1 rounded hover:bg-gray-100" aria-label={t('ruleSet.removeWeight')} onClick={() => { setGroupWeights((gs) => gs.filter((_, j) => j !== i)); touch(); }}>
                  <Trash2 className="w-4 h-4" />
                </button>
              </div>
            ))}
          </div>

          {setupIssues.length > 0 && (
            <div className="bg-red-50 border border-red-200 rounded p-2 text-sm text-red-800">
              {setupIssues.map((i, k) => <div key={k}>{i.message}</div>)}
            </div>
          )}
        </div>

        <div className="flex items-center justify-between">
          <h2 className="font-medium">{t('ruleSet.rulesCount', { count: fmt.number(rules.length) })}</h2>
          <Button variant="outline" size="sm" onClick={() => { setRules((rs) => { const r = newRule(rs, fields); setOpen((o) => ({ ...o, [rs.length]: true })); return [...rs, r]; }); touch(); }} disabled={usable.length === 0}>
            <Plus className="w-4 h-4 me-1" /> {t('ruleSet.addRule')}
          </Button>
        </div>

        <div className="space-y-3">
          {rules.map((r, i) => {
            const ruleIssues = issues.filter((x) => x.rule_id === r.id);
            const expanded = open[i] ?? false;
            return (
              <div key={i} className={`bg-white rounded-lg border ${r.enabled ? '' : 'opacity-60'}`}>
                <div className="p-3 flex items-center gap-3">
                  <button aria-label={expanded ? t('ruleSet.collapse') : t('ruleSet.expand')} onClick={() => setOpen((o) => ({ ...o, [i]: !expanded }))}>
                    {expanded ? <ChevronDown className="w-4 h-4" /> : <ChevronRight className="w-4 h-4" />}
                  </button>
                  <div className="flex-1 min-w-0">
                    <div className="font-medium truncate">{r.description ? <bdi>{r.description}</bdi> : <span className="text-gray-400">{t('ruleSet.untitled')}</span>}</div>
                    {!expanded && <div className="text-xs text-gray-500 truncate" dir="auto">{r.when ? t('ruleSet.whenPrefix', { when: describeExpr(r.when, byKey, t), expr: describeExpr(r.expr, byKey, t) }) : describeExpr(r.expr, byKey, t)}</div>}
                  </div>
                  <span className={`text-xs px-2 py-0.5 rounded ${r.kind === 'hard' ? 'bg-red-100 text-red-800' : 'bg-blue-100 text-blue-800'}`}>{r.kind === 'hard' ? t('ruleSet.mustHold') : t('ruleSet.softWeight', { weight: fmt.number(r.weight) })}</span>
                  {r.scope === 'directional' && <span className="text-xs px-2 py-0.5 rounded bg-purple-100 text-purple-800">{t('ruleSet.bothDirections')}</span>}
                  {r.group && <span className="text-xs px-2 py-0.5 rounded bg-gray-100"><bdi>{r.group}</bdi></span>}
                  {ruleIssues.length > 0 && <span className="text-xs px-2 py-0.5 rounded bg-red-600 text-white">{t(ruleIssues.length === 1 ? 'ruleSet.issueOne' : 'ruleSet.issueMany', { count: fmt.number(ruleIssues.length) })}</span>}
                  <label className="flex items-center gap-1 text-xs"><input type="checkbox" checked={r.enabled} onChange={(e) => updateRule(i, { enabled: e.target.checked })} /> {t('ruleSet.on')}</label>
                </div>

                {expanded && (
                  <div className="border-t p-4 space-y-4">
                    {ruleIssues.length > 0 && (
                      <div className="bg-red-50 border border-red-200 rounded p-2 text-sm text-red-800">
                        {ruleIssues.map((x, k) => <div key={k}>{x.message}</div>)}
                      </div>
                    )}
                    <div className="grid grid-cols-4 gap-3 items-center">
                      <label className="text-sm font-medium">{t('ruleSet.description')}</label>
                      <Input className="col-span-3" dir="auto" value={r.description} onChange={(e) => updateRule(i, { description: e.target.value })} />
                      <label className="text-sm font-medium">{t('ruleSet.type')}</label>
                      <div className="col-span-3 flex flex-wrap items-center gap-3">
                        <select className={sel} value={r.kind} onChange={(e) => updateRule(i, { kind: e.target.value as 'hard' | 'soft' })}>
                          <option value="hard">{t('ruleSet.kindHard')}</option>
                          <option value="soft">{t('ruleSet.kindSoft')}</option>
                        </select>
                        {r.kind === 'soft' && (
                          <label className="flex items-center gap-1 text-sm">{t('ruleSet.weight')} <Input className="w-20" dir="ltr" type="number" step="0.5" value={String(r.weight)} onChange={(e) => updateRule(i, { weight: Number(e.target.value) })} /></label>
                        )}
                      </div>
                      <label className="text-sm font-medium">{t('ruleSet.appliesTo')}</label>
                      <div className="col-span-3">
                        <select className={sel} value={r.scope} onChange={(e) => updateRule(i, { scope: e.target.value as 'pair' | 'directional' })}>
                          <option value="pair">{t('ruleSet.scopePair')}</option>
                          <option value="directional">{t('ruleSet.scopeDirectional')}</option>
                        </select>
                      </div>
                      <label className="text-sm font-medium">{t('ruleSet.dimPriority')}</label>
                      <div className="col-span-3 flex items-center gap-3">
                        <select className={sel} title={t('ruleSet.dimTip')} value={r.group ?? ''} onChange={(e) => updateRule(i, { group: e.target.value === '' ? null : e.target.value })}>
                          <option value="">{t('ruleSet.noDimension')}</option>
                          {dims.map((d) => <option key={d.key} value={d.key}>{d.label}</option>)}
                          {r.group && !dims.some((d) => d.key === r.group) && <option value={r.group}>{t('ruleSet.mapsToOther', { group: r.group })}</option>}
                        </select>
                        <label className="flex items-center gap-1 text-sm" title={t('ruleSet.priorityTip')}>{t('ruleSet.priority')} <Input className="w-20" dir="ltr" type="number" value={String(r.priority)} onChange={(e) => updateRule(i, { priority: Number(e.target.value) })} /></label>
                        <label className="flex items-center gap-1 text-sm">{t('ruleSet.ruleId')} <Input className="w-36" dir="ltr" value={r.id} onChange={(e) => updateRule(i, { id: e.target.value })} /></label>
                      </div>
                    </div>

                    <div>
                      <div className="flex items-center gap-3 mb-1">
                        <span className="text-sm font-medium">{t('ruleSet.appliesOnly')}</span>
                        {r.when ? (
                          <button className="text-sm text-blue-600 hover:underline" onClick={() => updateRule(i, { when: null })}>{t('ruleSet.alwaysInstead')}</button>
                        ) : (
                          <button className="text-sm text-blue-600 hover:underline" onClick={() => updateRule(i, { when: newNode('cmp', fields) })}>{t('ruleSet.addCondition')}</button>
                        )}
                      </div>
                      {r.when ? (
                        <ExprEditor expr={r.when} fields={fields} byKey={byKey} onChange={(when) => updateRule(i, { when })} />
                      ) : (
                        <p className="text-xs text-gray-500">{t('ruleSet.alwaysNote')}</p>
                      )}
                    </div>

                    <div>
                      <div className="text-sm font-medium mb-1">{t('ruleSet.mustHoldTitle')}</div>
                      <ExprEditor expr={r.expr} fields={fields} byKey={byKey} onChange={(expr) => updateRule(i, { expr })} />
                    </div>

                    <div className="flex justify-end">
                      <Button variant="ghost" size="sm" onClick={() => { setRules((rs) => rs.filter((_, j) => j !== i)); touch(); }}>
                        <Trash2 className="w-4 h-4 me-1" /> {t('ruleSet.deleteRule')}
                      </Button>
                    </div>
                  </div>
                )}
              </div>
            );
          })}
          {rules.length === 0 && <div className="bg-white rounded-lg border p-6 text-center text-sm text-gray-500">{t('ruleSet.noRules')}</div>}
        </div>

        {(dirty || isNew) && (
          <div className="bg-white rounded-lg border p-4">
            <label className="text-sm font-medium">{t('ruleSet.noteLabel')}</label>
            <Input className="mt-1" dir="auto" value={note} onChange={(e) => setNote(e.target.value)} />
          </div>
        )}

        {meta && (
          <>
            <div className="bg-white rounded-lg border p-4 space-y-3">
              <h2 className="font-medium">{t('ruleSet.testTitle')}</h2>
              <div className="flex flex-wrap items-center gap-2">
                <select className={sel} value={pa} onChange={(e) => { setPa(e.target.value); setEvalResult(null); }}>
                  <option value="">{t('ruleSet.personA')}</option>
                  {profiles.map((p) => <option key={p.id} value={p.id}>{p.name || t('ruleSet.unnamed')}</option>)}
                </select>
                <select className={sel} value={pb} onChange={(e) => { setPb(e.target.value); setEvalResult(null); }}>
                  <option value="">{t('ruleSet.personB')}</option>
                  {profiles.filter((p) => p.id !== pa).map((p) => <option key={p.id} value={p.id}>{p.name || t('ruleSet.unnamed')}</option>)}
                </select>
                <Button variant="outline" size="sm" onClick={evaluate} disabled={!pa || !pb || dirty}>{t('ruleSet.evaluate')}</Button>
                {dirty && <span className="text-xs text-gray-500">{t('ruleSet.saveFirst')}</span>}
              </div>

              {evalResult && (
                <div className="text-sm space-y-3">
                  <div className="flex flex-wrap items-center gap-3">
                    <span className={`px-2 py-0.5 rounded ${evalResult.eligible ? 'bg-green-100 text-green-800' : 'bg-red-100 text-red-800'}`}>{evalResult.eligible ? t('ruleSet.eligible') : t('ruleSet.excluded')}</span>
                    <span>{t('ruleSet.score', { score: evalResult.score != null ? t('ruleSet.scoreOutOf', { score: fmt.number(Math.round(evalResult.score)) }) : t('ruleSet.scoreNa') })}</span>
                    <span className="text-gray-600">{t('ruleSet.coverage', { percent: fmt.percent(evalResult.coverage ?? 0) })}</span>
                    {evalResult.meets_threshold === false && <span className="text-red-700">{t('ruleSet.belowMin')}</span>}
                    {evalResult.needs_info && <span className="text-amber-700">{t('ruleSet.needsInfo')}</span>}
                  </div>
                  <RuleResultList title={t('ruleSet.rulesPair', { a: nameOf(pa), b: nameOf(pb) })} results={evalResult.rules.results} />
                  <RuleResultList title={t('match.prefsAbout', { a: nameOf(pa), b: nameOf(pb) })} results={evalResult.a_preferences.results} />
                  <RuleResultList title={t('match.prefsAbout', { a: nameOf(pb), b: nameOf(pa) })} results={evalResult.b_preferences.results} />
                </div>
              )}
            </div>

            <div className="bg-white rounded-lg border p-4">
              <h2 className="font-medium mb-2">{t('ruleSet.historyTitle')}</h2>
              <ul className="space-y-1 text-sm">
                {meta.versions.map((v) => (
                  <li key={v.version} className="flex items-center gap-3">
                    <span className="w-10 font-medium">{t('ruleSet.versionLabel', { version: fmt.number(v.version) })}</span>
                    <span className="text-gray-500 w-44">{fmt.dateTime(v.created_at)}</span>
                    <span className="flex-1 truncate" dir="auto">{v.note || ''}</span>
                    {v.version !== meta.version && <button className="text-blue-600 hover:underline" onClick={() => load(v.version)}>{t('ruleSet.view')}</button>}
                    {v.version === meta.version && <span className="text-xs text-gray-500">{t('ruleSet.shownTag')}</span>}
                  </li>
                ))}
              </ul>
            </div>
          </>
        )}
      </div>
    </div>
  );
}

export default function RuleSetPage() {
  const { t } = useI18n();
  const fallbackLoading = t('common.loading');
  return (
    <Suspense fallback={<div className="p-6 text-gray-500">{fallbackLoading}</div>}>
      <Editor />
    </Suspense>
  );
}
