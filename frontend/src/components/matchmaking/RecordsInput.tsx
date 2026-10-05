'use client';

import React from 'react';
import { Plus, X } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { FieldDef, RecordValue, Value, kindName, kindOptions } from '@/types/matchmaking';

interface Props {
  defs: FieldDef[];
  entries: RecordValue[];
  entryLabel: string;
  onChange: (next: RecordValue[] | null) => void;
}

/** Editor for a repeating group of sub-fields, e.g. one card per child. */
export function RecordsInput({ defs, entries, entryLabel, onChange }: Props) {
  const emit = (next: RecordValue[]) => onChange(next.length ? next : null);

  const setSub = (i: number, key: string, v: Value | null) => {
    const next = entries.map((e, j) => {
      if (j !== i) return e;
      const copy: RecordValue = { ...e };
      if (v === null) delete copy[key];
      else copy[key] = v;
      return copy;
    });
    emit(next);
  };

  return (
    <div className="space-y-3">
      {entries.map((entry, i) => (
        <div key={i} className="border rounded-md p-3 bg-gray-50 space-y-2">
          <div className="flex items-center justify-between">
            <span className="text-sm font-medium">
              {entryLabel} {i + 1}
            </span>
            <button
              type="button"
              className="p-1 rounded hover:bg-gray-200"
              aria-label={`Remove ${entryLabel} ${i + 1}`}
              onClick={() => emit(entries.filter((_, j) => j !== i))}
            >
              <X className="w-4 h-4" />
            </button>
          </div>
          <div className="grid grid-cols-2 gap-3">
            {defs.map((d) => {
              const kind = kindName(d.kind);
              const v = entry[d.key];
              return (
                <label key={d.key} className="text-xs text-gray-600 space-y-1 block">
                  <span>{d.label}</span>
                  {kind === 'number' && (
                    <Input
                      type="number"
                      value={typeof v === 'number' ? String(v) : ''}
                      onChange={(e) => setSub(i, d.key, e.target.value === '' ? null : Number(e.target.value))}
                    />
                  )}
                  {kind === 'text' && (
                    <Input
                      value={typeof v === 'string' ? v : ''}
                      onChange={(e) => setSub(i, d.key, e.target.value === '' ? null : e.target.value)}
                    />
                  )}
                  {kind === 'bool' && (
                    <select
                      className="border rounded-md px-2 py-2 text-sm bg-white w-full"
                      value={typeof v === 'boolean' ? String(v) : ''}
                      onChange={(e) => setSub(i, d.key, e.target.value === '' ? null : e.target.value === 'true')}
                    >
                      <option value="">not set</option>
                      <option value="true">yes</option>
                      <option value="false">no</option>
                    </select>
                  )}
                  {kind === 'choice' && (
                    <select
                      className="border rounded-md px-2 py-2 text-sm bg-white w-full"
                      value={typeof v === 'string' ? v : ''}
                      onChange={(e) => setSub(i, d.key, e.target.value === '' ? null : e.target.value)}
                    >
                      <option value="">not set</option>
                      {kindOptions(d.kind).map((o) => (
                        <option key={o} value={o}>{o.replace(/_/g, ' ')}</option>
                      ))}
                    </select>
                  )}
                </label>
              );
            })}
          </div>
        </div>
      ))}
      <Button type="button" variant="outline" size="sm" onClick={() => emit([...entries, {}])}>
        <Plus className="w-4 h-4 mr-1" /> Add {entryLabel.toLowerCase()}
      </Button>
    </div>
  );
}
