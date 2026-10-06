'use client';

import React, { Suspense, useCallback, useEffect, useState } from 'react';
import { useRouter, useSearchParams } from 'next/navigation';
import { invoke } from '@tauri-apps/api/core';
import { ArrowLeft } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { ScoreCardView } from '@/components/matchmaking/ScoreCardView';
import { RuleResultList } from '@/components/matchmaking/RuleResultList';
import { MatchWorkflowPanel } from '@/components/matchmaking/MatchWorkflowPanel';
import { STATUS_CHIP, STATUS_LABEL } from '@/lib/workflow';
import { DimensionDef, MatchDetail, MatchView, ProfileView, RuleSetSummary } from '@/types/matchmaking';

const nameOf = (p: ProfileView, fallback: string) => (p.fields['full_name']?.value as string) || fallback;

function MatchPage() {
  const router = useRouter();
  const q = useSearchParams();
  const matchId = q.get('id');
  const a = q.get('a');
  const b = q.get('b');

  const [sets, setSets] = useState<RuleSetSummary[]>([]);
  const [setId, setSetId] = useState(q.get('set') ?? '');
  const [view, setView] = useState<MatchView | null>(null);
  const [detail, setDetail] = useState<MatchDetail | null>(null);
  const [dims, setDims] = useState<DimensionDef[]>([]);
  const [names, setNames] = useState<{ a: string; b: string }>({ a: 'Person A', b: 'Person B' });
  const [existingId, setExistingId] = useState<string | null>(null);
  const [showRules, setShowRules] = useState(false);
  const [creating, setCreating] = useState(false);

  // ---- tracked match (record mode)
  const loadDetail = useCallback(async () => {
    if (!matchId) return;
    try {
      const [d, catalog] = await Promise.all([
        invoke<MatchDetail>('mm_get_match', { id: matchId }),
        invoke<DimensionDef[]>('mm_dimension_catalog'),
      ]);
      setDetail(d);
      setDims(catalog);
    } catch (e) {
      toast.error(`${e}`);
    }
  }, [matchId]);

  useEffect(() => { void loadDetail(); }, [loadDetail]);

  // ---- ad-hoc comparison of two people
  useEffect(() => {
    if (matchId || !a || !b) return;
    (async () => {
      try {
        const [s, pa, pb, existing] = await Promise.all([
          invoke<RuleSetSummary[]>('mm_list_rule_sets', { includeArchived: false }),
          invoke<ProfileView>('mm_get_profile', { id: a }),
          invoke<ProfileView>('mm_get_profile', { id: b }),
          invoke<string | null>('mm_find_match_record', { profileA: a, profileB: b }),
        ]);
        setSets(s);
        if (!setId && s.length > 0) setSetId(s[0].id);
        setNames({ a: nameOf(pa, 'Person A'), b: nameOf(pb, 'Person B') });
        setExistingId(existing);
      } catch (e) {
        toast.error(`${e}`);
      }
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [matchId, a, b]);

  useEffect(() => {
    if (matchId || !a || !b || !setId) return;
    (async () => {
      try {
        setView(await invoke<MatchView>('mm_score_pair', { ruleSetId: setId, profileA: a, profileB: b, version: null }));
      } catch (e) {
        toast.error(`${e}`);
      }
    })();
  }, [matchId, a, b, setId]);

  const track = async () => {
    if (!a || !b || !setId) return;
    setCreating(true);
    try {
      const d = await invoke<MatchDetail>('mm_create_match', { profileA: a, profileB: b, ruleSetId: setId, sourceProfile: a });
      router.replace(`/match?id=${encodeURIComponent(d.id)}`);
    } catch (e) {
      toast.error(`${e}`);
    } finally {
      setCreating(false);
    }
  };

  if (!matchId && (!a || !b)) return <div className="p-6">Choose two profiles to compare.</div>;

  const title = matchId ? `${detail?.name_a || 'Person A'} ↔ ${detail?.name_b || 'Person B'}` : `${names.a} ↔ ${names.b}`;

  return (
    <div className="min-h-screen bg-gray-50">
      <div className="max-w-4xl mx-auto p-6 space-y-5">
        <div className="flex items-center justify-between gap-3">
          <div className="flex items-center gap-3">
            <button onClick={() => router.back()} className="p-2 rounded-lg hover:bg-gray-200" aria-label="Back">
              <ArrowLeft className="w-5 h-5" />
            </button>
            <h1 className="text-2xl font-semibold">{title}</h1>
            {detail && <span className={`px-2 py-0.5 rounded text-sm ${STATUS_CHIP[detail.status]}`}>{STATUS_LABEL[detail.status]}</span>}
          </div>
          {!matchId && (
            <div className="flex items-center gap-2">
              <select className="border rounded-md px-2 py-1.5 text-sm bg-white" value={setId} onChange={(e) => setSetId(e.target.value)}>
                {sets.map((s) => <option key={s.id} value={s.id}>{s.name}</option>)}
              </select>
              {existingId ? (
                <Button size="sm" onClick={() => router.push(`/match?id=${encodeURIComponent(existingId)}`)}>Open match record</Button>
              ) : (
                <Button size="sm" onClick={track} disabled={creating || !setId}>{creating ? 'Saving…' : 'Track this pair'}</Button>
              )}
            </div>
          )}
        </div>

        {matchId && !detail && <div className="text-gray-500">Loading…</div>}
        {matchId && detail && (
          <>
            {detail.scorecard ? (
              <ScoreCardView card={detail.scorecard} nameA={detail.name_a || 'Person A'} nameB={detail.name_b || 'Person B'} />
            ) : (
              <div className="text-gray-500">No score stored for this match.</div>
            )}
            <MatchWorkflowPanel detail={detail} dims={dims} onChange={setDetail} />
          </>
        )}

        {!matchId && !view && <div className="text-gray-500">Scoring…</div>}
        {!matchId && view && (
          <>
            <ScoreCardView card={view.scorecard} nameA={names.a} nameB={names.b} />
            <div className="bg-white rounded-lg border p-4">
              <button className="font-medium text-left w-full" onClick={() => setShowRules((v) => !v)}>
                Rule evaluation {showRules ? '▾' : '▸'}
              </button>
              {showRules && (
                <div className="mt-3 space-y-3 text-sm">
                  <RuleResultList title="Rules" results={view.evaluation.rules.results} />
                  <RuleResultList title={`${names.a}'s preferences about ${names.b}`} results={view.evaluation.a_preferences.results} />
                  <RuleResultList title={`${names.b}'s preferences about ${names.a}`} results={view.evaluation.b_preferences.results} />
                </div>
              )}
            </div>
          </>
        )}
      </div>
    </div>
  );
}

export default function Page() {
  return (
    <Suspense fallback={<div className="p-6 text-gray-500">Loading…</div>}>
      <MatchPage />
    </Suspense>
  );
}
