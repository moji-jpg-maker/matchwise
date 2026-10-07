'use client';

import React, { Suspense, useCallback, useEffect, useState } from 'react';
import { useRouter, useSearchParams } from 'next/navigation';
import { invoke } from '@tauri-apps/api/core';
import { ArrowLeft, Trash2 } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { FieldDef, ProfileView, RecordValue, UpdateResult, Value, kindName, kindOptions, recordFields } from '@/types/matchmaking';
import { RecordsInput } from '@/components/matchmaking/RecordsInput';
import { PreferencesPanel } from '@/components/matchmaking/PreferencesPanel';
import { MatchesPanel } from '@/components/matchmaking/MatchesPanel';
import { TelegramPanel } from '@/components/matchmaking/TelegramPanel';

const SOURCE_LABEL: Record<string, string> = {
  user: 'entered by the candidate',
  matchmaker: 'entered by a matchmaker',
  questionnaire: 'from a questionnaire',
  ai_inferred: 'suggested by AI: please verify',
};

function ProfileEditor() {
  const router = useRouter();
  const id = useSearchParams().get('id');
  const [fields, setFields] = useState<FieldDef[]>([]);
  const [profile, setProfile] = useState<ProfileView | null>(null);
  const [draft, setDraft] = useState<Record<string, Value | null>>({});
  const [saving, setSaving] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);

  const load = useCallback(async () => {
    if (!id) return;
    try {
      const [f, p] = await Promise.all([
        invoke<FieldDef[]>('mm_list_fields'),
        invoke<ProfileView>('mm_get_profile', { id }),
      ]);
      setFields(f);
      setProfile(p);
      setDraft({});
    } catch (e) {
      toast.error(`Could not load profile: ${e}`);
    }
  }, [id]);

  useEffect(() => { void load(); }, [load]);

  if (!id) return <div className="p-6">No profile selected.</div>;
  if (!profile) return <div className="p-6 text-gray-500">Loading…</div>;

  const current = (key: string): Value | undefined => {
    if (key in draft) return draft[key] ?? undefined;
    return profile.fields[key]?.value;
  };
  const setField = (key: string, v: Value | null) => setDraft((d) => ({ ...d, [key]: v }));
  const dirty = Object.keys(draft).length > 0;

  const save = async () => {
    setSaving(true);
    try {
      const res = await invoke<UpdateResult>('mm_update_profile_fields', { id, values: draft, source: 'matchmaker' });
      setProfile(res.profile);
      setDraft({});
      if (res.rejected.length) toast.warning(`Not changed (protected by a higher-trust source): ${res.rejected.join(', ')}`);
      else toast.success('Saved');
    } catch (e) {
      toast.error(`${e}`);
    } finally {
      setSaving(false);
    }
  };

  const toggleActive = async () => {
    try {
      await invoke('mm_set_profile_active', { id, active: profile.status !== 'active' });
      await load();
    } catch (e) {
      toast.error(`${e}`);
    }
  };

  const remove = async () => {
    try {
      await invoke('mm_delete_profile', { id });
      router.push('/profiles');
    } catch (e) {
      toast.error(`${e}`);
    }
  };

  const input = (def: FieldDef) => {
    const kind = kindName(def.kind);
    const v = current(def.key);
    const clearOnEmpty = (s: string, make: (s: string) => Value) => setField(def.key, s === '' ? null : make(s));
    if (kind === 'number')
      return <Input type="number" value={typeof v === 'number' ? String(v) : ''} onChange={(e) => clearOnEmpty(e.target.value, Number)} />;
    if (kind === 'bool')
      return (
        <select className="border rounded-md px-2 py-2 text-sm bg-white w-full" value={typeof v === 'boolean' ? String(v) : ''}
          onChange={(e) => clearOnEmpty(e.target.value, (s) => s === 'true')}>
          <option value="">not set</option>
          <option value="true">yes</option>
          <option value="false">no</option>
        </select>
      );
    if (kind === 'choice')
      return (
        <select className="border rounded-md px-2 py-2 text-sm bg-white w-full" value={typeof v === 'string' ? v : ''}
          onChange={(e) => clearOnEmpty(e.target.value, (s) => s)}>
          <option value="">not set</option>
          {kindOptions(def.kind).map((o) => <option key={o} value={o}>{o}</option>)}
        </select>
      );
    if (kind === 'multi_choice') {
      const sel = Array.isArray(v) ? (v as string[]) : [];
      return (
        <div className="flex flex-wrap gap-3">
          {kindOptions(def.kind).map((o) => (
            <label key={o} className="flex items-center gap-1 text-sm">
              <input type="checkbox" checked={sel.includes(o)}
                onChange={(e) => {
                  const next = e.target.checked ? [...sel, o] : sel.filter((x) => x !== o);
                  setField(def.key, next.length ? next : null);
                }} />
              {o}
            </label>
          ))}
        </div>
      );
    }
    if (kind === 'records') {
      const entries = Array.isArray(v) ? (v as RecordValue[]).filter((x) => typeof x === 'object' && x !== null) : [];
      return (
        <RecordsInput
          defs={recordFields(def.kind)}
          entries={entries}
          entryLabel={def.key === 'children' ? 'Child' : 'Entry'}
          onChange={(next) => setField(def.key, next)}
        />
      );
    }
    return <Input value={typeof v === 'string' ? v : ''} onChange={(e) => clearOnEmpty(e.target.value, (s) => s)} />;
  };

  return (
    <div className="min-h-screen bg-gray-50">
      <div className="max-w-3xl mx-auto p-6 space-y-5">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-3">
            <button onClick={() => router.push('/profiles')} className="p-2 rounded-lg hover:bg-gray-200" aria-label="Back">
              <ArrowLeft className="w-5 h-5" />
            </button>
            <h1 className="text-2xl font-semibold">
              {(profile.fields['full_name']?.value as string) || 'Unnamed profile'}
            </h1>
            {profile.status === 'deactivated' && <span className="text-xs px-2 py-0.5 rounded bg-gray-200">deactivated</span>}
          </div>
          <Button onClick={save} disabled={!dirty || saving}>{saving ? 'Saving…' : 'Save changes'}</Button>
        </div>

        <div className="bg-white rounded-lg border p-4 text-sm flex items-center justify-between">
          <div>
            Completeness: {profile.completeness != null ? `${Math.round(profile.completeness * 100)}%` : 'n/a'}
            {profile.missing_required.length > 0 && (
              <span className="text-gray-500"> · missing: {profile.missing_required.join(', ')}</span>
            )}
          </div>
        </div>

        {profile.issues.length > 0 && (
          <div className="bg-amber-50 border border-amber-200 rounded-lg p-3 text-sm">
            {profile.issues.map((i) => <div key={i.field}>{i.field}: {i.message}</div>)}
          </div>
        )}

        <div className="bg-white rounded-lg border divide-y">
          {fields.map((def) => {
            const src = profile.fields[def.key]?.source;
            return (
              <div key={def.key} className="p-4 grid grid-cols-3 gap-4 items-start">
                <div>
                  <div className="text-sm font-medium">
                    {def.label}{def.required && <span className="text-red-500"> *</span>}
                  </div>
                  {def.sensitive && <div className="text-xs text-gray-500">sensitive</div>}
                  {src && !(def.key in draft) && <div className="text-xs text-gray-400">{SOURCE_LABEL[src]}</div>}
                </div>
                <div className="col-span-2">{input(def)}</div>
              </div>
            );
          })}
        </div>

        <PreferencesPanel profileId={id} fields={fields} />

        <MatchesPanel profileId={id} />

        <TelegramPanel profileId={id} />

        <div className="flex items-center justify-between pt-2">
          <Button variant="outline" onClick={toggleActive}>
            {profile.status === 'active' ? 'Deactivate profile' : 'Reactivate profile'}
          </Button>
          {confirmDelete ? (
            <div className="flex items-center gap-2 text-sm">
              <span>Permanently delete this profile?</span>
              <Button variant="destructive" size="sm" onClick={remove}>Delete</Button>
              <Button variant="ghost" size="sm" onClick={() => setConfirmDelete(false)}>Cancel</Button>
            </div>
          ) : (
            <Button variant="ghost" onClick={() => setConfirmDelete(true)}>
              <Trash2 className="w-4 h-4 mr-1" /> Delete
            </Button>
          )}
        </div>
      </div>
    </div>
  );
}

export default function ProfilePage() {
  return (
    <Suspense fallback={<div className="p-6 text-gray-500">Loading…</div>}>
      <ProfileEditor />
    </Suspense>
  );
}
