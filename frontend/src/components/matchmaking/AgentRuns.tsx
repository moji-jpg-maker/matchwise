'use client';

import { useCallback, useEffect, useState } from 'react';
import Link from 'next/link';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Textarea } from '@/components/ui/textarea';
import { useI18n } from '@/i18n';
import { AgentRun, AgentTrace } from '@/types/matchmaking';

function Proposal({ trace, onDecided }: { trace: AgentTrace; onDecided: () => void }) {
  const { t, locale, fmt } = useI18n();
  const [args, setArgs] = useState(JSON.stringify(trace.action.args, null, 2));
  const [busy, setBusy] = useState(false);
  const [now, setNow] = useState(Date.now());
  const scheduled = trace.status === 'scheduled';
  useEffect(() => {
    if (!scheduled) return;
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, [scheduled]);
  const veto = async () => {
    setBusy(true);
    try {
      await invoke('agent_veto', { proposalId: trace.id });
      onDecided();
    } catch (error) {
      toast.error(t('agent.decisionFailed'), { description: String(error) });
    } finally { setBusy(false); }
  };
  const decide = async (approve: boolean) => {
    setBusy(true);
    try {
      await invoke('agent_decide_proposal', { proposalId: trace.id, approve, args: approve ? JSON.parse(args) : null, locale });
      onDecided();
    } catch (error) {
      toast.error(t('agent.decisionFailed'), { description: String(error) });
    } finally {
      setBusy(false);
    }
  };
  return <div className="border rounded p-3 space-y-2">
    <p className="text-sm">{t('agent.runNumber', { id: fmt.number(trace.run_id) })} · {t(`agent.tool.${trace.action.tool}`)}</p>
    {scheduled ? <>
      <p className="text-xs" role="timer">{(trace.due_at ?? 0) * 1000 > now
        ? t('agent.countdown', { seconds: fmt.number(Math.ceil(((trace.due_at ?? 0) * 1000 - now) / 1000)) })
        : t('agent.due')}</p>
      {trace.due_at !== null && <time className="block text-xs text-gray-500">{new Date(trace.due_at * 1000).toLocaleString(locale)}</time>}
      <pre dir="ltr" className="text-xs whitespace-pre-wrap break-all">{JSON.stringify(trace.action.args, null, 2)}</pre>
      <Button size="sm" variant="outline" disabled={busy} onClick={() => void veto()}>{t('agent.veto')}</Button>
    </> : <><label className="block text-xs space-y-1">
      <span>{t('agent.editArgs')}</span>
      <Textarea dir="ltr" className="font-mono text-xs" value={args} onChange={(event) => setArgs(event.target.value)} disabled={busy} />
    </label>
    <div className="flex gap-2">
      <Button size="sm" disabled={busy} onClick={() => void decide(true)}>{t('agent.approve')}</Button>
      <Button size="sm" variant="outline" disabled={busy} onClick={() => void decide(false)}>{t('agent.reject')}</Button>
    </div></>}
  </div>;
}

export function AgentRuns() {
  const { t, fmt } = useI18n();
  const [runs, setRuns] = useState<AgentRun[]>([]);
  const [proposals, setProposals] = useState<AgentTrace[]>([]);
  const [selected, setSelected] = useState<number | null>(null);
  const [traces, setTraces] = useState<AgentTrace[]>([]);
  const [error, setError] = useState(false);
  const refresh = useCallback(async () => {
    try {
      const [nextRuns, nextProposals, nextTraces] = await Promise.all([
        invoke<AgentRun[]>('agent_runs'),
        invoke<AgentTrace[]>('agent_proposals'),
        selected === null ? Promise.resolve([]) : invoke<AgentTrace[]>('agent_run_detail', { runId: selected }),
      ]);
      setRuns(nextRuns);
      setProposals(nextProposals);
      setTraces(nextTraces);
      setError(false);
    } catch {
      setError(true);
    }
  }, [selected]);
  useEffect(() => {
    void refresh();
    const timer = setInterval(() => void refresh(), 5000);
    return () => clearInterval(timer);
  }, [refresh]);

  return <section className="space-y-4 border-t pt-6">
    <div className="flex justify-between items-center">
      <h2 className="font-semibold">{t('agent.log')}</h2>
      <Button size="sm" variant="outline" onClick={() => void refresh()}>{t('agent.refresh')}</Button>
    </div>
    <p className="text-xs text-gray-500">{t('agent.scope')}</p>
    {error && <p role="alert" className="text-sm text-red-600">{t('agent.loadFailed')}</p>}
    <h3 className="text-sm font-medium">{t('agent.inbox')}</h3>
    {proposals.length === 0 && <p className="text-xs text-gray-500">{t('agent.noProposals')}</p>}
    {proposals.map((trace) => <Proposal key={trace.id} trace={trace} onDecided={() => void refresh()} />)}
    {runs.length === 0 && <p className="text-sm text-gray-500">{t('agent.noRuns')}</p>}
    {runs.map((run) => <div key={run.id} className="flex flex-wrap items-center gap-2 text-sm border-b pb-2">
      <button className="underline" onClick={() => { setTraces([]); setSelected(run.id); }}>{t('agent.runNumber', { id: fmt.number(run.id) })}</button>
      <Link className="underline font-mono text-xs" href={`/profile?id=${encodeURIComponent(run.profile_id)}`}>{run.profile_id}</Link>
      <span>{t(`agent.status.${run.status}`)}</span>
      <time className="text-xs text-gray-500">{run.created_at}</time>
    </div>)}
    {selected !== null && <div className="space-y-2">
      <h3 className="text-sm font-medium">{t('agent.runNumber', { id: fmt.number(selected) })}</h3>
      {traces.map((trace) => <details key={trace.id} className="border rounded p-2 text-xs">
        <summary className="cursor-pointer">{fmt.number(trace.turn + 1)} · {t(`agent.tool.${trace.action.tool}`)} · {t(`agent.status.${trace.status}`)}</summary>
        <p className="mt-2 font-mono">{trace.actor}{trace.decided_by ? ` / ${trace.decided_by}` : ''}</p>
        <pre dir="ltr" className="mt-2 whitespace-pre-wrap break-all">{JSON.stringify({ args: trace.action.args, observation: trace.observation }, null, 2)}</pre>
      </details>)}
    </div>}
  </section>;
}
