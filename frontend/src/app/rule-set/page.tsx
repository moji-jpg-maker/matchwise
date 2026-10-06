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
import { isConditionField } from '@/lib/conditions';
import { describeExpr, emptyRuleSet, newNode, newRule } from '@/lib/rules';
import { DimensionDef, FieldDef, MatchEvaluation, ProfileSummary, Rule, RuleIssue, RuleSetView } from '@/types/matchmaking';

const sel = 'border rounded-md px-2 py-1.5 text-sm bg-white';

function Editor() {
  const router = useRouter();
  const idParam = useSearchParams().get('id');
  const isNew = idParam === 'new' || !idParam;

  const [fields, setFields] = useState<FieldDef[]>([]);
  const [dims, setDims] = useState<DimensionDef[]>([]);
  const [priorScore, setPriorScore] = useState('');
  const [meta, setMeta] = useState<RuleSetView | null>(null);
  const [name, setName] = useState('');
  const [description, setDescription] = useState('');
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
        const [f, plist, catalog] = await Promise.all([
          invoke<FieldDef[]>('mm_list_fields'),
          invoke<ProfileSummary[]>('mm_list_profiles', { includeDeactivated: false }),
          invoke<DimensionDef[]>('mm_dimension_catalog'),
        ]);
        setFields(f);
        setDims(catalog);
        setProfiles(plist);
        if (isNew) {
          const e = emptyRuleSet();
          setMeta(null);
          setName('');
          setDescription('');
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
        toast.error(`Could not load: ${e}`);
      }
    },
    [apply, idParam, isNew]
  );

  useEffect(() => { void load(); }, [load]);

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
    const t = setTimeout(async () => {
      try {
        setIssues(await invoke<RuleIssue[]>('mm_validate_rule_set', { definition: buildDefinition() }));
      } catch {
        /* validation is advisory; saving re-checks */
      }
    }, 400);
    return () => clearTimeout(t);
  }, [buildDefinition, fields.length]);

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
        note: note || null,
      });
      toast.success(`Saved as version ${v.current_version}`);
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
  const nameOf = (id: string) => profiles.find((p) => p.id === id)?.name || 'Unnamed profile';

  return (
    <div className="min-h-screen bg-gray-50">
      <div className="max-w-4xl mx-auto p-6 space-y-5">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-3">
            <button onClick={() => router.push('/rules')} className="p-2 rounded-lg hover:bg-gray-200" aria-label="Back">
              <ArrowLeft className="w-5 h-5" />
            </button>
            <h1 className="text-2xl font-semibold">{isNew ? 'New rule set' : name || 'Rule set'}</h1>
            {meta && <span className="text-sm text-gray-500">showing v{meta.version}{meta.version !== meta.current_version ? ` (current is v${meta.current_version})` : ''}</span>}
          </div>
          <Button onClick={save} disabled={saving || (!dirty && !isNew) || name.trim() === ''}>
            {saving ? 'Saving…' : isNew ? 'Create' : 'Save new version'}
          </Button>
        </div>

        {meta && meta.version !== meta.current_version && (
          <div className="bg-amber-50 border border-amber-200 rounded-lg p-3 text-sm">
            You are viewing an older version. Saving creates a new version with this content.
          </div>
        )}

        <div className="bg-white rounded-lg border p-4 space-y-3">
          <div className="grid grid-cols-3 gap-3 items-center">
            <label className="text-sm font-medium">Name</label>
            <Input className="col-span-2" value={name} onChange={(e) => { setName(e.target.value); touch(); }} />
            <label className="text-sm font-medium">Description</label>
            <Input className="col-span-2" value={description} onChange={(e) => { setDescription(e.target.value); touch(); }} />
            <label className="text-sm font-medium" title="Pairs whose confidence-adjusted match score is below this are not recommended">Minimum match score (0-100)</label>
            <Input className="w-28" type="number" placeholder="none" value={minScore} onChange={(e) => { setMinScore(e.target.value); touch(); }} />
            <label className="text-sm font-medium" title="Scores resting on little information are pulled towards this neutral value">Neutral baseline (0-100)</label>
            <Input className="w-28" type="number" placeholder="50" value={priorScore} onChange={(e) => { setPriorScore(e.target.value); touch(); }} />
          </div>

          <div>
            <div className="flex items-center justify-between">
              <span className="text-sm font-medium">Dimension weights</span>
              <button className="text-sm text-blue-600 hover:underline" onClick={() => { setGroupWeights((g) => [...g, { group: '', weight: '1' }]); touch(); }}>
                + add
              </button>
            </div>
            <p className="text-xs text-gray-500">How much each dimension counts in the overall score (1 = normal, 0 = ignore, 3 = triple). A rule belongs to the dimension named in its group.</p>
            {groupWeights.map((g, i) => (
              <div key={i} className="flex items-center gap-2 mt-1">
                <select className={sel} value={g.group} onChange={(e) => { setGroupWeights((gs) => gs.map((x, j) => (j === i ? { ...x, group: e.target.value } : x))); touch(); }}>
                  <option value="">choose a dimension…</option>
                  {dims.map((d) => <option key={d.key} value={d.key}>{d.label}</option>)}
                  {g.group !== '' && !dims.some((d) => d.key === g.group) && <option value={g.group}>{g.group} (custom)</option>}
                </select>
                <span className="text-sm">×</span>
                <Input className="w-24" type="number" step="0.1" value={g.weight} onChange={(e) => { setGroupWeights((gs) => gs.map((x, j) => (j === i ? { ...x, weight: e.target.value } : x))); touch(); }} />
                <button className="p-1 rounded hover:bg-gray-100" aria-label="Remove group weight" onClick={() => { setGroupWeights((gs) => gs.filter((_, j) => j !== i)); touch(); }}>
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
          <h2 className="font-medium">Rules ({rules.length})</h2>
          <Button variant="outline" size="sm" onClick={() => { setRules((rs) => { const r = newRule(rs, fields); setOpen((o) => ({ ...o, [rs.length]: true })); return [...rs, r]; }); touch(); }} disabled={usable.length === 0}>
            <Plus className="w-4 h-4 mr-1" /> Add rule
          </Button>
        </div>

        <div className="space-y-3">
          {rules.map((r, i) => {
            const ruleIssues = issues.filter((x) => x.rule_id === r.id);
            const expanded = open[i] ?? false;
            return (
              <div key={i} className={`bg-white rounded-lg border ${r.enabled ? '' : 'opacity-60'}`}>
                <div className="p-3 flex items-center gap-3">
                  <button aria-label={expanded ? 'Collapse' : 'Expand'} onClick={() => setOpen((o) => ({ ...o, [i]: !expanded }))}>
                    {expanded ? <ChevronDown className="w-4 h-4" /> : <ChevronRight className="w-4 h-4" />}
                  </button>
                  <div className="flex-1 min-w-0">
                    <div className="font-medium truncate">{r.description || <span className="text-gray-400">Untitled rule</span>}</div>
                    {!expanded && <div className="text-xs text-gray-500 truncate">{r.when ? `when ${describeExpr(r.when, byKey)} → ` : ''}{describeExpr(r.expr, byKey)}</div>}
                  </div>
                  <span className={`text-xs px-2 py-0.5 rounded ${r.kind === 'hard' ? 'bg-red-100 text-red-800' : 'bg-blue-100 text-blue-800'}`}>{r.kind === 'hard' ? 'must hold' : `soft ×${r.weight}`}</span>
                  {r.scope === 'directional' && <span className="text-xs px-2 py-0.5 rounded bg-purple-100 text-purple-800">both directions</span>}
                  {r.group && <span className="text-xs px-2 py-0.5 rounded bg-gray-100">{r.group}</span>}
                  {ruleIssues.length > 0 && <span className="text-xs px-2 py-0.5 rounded bg-red-600 text-white">{ruleIssues.length} issue{ruleIssues.length > 1 ? 's' : ''}</span>}
                  <label className="flex items-center gap-1 text-xs"><input type="checkbox" checked={r.enabled} onChange={(e) => updateRule(i, { enabled: e.target.checked })} /> on</label>
                </div>

                {expanded && (
                  <div className="border-t p-4 space-y-4">
                    {ruleIssues.length > 0 && (
                      <div className="bg-red-50 border border-red-200 rounded p-2 text-sm text-red-800">
                        {ruleIssues.map((x, k) => <div key={k}>{x.message}</div>)}
                      </div>
                    )}
                    <div className="grid grid-cols-4 gap-3 items-center">
                      <label className="text-sm font-medium">Description</label>
                      <Input className="col-span-3" value={r.description} onChange={(e) => updateRule(i, { description: e.target.value })} />
                      <label className="text-sm font-medium">Type</label>
                      <div className="col-span-3 flex flex-wrap items-center gap-3">
                        <select className={sel} value={r.kind} onChange={(e) => updateRule(i, { kind: e.target.value as 'hard' | 'soft' })}>
                          <option value="hard">Must hold (excludes the pair if false)</option>
                          <option value="soft">Soft (adds to the score)</option>
                        </select>
                        {r.kind === 'soft' && (
                          <label className="flex items-center gap-1 text-sm">weight <Input className="w-20" type="number" step="0.5" value={String(r.weight)} onChange={(e) => updateRule(i, { weight: Number(e.target.value) })} /></label>
                        )}
                      </div>
                      <label className="text-sm font-medium">Applies to</label>
                      <div className="col-span-3">
                        <select className={sel} value={r.scope} onChange={(e) => updateRule(i, { scope: e.target.value as 'pair' | 'directional' })}>
                          <option value="pair">The pair (checked once; use for symmetric rules)</option>
                          <option value="directional">Each person in turn (A→B and B→A)</option>
                        </select>
                      </div>
                      <label className="text-sm font-medium">Dimension / priority</label>
                      <div className="col-span-3 flex items-center gap-3">
                        <select className={sel} title="The dimension this rule contributes to" value={r.group ?? ''} onChange={(e) => updateRule(i, { group: e.target.value === '' ? null : e.target.value })}>
                          <option value="">no dimension (other)</option>
                          {dims.map((d) => <option key={d.key} value={d.key}>{d.label}</option>)}
                          {r.group && !dims.some((d) => d.key === r.group) && <option value={r.group}>{r.group} (maps to Other)</option>}
                        </select>
                        <label className="flex items-center gap-1 text-sm" title="Higher priority results are listed first">priority <Input className="w-20" type="number" value={String(r.priority)} onChange={(e) => updateRule(i, { priority: Number(e.target.value) })} /></label>
                        <label className="flex items-center gap-1 text-sm">id <Input className="w-36" value={r.id} onChange={(e) => updateRule(i, { id: e.target.value })} /></label>
                      </div>
                    </div>

                    <div>
                      <div className="flex items-center gap-3 mb-1">
                        <span className="text-sm font-medium">Applies only when</span>
                        {r.when ? (
                          <button className="text-sm text-blue-600 hover:underline" onClick={() => updateRule(i, { when: null })}>always apply instead</button>
                        ) : (
                          <button className="text-sm text-blue-600 hover:underline" onClick={() => updateRule(i, { when: newNode('cmp', fields) })}>+ add a condition</button>
                        )}
                      </div>
                      {r.when ? (
                        <ExprEditor expr={r.when} fields={fields} byKey={byKey} onChange={(when) => updateRule(i, { when })} />
                      ) : (
                        <p className="text-xs text-gray-500">Always applies. If a condition is set and false, the rule is skipped for that pair.</p>
                      )}
                    </div>

                    <div>
                      <div className="text-sm font-medium mb-1">Rule: this must hold</div>
                      <ExprEditor expr={r.expr} fields={fields} byKey={byKey} onChange={(expr) => updateRule(i, { expr })} />
                    </div>

                    <div className="flex justify-end">
                      <Button variant="ghost" size="sm" onClick={() => { setRules((rs) => rs.filter((_, j) => j !== i)); touch(); }}>
                        <Trash2 className="w-4 h-4 mr-1" /> Delete rule
                      </Button>
                    </div>
                  </div>
                )}
              </div>
            );
          })}
          {rules.length === 0 && <div className="bg-white rounded-lg border p-6 text-center text-sm text-gray-500">No rules yet.</div>}
        </div>

        {(dirty || isNew) && (
          <div className="bg-white rounded-lg border p-4">
            <label className="text-sm font-medium">What changed? (optional, kept in the version history)</label>
            <Input className="mt-1" value={note} onChange={(e) => setNote(e.target.value)} />
          </div>
        )}

        {meta && (
          <>
            <div className="bg-white rounded-lg border p-4 space-y-3">
              <h2 className="font-medium">Test on two people</h2>
              <div className="flex flex-wrap items-center gap-2">
                <select className={sel} value={pa} onChange={(e) => { setPa(e.target.value); setEvalResult(null); }}>
                  <option value="">Person A…</option>
                  {profiles.map((p) => <option key={p.id} value={p.id}>{p.name || 'Unnamed profile'}</option>)}
                </select>
                <select className={sel} value={pb} onChange={(e) => { setPb(e.target.value); setEvalResult(null); }}>
                  <option value="">Person B…</option>
                  {profiles.filter((p) => p.id !== pa).map((p) => <option key={p.id} value={p.id}>{p.name || 'Unnamed profile'}</option>)}
                </select>
                <Button variant="outline" size="sm" onClick={evaluate} disabled={!pa || !pb || dirty}>Evaluate</Button>
                {dirty && <span className="text-xs text-gray-500">Save first: the test runs the saved version.</span>}
              </div>

              {evalResult && (
                <div className="text-sm space-y-3">
                  <div className="flex flex-wrap items-center gap-3">
                    <span className={`px-2 py-0.5 rounded ${evalResult.eligible ? 'bg-green-100 text-green-800' : 'bg-red-100 text-red-800'}`}>{evalResult.eligible ? 'Eligible' : 'Excluded'}</span>
                    <span>Score: {evalResult.score != null ? `${Math.round(evalResult.score)}/100` : 'n/a'}</span>
                    <span className="text-gray-600">Based on {evalResult.coverage != null ? `${Math.round(evalResult.coverage * 100)}%` : '0%'} of the available checks</span>
                    {evalResult.meets_threshold === false && <span className="text-red-700">below the minimum score</span>}
                    {evalResult.needs_info && <span className="text-amber-700">some must-haves cannot be decided: information missing</span>}
                  </div>
                  <RuleResultList title={`Rules: ${nameOf(pa)} & ${nameOf(pb)}`} results={evalResult.rules.results} />
                  <RuleResultList title={`${nameOf(pa)}'s preferences about ${nameOf(pb)}`} results={evalResult.a_preferences.results} />
                  <RuleResultList title={`${nameOf(pb)}'s preferences about ${nameOf(pa)}`} results={evalResult.b_preferences.results} />
                </div>
              )}
            </div>

            <div className="bg-white rounded-lg border p-4">
              <h2 className="font-medium mb-2">Version history</h2>
              <ul className="space-y-1 text-sm">
                {meta.versions.map((v) => (
                  <li key={v.version} className="flex items-center gap-3">
                    <span className="w-10 font-medium">v{v.version}</span>
                    <span className="text-gray-500 w-44">{new Date(v.created_at).toLocaleString()}</span>
                    <span className="flex-1 truncate">{v.note || ''}</span>
                    {v.version !== meta.version && <button className="text-blue-600 hover:underline" onClick={() => load(v.version)}>View</button>}
                    {v.version === meta.version && <span className="text-xs text-gray-500">shown</span>}
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
  return (
    <Suspense fallback={<div className="p-6 text-gray-500">Loading…</div>}>
      <Editor />
    </Suspense>
  );
}
