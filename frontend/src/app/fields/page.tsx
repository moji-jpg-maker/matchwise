'use client';

import React, { useCallback, useEffect, useState } from 'react';
import { useRouter } from 'next/navigation';
import { invoke } from '@tauri-apps/api/core';
import { ArrowLeft, Plus } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { FieldDef, FieldKind, kindName, kindOptions } from '@/types/matchmaking';

type SimpleKind = 'bool' | 'number' | 'text' | 'choice' | 'multi_choice';

interface Draft {
  key: string;
  label: string;
  kind: SimpleKind;
  options: string;
  sensitive: boolean;
  required: boolean;
  existing: boolean;
}

const EMPTY: Draft = { key: '', label: '', kind: 'text', options: '', sensitive: false, required: false, existing: false };

function buildKind(draft: Draft): FieldKind {
  const options = draft.options.split(',').map((o) => o.trim()).filter((o) => o !== '');
  if (draft.kind === 'choice') return { choice: options };
  if (draft.kind === 'multi_choice') return { multi_choice: options };
  return draft.kind;
}

export default function FieldsPage() {
  const router = useRouter();
  const [fields, setFields] = useState<FieldDef[]>([]);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [filter, setFilter] = useState('');
  const [draft, setDraft] = useState<Draft | null>(null);
  const [saving, setSaving] = useState(false);

  const load = useCallback(async () => {
    setLoading(true);
    setLoadError(null);
    try {
      setFields(await invoke<FieldDef[]>('mm_list_fields'));
    } catch (e) {
      setLoadError(`${e}`);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => { void load(); }, [load]);

  const edit = (f: FieldDef) => {
    const kind = kindName(f.kind);
    if (kind === 'records') {
      toast.message('Record fields cannot be edited here');
      return;
    }
    setDraft({
      key: f.key,
      label: f.label,
      kind,
      options: kindOptions(f.kind).join(', '),
      sensitive: f.sensitive,
      required: f.required,
      existing: true,
    });
  };

  const save = async () => {
    if (!draft) return;
    setSaving(true);
    try {
      await invoke('mm_save_field', {
        def: {
          key: draft.key.trim(),
          label: draft.label.trim(),
          kind: buildKind(draft),
          sensitive: draft.sensitive,
          required: draft.required,
        },
      });
      toast.success('Field saved');
      setDraft(null);
      await load();
    } catch (e) {
      toast.error(`${e}`);
    } finally {
      setSaving(false);
    }
  };

  const needle = filter.trim().toLowerCase();
  const visible = needle
    ? fields.filter((f) => f.key.toLowerCase().includes(needle) || f.label.toLowerCase().includes(needle))
    : fields;
  const hasOptions = draft?.kind === 'choice' || draft?.kind === 'multi_choice';

  return (
    <div className="min-h-screen bg-gray-50">
      <div className="max-w-3xl mx-auto p-6 space-y-5">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-3">
            <button onClick={() => router.push('/profiles')} className="p-2 rounded-lg hover:bg-gray-200" aria-label="Back">
              <ArrowLeft className="w-5 h-5" />
            </button>
            <div>
              <h1 className="text-2xl font-semibold">Fields</h1>
              <p className="text-sm text-gray-500">What a profile records. A key cannot be renamed once created.</p>
            </div>
          </div>
          <Button onClick={() => setDraft({ ...EMPTY })}>
            <Plus className="w-4 h-4 mr-1" /> New field
          </Button>
        </div>

        <Input placeholder="Search fields" value={filter} onChange={(e) => setFilter(e.target.value)} />

        {draft && (
          <div className="bg-white rounded-lg border p-4 space-y-3">
            <h2 className="font-medium">{draft.existing ? `Edit ${draft.key}` : 'New field'}</h2>
            <div className="grid grid-cols-2 gap-3">
              <label className="text-sm space-y-1">
                <span className="font-medium">Key</span>
                <Input
                  value={draft.key}
                  disabled={draft.existing}
                  placeholder="lowercase_with_underscores"
                  onChange={(e) => setDraft({ ...draft, key: e.target.value })}
                />
              </label>
              <label className="text-sm space-y-1">
                <span className="font-medium">Label</span>
                <Input value={draft.label} onChange={(e) => setDraft({ ...draft, label: e.target.value })} />
              </label>
              <label className="text-sm space-y-1">
                <span className="font-medium">Type</span>
                <select
                  className="w-full border rounded h-9 px-2 text-sm bg-white"
                  value={draft.kind}
                  onChange={(e) => setDraft({ ...draft, kind: e.target.value as SimpleKind })}
                >
                  <option value="text">Text</option>
                  <option value="number">Number</option>
                  <option value="bool">Yes / no</option>
                  <option value="choice">Single choice</option>
                  <option value="multi_choice">Multiple choice</option>
                </select>
              </label>
              {hasOptions && (
                <label className="text-sm space-y-1">
                  <span className="font-medium">Options (comma separated)</span>
                  <Input value={draft.options} onChange={(e) => setDraft({ ...draft, options: e.target.value })} />
                </label>
              )}
            </div>
            <div className="flex gap-6 text-sm">
              <label className="flex items-center gap-2">
                <input type="checkbox" checked={draft.required} onChange={(e) => setDraft({ ...draft, required: e.target.checked })} />
                Counts toward completeness
              </label>
              <label className="flex items-center gap-2">
                <input type="checkbox" checked={draft.sensitive} onChange={(e) => setDraft({ ...draft, sensitive: e.target.checked })} />
                Sensitive
              </label>
            </div>
            <div className="flex gap-2">
              <Button onClick={save} disabled={saving}>{saving ? 'Saving…' : 'Save'}</Button>
              <Button variant="ghost" onClick={() => setDraft(null)} disabled={saving}>Cancel</Button>
            </div>
          </div>
        )}

        {loading ? (
          <div className="text-sm text-gray-500">Loading…</div>
        ) : loadError ? (
          <div className="bg-white rounded-lg border p-6 text-center space-y-2">
            <p className="text-sm text-red-700">Could not load fields: {loadError}</p>
            <Button variant="outline" onClick={() => void load()}>Retry</Button>
          </div>
        ) : (
          <div className="bg-white rounded-lg border divide-y">
            {visible.length === 0 && (
              <div className="p-6 text-center text-sm text-gray-500">
                {fields.length === 0 ? 'No fields yet.' : 'No fields match your search.'}
              </div>
            )}
            {visible.map((f) => (
              <div key={f.key} className="p-4 flex items-center gap-4">
                <div className="flex-1 min-w-0">
                  <div className="font-medium">{f.label}</div>
                  <div className="text-xs text-gray-500">
                    {f.key} · {kindName(f.kind)}
                    {f.required ? ' · counts toward completeness' : ''}
                    {f.sensitive ? ' · sensitive' : ''}
                  </div>
                </div>
                <Button variant="ghost" size="sm" onClick={() => edit(f)}>Edit</Button>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
