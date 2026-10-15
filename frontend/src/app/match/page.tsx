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
import { MatchAiPanel } from '@/components/matchmaking/MatchAiPanel';
import { STATUS_CHIP, STATUS_KEY } from '@/lib/workflow';
import { useI18n } from '@/i18n';
import { DimensionDef, MatchDetail, MatchView, ProfileView, RuleSetSummary } from '@/types/matchmaking';

const nameOf = (p: ProfileView, fallback: string) => (p.fields['full_name']?.value as string) || fallback;

function MatchPage() {
  const { t, locale } = useI18n();
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
  const [names, setNames] = useState<{ a: string; b: string }>({ a: '', b: '' });
  const [existingId, setExistingId] = useState<string | null>(null);
  const [showRules, setShowRules] = useState(false);
  const [creating, setCreating] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [setsLoaded, setSetsLoaded] = useState(false);

  // ---- tracked match (record mode)
  const loadDetail = useCallback(async () => {
    if (!matchId) return;
    setLoadError(null);
    try {
      const [d, catalog] = await Promise.all([
        invoke<MatchDetail>('mm_get_match', { id: matchId, locale }),
        invoke<DimensionDef[]>('mm_dimension_catalog', { locale }),
      ]);
      setDetail(d);
      setDims(catalog);
    } catch (e) {
      setLoadError(`${e}`);
      toast.error(`${e}`);
    }
  }, [matchId, locale]);

  useEffect(() => { void loadDetail(); }, [loadDetail]);

  // ---- ad-hoc comparison of two people
  useEffect(() => {
    if (matchId || !a || !b) return;
    (async () => {
      try {
        const [s, pa, pb, existing] = await Promise.all([
          invoke<RuleSetSummary[]>('mm_list_rule_sets', { includeArchived: false }),
          invoke<ProfileView>('mm_get_profile', { id: a, locale }),
          invoke<ProfileView>('mm_get_profile', { id: b, locale }),
          invoke<string | null>('mm_find_match_record', { profileA: a, profileB: b }),
        ]);
        setSets(s);
        if (!setId && s.length > 0) setSetId(s[0].id);
        setNames({ a: nameOf(pa, ''), b: nameOf(pb, '') });
        setExistingId(existing);
      } catch (e) {
        setLoadError(`${e}`);
        toast.error(`${e}`);
      } finally {
        setSetsLoaded(true);
      }
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [matchId, a, b]);

  useEffect(() => {
    if (matchId || !a || !b || !setId) return;
    (async () => {
      try {
        setView(await invoke<MatchView>('mm_score_pair', { ruleSetId: setId, profileA: a, profileB: b, version: null, locale }));
      } catch (e) {
        toast.error(`${e}`);
      }
    })();
  }, [matchId, a, b, setId, locale]);

  const track = async () => {
    if (!a || !b || !setId) return;
    setCreating(true);
    try {
      const d = await invoke<MatchDetail>('mm_create_match', { profileA: a, profileB: b, ruleSetId: setId, sourceProfile: a, locale });
      router.replace(`/match?id=${encodeURIComponent(d.id)}`);
    } catch (e) {
      toast.error(`${e}`);
    } finally {
      setCreating(false);
    }
  };

  if (!matchId && (!a || !b)) return <div className="p-6">{t('match.choose')}</div>;

  if (loadError && (matchId ? !detail : !view))
    return (
      <div className="p-6 space-y-3">
        <div className="text-red-700">{t(matchId ? 'match.loadMatch' : 'match.loadComparison', { error: loadError })}</div>
        <div className="flex gap-2">
          {matchId && <Button variant="outline" onClick={() => void loadDetail()}>{t('profiles.retry')}</Button>}
          <Button variant="ghost" onClick={() => router.push(matchId ? '/matches' : '/profiles')}>{t('profiles.back')}</Button>
        </div>
      </div>
    );

  const personA = (matchId ? detail?.name_a : names.a) || t('match.personA');
  const personB = (matchId ? detail?.name_b : names.b) || t('match.personB');
  const title = `${personA} ↔ ${personB}`;

  return (
    <div className="min-h-screen bg-gray-50">
      <div className="max-w-4xl mx-auto p-6 space-y-5">
        <div className="flex items-center justify-between gap-3">
          <div className="flex items-center gap-3">
            <button onClick={() => router.back()} className="p-2 rounded-lg hover:bg-gray-200" aria-label={t('profiles.back')}>
              <ArrowLeft className="w-5 h-5 rtl-mirror" />
            </button>
            <h1 className="text-2xl font-semibold"><bdi>{title}</bdi></h1>
            {detail && <span className={`px-2 py-0.5 rounded text-sm ${STATUS_CHIP[detail.status]}`}>{t(STATUS_KEY[detail.status])}</span>}
          </div>
          {!matchId && (
            <div className="flex items-center gap-2">
              <select className="border rounded-md px-2 py-1.5 text-sm bg-white" value={setId} onChange={(e) => setSetId(e.target.value)}>
                {sets.map((s) => <option key={s.id} value={s.id}>{s.name}</option>)}
              </select>
              {existingId ? (
                <Button size="sm" onClick={() => router.push(`/match?id=${encodeURIComponent(existingId)}`)}>{t('match.openRecord')}</Button>
              ) : (
                <Button size="sm" onClick={track} disabled={creating || !setId}>{creating ? t('match.saving') : t('match.track')}</Button>
              )}
            </div>
          )}
        </div>

        {matchId && !detail && <div className="text-gray-500">{t('common.loading')}</div>}
        {matchId && detail && (
          <>
            {detail.scorecard ? (
              <ScoreCardView card={detail.scorecard} nameA={personA} nameB={personB} />
            ) : (
              <div className="text-gray-500">{t('match.noScore')}</div>
            )}
            <MatchAiPanel matchId={detail.id} nameA={personA} nameB={personB} />
            <MatchWorkflowPanel detail={detail} dims={dims} onChange={setDetail} />
          </>
        )}

        {!matchId && !view && setsLoaded && sets.length === 0 && (
          <div className="text-gray-500">
            {t('match.noRuleSetsPre')} <button className="underline" onClick={() => router.push('/rules')}>{t('match.createRuleSet')}</button> {t('match.noRuleSetsPost')}
          </div>
        )}
        {!matchId && !view && !(setsLoaded && sets.length === 0) && <div className="text-gray-500">{t('match.scoring')}</div>}
        {!matchId && view && (
          <>
            <ScoreCardView card={view.scorecard} nameA={personA} nameB={personB} />
            <div className="bg-white rounded-lg border p-4">
              <button className="font-medium text-start w-full" onClick={() => setShowRules((v) => !v)}>
                {t('match.ruleEvaluation')} {showRules ? '▾' : '▸'}
              </button>
              {showRules && (
                <div className="mt-3 space-y-3 text-sm">
                  <RuleResultList title={t('match.rules')} results={view.evaluation.rules.results} />
                  <RuleResultList title={t('match.prefsAbout', { a: personA, b: personB })} results={view.evaluation.a_preferences.results} />
                  <RuleResultList title={t('match.prefsAbout', { a: personB, b: personA })} results={view.evaluation.b_preferences.results} />
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
  const { t } = useI18n();
  const fallbackLoading = t('common.loading');
  return (
    <Suspense fallback={<div className="p-6 text-gray-500">{fallbackLoading}</div>}>
      <MatchPage />
    </Suspense>
  );
}
