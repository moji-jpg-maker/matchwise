'use client';

import React, { useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import {
  ACTION_LABEL,
  INTEREST_LABEL,
  NEGATIVE,
  OUTCOMES,
  OUTCOME_LABEL,
  PIPELINE,
  STATUS_CHIP,
  STATUS_LABEL,
  describeEvent,
  eventText,
} from '@/lib/workflow';
import { DimensionDef, Interest, MatchDetail, MatchStatus, Outcome } from '@/types/matchmaking';

const sel = 'border rounded-md px-2 py-1.5 text-sm bg-white';

interface Props {
  detail: MatchDetail;
  dims: DimensionDef[];
  onChange: (d: MatchDetail) => void;
}

/** Everything a matchmaker does to a tracked match: move it along, record answers and outcomes, hold, annotate, re-score. */
export function MatchWorkflowPanel({ detail: d, dims, onChange }: Props) {
  const nameA = d.name_a || 'Person A';
  const nameB = d.name_b || 'Person B';

  const [pending, setPending] = useState<MatchStatus | null>(null);
  const [reason, setReason] = useState('');
  const [override, setOverride] = useState('');
  const [closeOutcome, setCloseOutcome] = useState<Outcome | ''>('');
  const [holdText, setHoldText] = useState('');
  const [noteText, setNoteText] = useState('');
  const [latestRules, setLatestRules] = useState(false);
  const [weightRows, setWeightRows] = useState<{ key: string; weight: string }[] | null>(null);
  const [busy, setBusy] = useState(false);

  const run = async <T,>(fn: () => Promise<T>, after?: (r: T) => void) => {
    setBusy(true);
    try {
      const r = await fn();
      after?.(r);
    } catch (e) {
      toast.error(`${e}`);
    } finally {
      setBusy(false);
    }
  };
  const apply = (next: MatchDetail) => onChange(next);

  const startTransition = (to: MatchStatus) => {
    setPending(to);
    setReason('');
    setOverride('');
    setCloseOutcome(d.outcome ?? '');
  };

  const needsOverride = pending !== null && ['recommended', 'approved'].includes(pending) && !d.eligible;
  const needsOutcome = pending === 'closed' && !d.outcome;

  const confirm = () => {
    if (!pending) return;
    if (needsOverride && override.trim().length < 3) {
      toast.error('Write a short override reason: the rules exclude this pair.');
      return;
    }
    if (needsOutcome && !closeOutcome) {
      toast.error('Choose an outcome before closing.');
      return;
    }
    void run(
      () =>
        invoke<MatchDetail>('mm_transition_match', {
          id: d.id,
          toStatus: pending,
          reason: reason.trim() || null,
          overrideReason: needsOverride ? override.trim() : null,
          outcome: closeOutcome || null,
        }),
      (r) => {
        setPending(null);
        apply(r);
      }
    );
  };

  const setResponse = (side: 'a' | 'b', response: Interest) =>
    run(() => invoke<MatchDetail>('mm_set_response', { id: d.id, side, response }), apply);

  const rescore = (weights: Record<string, number> | null) =>
    run(() => invoke<MatchDetail>('mm_rescore_match', { id: d.id, useLatestRuleSet: latestRules, weights }), (r) => {
      setWeightRows(null);
      apply(r);
      toast.success('Re-scored; the previous score is kept in the history');
    });

  const saveWeights = () => {
    const out: Record<string, number> = {};
    for (const r of weightRows ?? []) {
      if (!r.key) continue;
      const w = Number(r.weight);
      if (Number.isNaN(w)) {
        toast.error('Weights must be numbers between 0 and 10');
        return;
      }
      out[r.key] = w;
    }
    void rescore(out);
  };

  const stage = PIPELINE.indexOf(d.status);
  const terminal = stage === -1;

  return (
    <div className="space-y-4">
      {/* status */}
      <div className="bg-white rounded-lg border p-4 space-y-3">
        <div className="flex flex-wrap items-center gap-3">
          <span className={`px-2.5 py-1 rounded text-sm font-medium ${STATUS_CHIP[d.status]}`}>{STATUS_LABEL[d.status]}</span>
          {d.outcome && <span className="text-sm text-gray-600">Outcome: <b>{OUTCOME_LABEL[d.outcome]}</b></span>}
          {d.hidden && <span className="text-xs px-2 py-0.5 rounded bg-gray-200">hidden</span>}
          <div className="flex-1" />
          <span className="text-xs text-gray-500">Rules: {d.rule_set_name ?? 'rule set'} v{d.rule_set_version}</span>
        </div>

        {!terminal && (
          <ol className="flex flex-wrap gap-1 text-xs">
            {PIPELINE.map((s, i) => (
              <li key={s} className={`px-2 py-1 rounded ${i < stage ? 'bg-green-100 text-green-800' : i === stage ? 'bg-blue-600 text-white' : 'bg-gray-100 text-gray-500'}`}>
                {STATUS_LABEL[s]}
              </li>
            ))}
          </ol>
        )}

        {d.override_reason && (
          <div className="text-sm bg-amber-50 border border-amber-200 rounded p-2">
            Advanced despite the rules. Override reason: {d.override_reason}
          </div>
        )}
        {!d.eligible && !terminal && (
          <div className="text-sm bg-red-50 border border-red-200 rounded p-2 text-red-800">
            The rules currently exclude this pair. Recommending or approving it needs an override reason, which is kept on record.
          </div>
        )}
        {d.hold_reason && (
          <div className="text-sm bg-amber-50 border border-amber-200 rounded p-2 flex items-start gap-3">
            <div className="flex-1"><b>Waiting for information:</b> {d.hold_reason}</div>
            <Button size="sm" variant="outline" disabled={busy} onClick={() => run(() => invoke<MatchDetail>('mm_set_match_hold', { id: d.id, reason: null }), apply)}>Clear</Button>
          </div>
        )}

        {/* actions */}
        {d.allowed_next.length > 0 && (
          <div className="flex flex-wrap gap-2">
            {d.allowed_next.map((to) => (
              <Button key={to} size="sm" variant={NEGATIVE.includes(to) ? 'outline' : 'default'} disabled={busy} onClick={() => startTransition(to)}>
                {d.status === 'rejected' && to === 'reviewed' ? 'Reconsider' : ACTION_LABEL[to]}
              </Button>
            ))}
          </div>
        )}
        {terminal && d.allowed_next.length === 0 && <p className="text-sm text-gray-500">This match is finished.</p>}

        {pending && (
          <div className="border rounded-md p-3 bg-gray-50 space-y-2">
            <div className="font-medium text-sm">{ACTION_LABEL[pending]}</div>
            {needsOverride && (
              <div>
                <label className="text-xs font-medium text-red-800">Override reason (required: the rules exclude this pair)</label>
                <Input className="mt-1" value={override} onChange={(e) => setOverride(e.target.value)} placeholder="Why go ahead anyway?" />
              </div>
            )}
            {needsOutcome && (
              <div>
                <label className="text-xs font-medium">Outcome (required to close)</label>
                <select className={`${sel} ml-2`} value={closeOutcome} onChange={(e) => setCloseOutcome(e.target.value as Outcome)}>
                  <option value="">choose…</option>
                  {OUTCOMES.map((o) => <option key={o} value={o}>{OUTCOME_LABEL[o]}</option>)}
                </select>
              </div>
            )}
            <div>
              <label className="text-xs font-medium">Note for the record (optional)</label>
              <Input className="mt-1" value={reason} onChange={(e) => setReason(e.target.value)} placeholder={NEGATIVE.includes(pending) ? 'Why?' : 'Anything worth remembering'} />
            </div>
            <div className="flex gap-2">
              <Button size="sm" onClick={confirm} disabled={busy}>Confirm</Button>
              <Button size="sm" variant="ghost" onClick={() => setPending(null)}>Cancel</Button>
            </div>
          </div>
        )}
      </div>

      {/* responses and outcome */}
      {(d.can_record_responses || d.can_record_outcome) && (
        <div className="bg-white rounded-lg border p-4 space-y-3">
          {d.can_record_responses && (
            <div>
              <div className="font-medium text-sm mb-1">Are they interested in meeting?</div>
              <p className="text-xs text-gray-500 mb-2">Two “interested” answers move the match on; one “not interested” ends it.</p>
              {([['a', nameA, d.a_response], ['b', nameB, d.b_response]] as const).map(([side, name, value]) => (
                <div key={side} className="flex items-center gap-3 py-1">
                  <span className="w-40 text-sm">{name}</span>
                  {(['interested', 'not_interested', 'unknown'] as Interest[]).map((r) => (
                    <button
                      key={r}
                      disabled={busy}
                      onClick={() => value !== r && setResponse(side, r)}
                      className={`px-2.5 py-1 rounded text-xs border ${value === r ? 'bg-blue-600 text-white border-blue-600' : 'bg-white hover:bg-gray-50'}`}
                    >
                      {INTEREST_LABEL[r]}
                    </button>
                  ))}
                </div>
              ))}
            </div>
          )}
          {d.can_record_outcome && (
            <div className="flex items-center gap-2">
              <span className="text-sm font-medium">What happened?</span>
              <select className={sel} value={d.outcome ?? ''} onChange={(e) => e.target.value && run(() => invoke<MatchDetail>('mm_record_outcome', { id: d.id, outcome: e.target.value }), apply)}>
                <option value="">record an outcome…</option>
                {OUTCOMES.map((o) => <option key={o} value={o}>{OUTCOME_LABEL[o]}</option>)}
              </select>
              <span className="text-xs text-gray-500">Outcomes are the labels later learning uses.</span>
            </div>
          )}
        </div>
      )}

      {/* information request, hide, rescore */}
      <div className="bg-white rounded-lg border p-4 space-y-3">
        {!d.hold_reason && !d.status.match(/^(closed|rejected|declined|stopped)$/) && (
          <div className="flex items-center gap-2">
            <Input className="flex-1" placeholder="Request more information, e.g. “need her education level”" value={holdText} onChange={(e) => setHoldText(e.target.value)} />
            <Button size="sm" variant="outline" disabled={busy || holdText.trim() === ''} onClick={() => run(() => invoke<MatchDetail>('mm_set_match_hold', { id: d.id, reason: holdText }), (r) => { setHoldText(''); apply(r); })}>
              Request
            </Button>
          </div>
        )}
        <div className="flex flex-wrap items-center gap-3">
          {!terminal && (
            <>
              <Button size="sm" variant="outline" disabled={busy} onClick={() => rescore(null)}>Re-score now</Button>
              <label className="flex items-center gap-1 text-xs"><input type="checkbox" checked={latestRules} onChange={(e) => setLatestRules(e.target.checked)} /> use the newest rule-set version</label>
              <Button size="sm" variant="outline" onClick={() => setWeightRows(Object.entries(d.weight_overrides).map(([key, w]) => ({ key, weight: String(w) })))}>
                Adjust weights
              </Button>
            </>
          )}
          <div className="flex-1" />
          <Button size="sm" variant="ghost" disabled={busy} onClick={() => run(() => invoke<MatchDetail>('mm_set_match_hidden', { id: d.id, hidden: !d.hidden }), apply)}>
            {d.hidden ? 'Show in lists' : 'Hide from lists'}
          </Button>
        </div>
        <p className="text-xs text-gray-500">
          Score from {d.scorecard_at ? new Date(d.scorecard_at).toLocaleString() : 'n/a'} ({d.scorecard_trigger ?? 'created'}). Each re-score adds a snapshot; {d.snapshot_count} kept.
        </p>

        {weightRows && (
          <div className="border rounded-md p-3 bg-gray-50 space-y-2">
            <div className="text-sm font-medium">Dimension weights for this match only</div>
            <p className="text-xs text-gray-500">1 = as in the rule set, 0 = ignore the dimension, 3 = triple. The rule set itself is not changed.</p>
            {weightRows.map((r, i) => (
              <div key={i} className="flex items-center gap-2">
                <select className={sel} value={r.key} onChange={(e) => setWeightRows((rs) => rs!.map((x, j) => (j === i ? { ...x, key: e.target.value } : x)))}>
                  <option value="">choose a dimension…</option>
                  {dims.map((x) => <option key={x.key} value={x.key}>{x.label}</option>)}
                </select>
                <span className="text-sm">×</span>
                <Input className="w-20" type="number" step="0.5" value={r.weight} onChange={(e) => setWeightRows((rs) => rs!.map((x, j) => (j === i ? { ...x, weight: e.target.value } : x)))} />
                <button className="text-sm text-gray-500 hover:text-red-600" onClick={() => setWeightRows((rs) => rs!.filter((_, j) => j !== i))}>remove</button>
              </div>
            ))}
            <div className="flex gap-2">
              <Button size="sm" variant="outline" onClick={() => setWeightRows((rs) => [...rs!, { key: '', weight: '1' }])}>+ dimension</Button>
              <Button size="sm" onClick={saveWeights} disabled={busy}>Save and re-score</Button>
              <Button size="sm" variant="ghost" onClick={() => setWeightRows(null)}>Cancel</Button>
            </div>
          </div>
        )}
      </div>

      {/* notes */}
      <div className="bg-white rounded-lg border p-4 space-y-2">
        <div className="font-medium text-sm">Notes</div>
        <div className="flex gap-2">
          <Input className="flex-1" placeholder="Add a note" value={noteText} onChange={(e) => setNoteText(e.target.value)} />
          <Button size="sm" disabled={busy || noteText.trim() === ''} onClick={() => run(() => invoke<MatchDetail>('mm_add_match_note', { id: d.id, text: noteText }), (r) => { setNoteText(''); apply(r); })}>Add</Button>
        </div>
        {d.notes.length === 0 && <p className="text-sm text-gray-500">No notes yet.</p>}
        <ul className="space-y-2">
          {d.notes.map((n) => (
            <li key={n.id} className="text-sm border-l-2 pl-3">
              <div>{n.text}</div>
              <div className="text-xs text-gray-400">{new Date(n.created_at).toLocaleString()}</div>
            </li>
          ))}
        </ul>
      </div>

      {/* history */}
      <div className="bg-white rounded-lg border p-4">
        <div className="font-medium text-sm mb-2">History</div>
        <ul className="space-y-2">
          {d.events.map((e, i) => (
            <li key={i} className="text-sm flex gap-3">
              <span className="w-40 shrink-0 text-xs text-gray-400">{new Date(e.at).toLocaleString()}</span>
              <div>
                <div>{describeEvent(e)}</div>
                {eventText(e) && <div className="text-xs text-gray-600 whitespace-pre-line">{eventText(e)}</div>}
              </div>
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
}
