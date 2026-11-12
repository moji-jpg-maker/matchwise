'use client';

import React, { useCallback, useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Bot } from 'lucide-react';
import { Switch } from '@/components/ui/switch';
import { AutonomyPolicy, PolicyView } from '@/types/matchmaking';
import { useI18n } from '@/i18n';

/** Slim global header: the current autonomy level and the kill switch. */
export const KillSwitchBar: React.FC = () => {
  const { t } = useI18n();
  const [policy, setPolicy] = useState<AutonomyPolicy | null>(null);

  const load = useCallback(async () => {
    try {
      setPolicy((await invoke<PolicyView>('agent_get_policy', {})).policy);
    } catch {
      setPolicy(null);
    }
  }, []);

  useEffect(() => {
    void load();
    window.addEventListener('autonomy-changed', load);
    return () => window.removeEventListener('autonomy-changed', load);
  }, [load]);

  if (!policy) return null;

  const off = policy.level === 'l0_manual';
  const label = policy.paused
    ? t('autonomy.pausedBanner')
    : off
      ? t('autonomy.offBanner')
      : t('autonomy.activeBanner', { level: t(`autonomy.level.${policy.level}`) });

  const toggle = async (paused: boolean) => {
    try {
      const view = await invoke<PolicyView>('agent_set_paused', { paused });
      setPolicy(view.policy);
      window.dispatchEvent(new Event('autonomy-changed'));
    } catch {
      await load();
    }
  };

  return (
    <div
      className={`flex items-center justify-between gap-3 px-4 py-1 text-xs border-b ${
        policy.paused ? 'bg-amber-50 text-amber-800' : 'bg-gray-50 text-gray-600'
      }`}
    >
      <span className="flex items-center gap-2"><Bot className="w-3.5 h-3.5" />{label}</span>
      {!off && (
        <span className="flex items-center gap-2">
          {policy.paused ? t('autonomy.resume') : t('autonomy.pause')}
          <Switch checked={policy.paused} onCheckedChange={(paused) => void toggle(paused)} aria-label={t('autonomy.pause')} />
        </span>
      )}
    </div>
  );
};
