'use client';

import React, { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import {
  ACTION_KEY,
  INTEREST_KEY,
  NEGATIVE,
  OUTCOMES,
  OUTCOME_KEY,
  PIPELINE,
  STATUS_CHIP,
  STATUS_KEY,
  describeEvent,
  eventText,
} from '@/lib/workflow';
import { useI18n } from '@/i18n';
import { DimensionDef, Interest, IntroPreview, MatchDetail, MatchStatus, Outcome } from '@/types/matchmaking';

const sel = 'border rounded-md px-2 py-1.5 text-sm bg-white';

interface Props {
  detail: MatchDetail;
  dims: DimensionDef[];
  onChange: (d: MatchDetail) => void;
}

/** Everything a matchmaker does to a tracked match: move it along, record answers and outcomes, hold, annotate, re-score. */
export function MatchWorkflowPanel({ detail: d, dims, onChange }: Props) {
  const { t, fmt, locale } = useI18n();
  const nameA = d.name_a || t('match.personA');
  const nameB = d.name_b || t('match.personB');

  const [pending, setPending] = useState<MatchStatus | null>(null);
  const [reason, setReason] = useState('');
  const [override, setOverride] = useState('');
  const [closeOutcome, setCloseOutcome] = useState<Outcome | ''>('');
  const [holdText, setHoldText] = useState('');
  const [noteText, setNoteText] = useState('');
  const [latestRules, setLatestRules] = useState(false);
  const [weightRows, setWeightRows] = useState<{ key: string; weight: string }[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [preview, setPreview] = useState<IntroPreview | null>(null);
  const [holdSide, setHoldSide] = useState<'a' | 'b'>('a');

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

  // Proposing an introduction sends messages: show exactly what, and to whom, before the matchmaker confirms.
  useEffect(() => {
    if (pending !== 'introduction_proposed') {
      setPreview(null);
      return;
    }
    invoke<IntroPreview>('tg_preview_introduction', { matchId: d.id }).then(setPreview).catch(() => setPreview(null));
  }, [pending, d.id]);

  const needsOverride = pending !== null && ['recommended', 'approved'].includes(pending) && !d.eligible;
  const needsOutcome = pending === 'closed' && !d.outcome;

  const confirm = () => {
    if (!pending) return;
    if (needsOverride && override.trim().length < 3) {
      toast.error(t('matchPanel.overrideRequired'));
      return;
    }
    if (needsOutcome && !closeOutcome) {
      toast.error(t('matchPanel.outcomeRequired'));
      return;
    }
    void run(
      () =>
        invoke<MatchDetail>('mm_transition_match', {
          id: d.id,
          locale,
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
    run(() => invoke<MatchDetail>('mm_set_response', { id: d.id, locale, side, response }), apply);

  const rescore = (weights: Record<string, number> | null) =>
    run(() => invoke<MatchDetail>('mm_rescore_match', { id: d.id, locale, useLatestRuleSet: latestRules, weights }), (r) => {
      setWeightRows(null);
      apply(r);
      toast.success(t('matchPanel.rescored'));
    });

  const saveWeights = () => {
    const out: Record<string, number> = {};
    for (const r of weightRows ?? []) {
      if (!r.key) continue;
      const w = Number(r.weight);
      if (Number.isNaN(w)) {
        toast.error(t('matchPanel.weightsInvalid'));
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
          <span className={`px-2.5 py-1 rounded text-sm font-medium ${STATUS_CHIP[d.status]}`}>{t(STATUS_KEY[d.status])}</span>
          {d.outcome && <span className="text-sm text-gray-600">{t('matchPanel.outcomeLabel')} <b>{t(OUTCOME_KEY[d.outcome])}</b></span>}
          {d.hidden && <span className="text-xs px-2 py-0.5 rounded bg-gray-200">{t('matches.hiddenChip')}</span>}
          <div className="flex-1" />
          <span className="text-xs text-gray-500"><bdi>{t('matchPanel.rules', { name: d.rule_set_name ?? t('matchPanel.ruleSetFallback'), version: fmt.number(d.rule_set_version) })}</bdi></span>
        </div>

        {!terminal && (
          <ol className="flex flex-wrap gap-1 text-xs">
            {PIPELINE.map((s, i) => (
              <li key={s} className={`px-2 py-1 rounded ${i < stage ? 'bg-green-100 text-green-800' : i === stage ? 'bg-blue-600 text-white' : 'bg-gray-100 text-gray-500'}`}>
                {t(STATUS_KEY[s])}
              </li>
            ))}
          </ol>
        )}

        {d.override_reason && (
          <div className="text-sm bg-amber-50 border border-amber-200 rounded p-2">
            {t('matchPanel.overrideBanner', { reason: d.override_reason })}
          </div>
        )}
        {!d.eligible && !terminal && (
          <div className="text-sm bg-red-50 border border-red-200 rounded p-2 text-red-800">
            {t('matchPanel.excludedBanner')}
          </div>
        )}
        {d.hold_reason && (
          <div className="text-sm bg-amber-50 border border-amber-200 rounded p-2 flex items-start gap-3">
            <div className="flex-1"><b>{t('matchPanel.waiting')}</b> <bdi>{d.hold_reason}</bdi></div>
            <Button size="sm" variant="outline" disabled={busy} onClick={() => run(() => invoke<MatchDetail>('mm_set_match_hold', { id: d.id, locale, reason: null }), apply)}>{t('matchPanel.clear')}</Button>
          </div>
        )}

        {/* actions */}
        {d.allowed_next.length > 0 && (
          <div className="flex flex-wrap gap-2">
            {d.allowed_next.map((to) => (
              <Button key={to} size="sm" variant={NEGATIVE.includes(to) ? 'outline' : 'default'} disabled={busy} onClick={() => startTransition(to)}>
                {d.status === 'rejected' && to === 'reviewed' ? t('matchPanel.reconsider') : t(ACTION_KEY[to])}
              </Button>
            ))}
          </div>
        )}
        {terminal && d.allowed_next.length === 0 && <p className="text-sm text-gray-500">{t('matchPanel.finished')}</p>}

        {pending && (
          <div className="border rounded-md p-3 bg-gray-50 space-y-2">
            <div className="font-medium text-sm">{t(ACTION_KEY[pending])}</div>
            {needsOverride && (
              <div>
                <label className="text-xs font-medium text-red-800">{t('matchPanel.overrideLabel')}</label>
                <Input className="mt-1" dir="auto" value={override} onChange={(e) => setOverride(e.target.value)} placeholder={t('matchPanel.overridePlaceholder')} />
              </div>
            )}
            {needsOutcome && (
              <div>
                <label className="text-xs font-medium">{t('matchPanel.outcomeClose')}</label>
                <select className={`${sel} ms-2`} value={closeOutcome} onChange={(e) => setCloseOutcome(e.target.value as Outcome)}>
                  <option value="">{t('matchPanel.choose')}</option>
                  {OUTCOMES.map((o) => <option key={o} value={o}>{t(OUTCOME_KEY[o])}</option>)}
                </select>
              </div>
            )}
            {pending === 'introduction_proposed' && preview && (
              <div className="border rounded-md p-2 bg-white space-y-2 text-sm">
                <div className="font-medium">{t('matchPanel.introTitle')}</div>
                {([['a', nameA, preview.a], ['b', nameB, preview.b]] as const).map(([k, name, side]) => (
                  <div key={k}>
                    <div className="text-xs text-gray-500">{t('matchPanel.introTo', { name })}</div>
                    {side.reachable && side.message ? (
                      <pre dir="auto" className="whitespace-pre-wrap text-xs bg-gray-50 border rounded p-2 font-sans">{side.message}</pre>
                    ) : (
                      <div className="text-xs text-amber-800 bg-amber-50 border border-amber-200 rounded p-2">
                        {t('matchPanel.unreachable', { name })}
                      </div>
                    )}
                  </div>
                ))}
                <p className="text-xs text-gray-500">{t('matchPanel.introHint')}</p>
              </div>
            )}
            <div>
              <label className="text-xs font-medium">{t('matchPanel.noteLabel')}</label>
              <Input className="mt-1" dir="auto" value={reason} onChange={(e) => setReason(e.target.value)} placeholder={NEGATIVE.includes(pending) ? t('matchPanel.whyNegative') : t('matchPanel.whyPositive')} />
            </div>
            <div className="flex gap-2">
              <Button size="sm" onClick={confirm} disabled={busy}>{t('matchPanel.confirm')}</Button>
              <Button size="sm" variant="ghost" onClick={() => setPending(null)}>{t('common.cancel')}</Button>
            </div>
          </div>
        )}
      </div>

      {/* responses and outcome */}
      {(d.can_record_responses || d.can_record_outcome) && (
        <div className="bg-white rounded-lg border p-4 space-y-3">
          {d.can_record_responses && (
            <div>
              <div className="font-medium text-sm mb-1">{t('matchPanel.interestedTitle')}</div>
              <p className="text-xs text-gray-500 mb-2">{t('matchPanel.interestedHint')}</p>
              {([['a', nameA, d.a_response], ['b', nameB, d.b_response]] as const).map(([side, name, value]) => (
                <div key={side} className="flex items-center gap-3 py-1">
                  <span className="w-40 text-sm"><bdi>{name}</bdi></span>
                  {(['interested', 'not_interested', 'unknown'] as Interest[]).map((r) => (
                    <button
                      key={r}
                      disabled={busy}
                      onClick={() => value !== r && setResponse(side, r)}
                      className={`px-2.5 py-1 rounded text-xs border ${value === r ? 'bg-blue-600 text-white border-blue-600' : 'bg-white hover:bg-gray-50'}`}
                    >
                      {t(INTEREST_KEY[r])}
                    </button>
                  ))}
                </div>
              ))}
            </div>
          )}
          {d.can_record_outcome && (
            <div className="flex items-center gap-2">
              <span className="text-sm font-medium">{t('matchPanel.whatHappened')}</span>
              <select className={sel} value={d.outcome ?? ''} onChange={(e) => e.target.value && run(() => invoke<MatchDetail>('mm_record_outcome', { id: d.id, locale, outcome: e.target.value }), apply)}>
                <option value="">{t('matchPanel.recordOutcome')}</option>
                {OUTCOMES.map((o) => <option key={o} value={o}>{t(OUTCOME_KEY[o])}</option>)}
              </select>
              <span className="text-xs text-gray-500">{t('matchPanel.outcomesHint')}</span>
            </div>
          )}
        </div>
      )}

      {/* information request, hide, rescore */}
      <div className="bg-white rounded-lg border p-4 space-y-3">
        {!d.hold_reason && !d.status.match(/^(closed|rejected|declined|stopped)$/) && (
          <div className="flex items-center gap-2">
            <select className={sel} value={holdSide} onChange={(e) => setHoldSide(e.target.value as 'a' | 'b')} aria-label={t('matchPanel.askWhom')}>
              <option value="a">{nameA}</option>
              <option value="b">{nameB}</option>
            </select>
            <Input className="flex-1" dir="auto" placeholder={t('matchPanel.requestPlaceholder')} value={holdText} onChange={(e) => setHoldText(e.target.value)} />
            <Button
              size="sm"
              variant="outline"
              disabled={busy || holdText.trim() === ''}
              title={(holdSide === 'a' ? d.reachable_a : d.reachable_b) ? t('matchPanel.sentTelegram') : t('matchPanel.notTelegram')}
              onClick={() =>
                run(
                  async () => {
                    if (holdSide === 'a' ? d.reachable_a : d.reachable_b) {
                      await invoke('tg_request_info', { matchId: d.id, side: holdSide, text: holdText });
                      return invoke<MatchDetail>('mm_get_match', { id: d.id, locale });
                    }
                    return invoke<MatchDetail>('mm_set_match_hold', { id: d.id, locale, reason: holdText });
                  },
                  (r) => {
                    setHoldText('');
                    apply(r);
                  }
                )
              }
            >
              {(holdSide === 'a' ? d.reachable_a : d.reachable_b) ? t('matchPanel.askTelegram') : t('matchPanel.recordRequest')}
            </Button>
          </div>
        )}
        <div className="flex flex-wrap items-center gap-3">
          {!terminal && (
            <>
              <Button size="sm" variant="outline" disabled={busy} onClick={() => rescore(null)}>{t('matchPanel.rescoreNow')}</Button>
              <label className="flex items-center gap-1 text-xs"><input type="checkbox" checked={latestRules} onChange={(e) => setLatestRules(e.target.checked)} /> {t('matchPanel.newestRules')}</label>
              <Button size="sm" variant="outline" onClick={() => setWeightRows(Object.entries(d.weight_overrides).map(([key, w]) => ({ key, weight: String(w) })))}>
                {t('matchPanel.adjustWeights')}
              </Button>
            </>
          )}
          <div className="flex-1" />
          <Button size="sm" variant="ghost" disabled={busy} onClick={() => run(() => invoke<MatchDetail>('mm_set_match_hidden', { id: d.id, locale, hidden: !d.hidden }), apply)}>
            {d.hidden ? t('matchPanel.showInLists') : t('matchPanel.hideFromLists')}
          </Button>
        </div>
        <p className="text-xs text-gray-500">
          {t('matchPanel.scoreFrom', { date: d.scorecard_at ? fmt.dateTime(d.scorecard_at) : t('matchPanel.scoreNA'), trigger: d.scorecard_trigger === 'rescore' ? t('matchPanel.scoreRescore') : d.scorecard_trigger === 'weights_adjusted' ? t('matchPanel.scoreWeights') : t('matchPanel.scoreCreated'), count: fmt.number(d.snapshot_count) })}
        </p>

        {weightRows && (
          <div className="border rounded-md p-3 bg-gray-50 space-y-2">
            <div className="text-sm font-medium">{t('matchPanel.weightsTitle')}</div>
            <p className="text-xs text-gray-500">{t('matchPanel.weightsHint')}</p>
            {weightRows.map((r, i) => (
              <div key={i} className="flex items-center gap-2">
                <select className={sel} value={r.key} onChange={(e) => setWeightRows((rs) => rs!.map((x, j) => (j === i ? { ...x, key: e.target.value } : x)))}>
                  <option value="">{t('matchPanel.chooseDimension')}</option>
                  {dims.map((x) => <option key={x.key} value={x.key}>{x.label}</option>)}
                </select>
                <span className="text-sm">×</span>
                <Input className="w-20" dir="ltr" type="number" step="0.5" value={r.weight} onChange={(e) => setWeightRows((rs) => rs!.map((x, j) => (j === i ? { ...x, weight: e.target.value } : x)))} />
                <button className="text-sm text-gray-500 hover:text-red-600" onClick={() => setWeightRows((rs) => rs!.filter((_, j) => j !== i))}>{t('matchPanel.remove')}</button>
              </div>
            ))}
            <div className="flex gap-2">
              <Button size="sm" variant="outline" onClick={() => setWeightRows((rs) => [...rs!, { key: '', weight: '1' }])}>{t('matchPanel.addDimension')}</Button>
              <Button size="sm" onClick={saveWeights} disabled={busy}>{t('matchPanel.saveRescore')}</Button>
              <Button size="sm" variant="ghost" onClick={() => setWeightRows(null)}>{t('common.cancel')}</Button>
            </div>
          </div>
        )}
      </div>

      {/* notes */}
      <div className="bg-white rounded-lg border p-4 space-y-2">
        <div className="font-medium text-sm">{t('matchPanel.notes')}</div>
        <div className="flex gap-2">
          <Input className="flex-1" dir="auto" placeholder={t('matchPanel.addNote')} value={noteText} onChange={(e) => setNoteText(e.target.value)} />
          <Button size="sm" disabled={busy || noteText.trim() === ''} onClick={() => run(() => invoke<MatchDetail>('mm_add_match_note', { id: d.id, locale, text: noteText }), (r) => { setNoteText(''); apply(r); })}>{t('matchPanel.add')}</Button>
        </div>
        {d.notes.length === 0 && <p className="text-sm text-gray-500">{t('matchPanel.noNotes')}</p>}
        <ul className="space-y-2">
          {d.notes.map((n) => (
            <li key={n.id} className="text-sm border-s-2 ps-3">
              <div dir="auto">{n.text}</div>
              <div className="text-xs text-gray-400">{fmt.dateTime(n.created_at)}</div>
            </li>
          ))}
        </ul>
      </div>

      {/* history */}
      <div className="bg-white rounded-lg border p-4">
        <div className="font-medium text-sm mb-2">{t('matchPanel.history')}</div>
        <ul className="space-y-2">
          {d.events.map((e, i) => (
            <li key={i} className="text-sm flex gap-3">
              <span className="w-40 shrink-0 text-xs text-gray-400">{fmt.dateTime(e.at)}</span>
              <div>
                <div>{describeEvent(e, t)}</div>
                {eventText(e) && <div className="text-xs text-gray-600 whitespace-pre-line">{eventText(e)}</div>}
              </div>
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
}
