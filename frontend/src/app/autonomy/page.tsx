'use client';

import React, { useCallback, useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Switch } from '@/components/ui/switch';
import {
  AUTONOMY_LEVELS,
  AUTONOMY_STEPS,
  AutonomyLevel,
  AutonomyPolicy,
  AutonomyStep,
  AutonomyStepMode,
  BUILT_AUTONOMY_STEPS,
  PolicyView,
  UNRELEASED_AUTONOMY_LEVELS,
} from '@/types/matchmaking';
import { useI18n } from '@/i18n';

const DEFAULT_REVIEW_MINUTES = 60;

type ModeName = 'off' | 'suggest' | 'review_window' | 'auto';

function modeName(mode: AutonomyStepMode): ModeName {
  return typeof mode === 'string' ? mode : 'review_window';
}

function modeFromName(name: ModeName, previous: AutonomyStepMode): AutonomyStepMode {
  if (name !== 'review_window') return name;
  return typeof previous === 'object' ? previous : { review_window: { minutes: DEFAULT_REVIEW_MINUTES } };
}

/** Mirrors `AutonomyPolicy::preset` in matchmaking-core closely enough to preview a level before saving. */
function presetSteps(level: AutonomyLevel): Record<AutonomyStep, AutonomyStepMode> {
  const steps = {} as Record<AutonomyStep, AutonomyStepMode>;
  for (const step of AUTONOMY_STEPS) {
    const candidateFacing = ['send_introduction', 'send_follow_up', 'send_message', 'request_info'].includes(step);
    const internal = !candidateFacing && step !== 'rule_override' && step !== 'approve';
    let mode: AutonomyStepMode = 'off';
    if (level === 'l1_assisted') mode = 'suggest';
    if (level === 'l2_supervised') mode = internal ? 'auto' : 'suggest';
    if (level === 'l3_delegated') {
      if (candidateFacing) mode = { review_window: { minutes: DEFAULT_REVIEW_MINUTES } };
      else mode = step === 'rule_override' ? 'suggest' : 'auto';
    }
    if (level === 'l4_autonomous') mode = 'auto';
    steps[step] = BUILT_AUTONOMY_STEPS.includes(step) ? mode : 'off';
  }
  return steps;
}

export default function AutonomyPage() {
  const { t } = useI18n();
  const [policy, setPolicy] = useState<AutonomyPolicy | null>(null);
  const [custom, setCustom] = useState(false);
  const [saving, setSaving] = useState(false);

  const load = useCallback(async () => {
    try {
      const view = await invoke<PolicyView>('agent_get_policy', {});
      setPolicy(view.policy);
      setCustom(view.custom);
    } catch (error) {
      toast.error(t('autonomy.loadFailed'), { description: String(error) });
    }
  }, [t]);

  useEffect(() => { void load(); }, [load]);

  if (!policy) return <div className="p-6 text-sm text-gray-500">{t('common.loading')}</div>;

  const chooseLevel = (level: AutonomyLevel) => {
    setPolicy({ ...policy, level, steps: presetSteps(level) });
    setCustom(false);
  };

  const setMode = (step: AutonomyStep, name: ModeName) => {
    setPolicy({ ...policy, steps: { ...policy.steps, [step]: modeFromName(name, policy.steps[step]) } });
    setCustom(true);
  };

  const setMinutes = (step: AutonomyStep, minutes: number) => {
    setPolicy({ ...policy, steps: { ...policy.steps, [step]: { review_window: { minutes } } } });
    setCustom(true);
  };

  const save = async () => {
    setSaving(true);
    try {
      const view = await invoke<PolicyView>('agent_save_policy', { policy });
      setPolicy(view.policy);
      setCustom(view.custom);
      toast.success(t('autonomy.saved'));
    } catch (error) {
      toast.error(t('autonomy.saveFailed'), { description: String(error) });
    } finally {
      setSaving(false);
    }
  };

  const togglePaused = async (paused: boolean) => {
    try {
      const view = await invoke<PolicyView>('agent_set_paused', { paused });
      setPolicy((current) => (current ? { ...current, paused: view.policy.paused, version: view.policy.version } : current));
      window.dispatchEvent(new Event('autonomy-changed'));
    } catch (error) {
      toast.error(t('autonomy.saveFailed'), { description: String(error) });
    }
  };

  return (
    <div className="p-6 space-y-6 max-w-3xl">
      <div>
        <h1 className="text-2xl font-semibold">{t('autonomy.title')}</h1>
        <p className="text-sm text-gray-500 mt-1">{t('autonomy.intro')}</p>
      </div>

      <div className="bg-white rounded-lg border p-4 flex items-center justify-between gap-4">
        <div className="text-sm font-medium">{policy.paused ? t('autonomy.resume') : t('autonomy.pause')}</div>
        <Switch checked={policy.paused} onCheckedChange={(paused) => void togglePaused(paused)} aria-label={t('autonomy.pause')} />
      </div>

      <div className="bg-white rounded-lg border divide-y">
        <div className="p-4 text-sm font-medium">
          {t('autonomy.preset')}
          {custom && <span className="ms-2 text-xs font-normal text-amber-600">{t('autonomy.custom')}</span>}
        </div>
        {AUTONOMY_LEVELS.map((level) => {
          const unreleased = UNRELEASED_AUTONOMY_LEVELS.includes(level);
          return (
            <label key={level} className={`p-4 flex items-start gap-3 ${unreleased ? 'opacity-50' : 'cursor-pointer'}`}>
              <input
                type="radio"
                name="autonomy-level"
                className="mt-1"
                checked={policy.level === level}
                disabled={unreleased}
                onChange={() => chooseLevel(level)}
              />
              <span>
                <span className="text-sm font-medium">{t(`autonomy.level.${level}`)}</span>
                {unreleased && <span className="ms-2 text-xs text-gray-500">{t('autonomy.unreleased')}</span>}
                <span className="block text-xs text-gray-500">{t(`autonomy.levelDesc.${level}`)}</span>
              </span>
            </label>
          );
        })}
      </div>

      <div className="bg-white rounded-lg border divide-y">
        <div className="p-4 text-sm font-medium">{t('autonomy.stepsTitle')}</div>
        {AUTONOMY_STEPS.map((step) => {
          const built = BUILT_AUTONOMY_STEPS.includes(step);
          const mode = policy.steps[step];
          return (
            <div key={step} className={`p-4 flex items-center justify-between gap-4 ${built ? '' : 'opacity-50'}`}>
              <div className="text-sm">
                {t(`autonomy.step.${step}`)}
                {!built && <span className="ms-2 text-xs text-gray-500">{t('autonomy.notBuilt')}</span>}
              </div>
              <div className="flex items-center gap-2">
                {typeof mode === 'object' && (
                  <>
                    <Input
                      type="number"
                      min={1}
                      className="w-20"
                      disabled={!built}
                      value={mode.review_window.minutes}
                      onChange={(event) => setMinutes(step, Math.max(1, Number(event.target.value) || 1))}
                    />
                    <span className="text-xs text-gray-500">{t('autonomy.minutes')}</span>
                  </>
                )}
                <select
                  className="border rounded-md px-2 py-1 text-sm bg-white"
                  disabled={!built}
                  value={modeName(mode)}
                  aria-label={t(`autonomy.step.${step}`)}
                  onChange={(event) => setMode(step, event.target.value as ModeName)}
                >
                  {(['off', 'suggest', 'review_window', 'auto'] as ModeName[]).map((name) => (
                    <option key={name} value={name}>{t(`autonomy.mode.${name}`)}</option>
                  ))}
                </select>
              </div>
            </div>
          );
        })}
      </div>

      <div className="bg-white rounded-lg border p-4 space-y-2">
        <div className="text-sm font-medium">{t('autonomy.threshold')}</div>
        <Input
          type="number"
          min={0}
          max={1}
          step={0.05}
          className="w-28"
          value={policy.min_confidence}
          onChange={(event) => setPolicy({ ...policy, min_confidence: Math.min(1, Math.max(0, Number(event.target.value))) })}
        />
        <p className="text-xs text-gray-500">{t('autonomy.thresholdHelp')}</p>
      </div>

      <p className="text-xs text-gray-500">{t('autonomy.humanOnly')}</p>
      <Button onClick={() => void save()} disabled={saving}>{t('autonomy.save')}</Button>
    </div>
  );
}
