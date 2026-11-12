'use client';

import { useState } from 'react';
import Link from 'next/link';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { useI18n } from '@/i18n';

export function AgentRunButton({ profileId, batchId, disabled, onComplete }: {
  profileId?: string;
  batchId?: string;
  disabled?: boolean;
  onComplete?: () => void;
}) {
  const { t, locale } = useI18n();
  const [running, setRunning] = useState(false);
  const [started, setStarted] = useState(false);

  const run = async () => {
    setRunning(true);
    setStarted(true);
    try {
      await invoke<number[]>('agent_run_now', { profileId: profileId ?? null, batchId: batchId ?? null, locale });
      toast.info(t('agent.runEnded'));
      onComplete?.();
    } catch (error) {
      toast.error(t('agent.runFailed'), { description: String(error) });
    } finally {
      setRunning(false);
    }
  };

  return <div className="flex items-center gap-2">
    <Button variant="outline" disabled={disabled || running} onClick={() => void run()}>{t(running ? 'agent.running' : 'agent.runNow')}</Button>
    {started && <Link href="/autonomy" className="text-xs underline">{t('agent.log')}</Link>}
  </div>;
}
