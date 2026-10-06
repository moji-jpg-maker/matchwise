'use client';

import React, { Suspense, useEffect, useState } from 'react';
import { useRouter, useSearchParams } from 'next/navigation';
import { invoke } from '@tauri-apps/api/core';
import { ArrowLeft } from 'lucide-react';
import { toast } from 'sonner';
import { ScoreCardView } from '@/components/matchmaking/ScoreCardView';
import { RuleResultList } from '@/components/matchmaking/RuleResultList';
import { MatchView, ProfileView, RuleSetSummary } from '@/types/matchmaking';

function MatchPage() {
  const router = useRouter();
  const q = useSearchParams();
  const a = q.get('a');
  const b = q.get('b');
  const [sets, setSets] = useState<RuleSetSummary[]>([]);
  const [setId, setSetId] = useState(q.get('set') ?? '');
  const [view, setView] = useState<MatchView | null>(null);
  const [names, setNames] = useState<{ a: string; b: string }>({ a: 'Person A', b: 'Person B' });
  const [showRules, setShowRules] = useState(false);

  useEffect(() => {
    (async () => {
      try {
        const s = await invoke<RuleSetSummary[]>('mm_list_rule_sets', { includeArchived: false });
        setSets(s);
        if (!setId && s.length > 0) setSetId(s[0].id);
        if (a && b) {
          const [pa, pb] = await Promise.all([invoke<ProfileView>('mm_get_profile', { id: a }), invoke<ProfileView>('mm_get_profile', { id: b })]);
          setNames({ a: (pa.fields['full_name']?.value as string) || 'Person A', b: (pb.fields['full_name']?.value as string) || 'Person B' });
        }
      } catch (e) {
        toast.error(`${e}`);
      }
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [a, b]);

  useEffect(() => {
    if (!a || !b || !setId) return;
    (async () => {
      try {
        setView(await invoke<MatchView>('mm_score_pair', { ruleSetId: setId, profileA: a, profileB: b, version: null }));
      } catch (e) {
        toast.error(`${e}`);
      }
    })();
  }, [a, b, setId]);

  if (!a || !b) return <div className="p-6">Choose two profiles to compare.</div>;

  return (
    <div className="min-h-screen bg-gray-50">
      <div className="max-w-4xl mx-auto p-6 space-y-5">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-3">
            <button onClick={() => router.back()} className="p-2 rounded-lg hover:bg-gray-200" aria-label="Back">
              <ArrowLeft className="w-5 h-5" />
            </button>
            <h1 className="text-2xl font-semibold">{names.a} ↔ {names.b}</h1>
          </div>
          <select className="border rounded-md px-2 py-1.5 text-sm bg-white" value={setId} onChange={(e) => setSetId(e.target.value)}>
            {sets.map((s) => <option key={s.id} value={s.id}>{s.name}</option>)}
          </select>
        </div>

        {!view && <div className="text-gray-500">Scoring…</div>}
        {view && (
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
