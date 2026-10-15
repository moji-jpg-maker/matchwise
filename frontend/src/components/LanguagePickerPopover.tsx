"use client";

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { LANGUAGE_OPTIONS } from "@/lib/summary-languages";
import { useRecentLanguages } from "@/hooks/useRecentLanguages";
import { useI18n } from "@/i18n";
import { normalizeSearch } from "@/i18n/format";

interface LanguagePickerPopoverProps {
  value: string | null;
  onChange: (code: string | null) => void;
  onClose: () => void;
  mode?: "meeting" | "settings";
  autoSubtitle?: string;
}

export function LanguagePickerPopover({
  value,
  onChange,
  onClose,
  mode = "meeting",
  autoSubtitle,
}: LanguagePickerPopoverProps) {
  const { recents } = useRecentLanguages();
  const { t, fmt } = useI18n();
  const [query, setQuery] = useState("");
  const containerRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  useEffect(() => {
    const onDocClick = (e: MouseEvent) => {
      if (!containerRef.current?.contains(e.target as Node)) onClose();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    document.addEventListener("mousedown", onDocClick);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDocClick);
      document.removeEventListener("keydown", onKey);
    };
  }, [onClose]);

  const filter = normalizeSearch(query.trim());

  const matchesFilter = useCallback(
    (option: (typeof LANGUAGE_OPTIONS)[number]) =>
      option.code.toLowerCase().includes(filter) ||
      normalizeSearch(option.label).includes(filter) ||
      normalizeSearch(fmt.languageName(option.code)).includes(filter),
    [filter, fmt],
  );

  const recentCodes = useMemo(() => new Set(recents), [recents]);

  const filteredAll = useMemo(() => {
    const options = mode === "meeting"
      ? LANGUAGE_OPTIONS.filter((l) => !recentCodes.has(l.code))
      : LANGUAGE_OPTIONS;
    if (!filter) return options;
    return options.filter(matchesFilter);
  }, [filter, mode, recentCodes, matchesFilter]);

  const recentsResolved = useMemo(
    () =>
      recents
        .map((code) => LANGUAGE_OPTIONS.find((l) => l.code === code))
        .filter((l): l is (typeof LANGUAGE_OPTIONS)[number] => Boolean(l))
        .filter((l) => !filter || matchesFilter(l)),
    [recents, filter, matchesFilter],
  );

  const showAuto = mode === "meeting" && (!filter || "auto".includes(filter) || normalizeSearch(t("langPicker.auto")).includes(filter));
  const showRecents = mode === "meeting" && recentsResolved.length > 0;
  const hasNoResults =
    filteredAll.length === 0 && recentsResolved.length === 0 && !showAuto;

  return (
    <div
      ref={containerRef}
      className="w-72 rounded-lg bg-white border border-gray-200 shadow-lg overflow-hidden"
      role="dialog"
      aria-label={t("langPicker.aria")}
    >
      <div className="flex items-center gap-2 px-3 py-2.5 border-b border-gray-100">
        <span className="text-gray-400 text-sm">🔍</span>
        <input
          ref={inputRef}
          type="text"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder={t("langPicker.search")}
          className="flex-1 text-sm text-gray-900 bg-transparent border-none outline-none placeholder-gray-400"
        />
      </div>

      <div className="max-h-80 overflow-y-auto py-1">
        {showRecents && (
          <>
            <div className="px-3 pt-1 pb-1 text-[10px] font-semibold uppercase tracking-wider text-gray-400">
              {t("langPicker.recent")}
            </div>
            {recentsResolved.map((opt) => (
              <button
                key={`recent-${opt.code}`}
                type="button"
                aria-pressed={value === opt.code}
                onClick={() => onChange(opt.code)}
                className={`flex w-full items-center justify-between px-3 py-1.5 text-sm hover:bg-gray-50 text-start ${
                  value === opt.code ? "text-blue-600 font-medium" : "text-gray-800"
                }`}
              >
                <span>
                  <bdi>{fmt.languageName(opt.code)}</bdi>{" "}
                  <span dir="ltr" className="text-xs text-gray-400">({opt.code})</span>
                </span>
                {value === opt.code && <span className="text-blue-600" aria-hidden="true">✓</span>}
              </button>
            ))}
            <div className="my-1 h-px bg-gray-100" />
          </>
        )}

        {showAuto && (
          <button
            type="button"
            aria-pressed={value === null}
            onClick={() => onChange(null)}
            className={`flex w-full items-center justify-between px-3 py-1.5 text-sm hover:bg-gray-50 text-start ${
              value === null ? "text-blue-600 font-medium" : "text-gray-800"
            }`}
          >
            <span className="flex flex-col">
              <span>{t("langPicker.auto")}</span>
              {autoSubtitle && (
                <span className="text-xs font-normal text-gray-400">{autoSubtitle}</span>
              )}
            </span>
            {value === null && <span className="text-blue-600" aria-hidden="true">✓</span>}
          </button>
        )}

        {filteredAll.length > 0 && (
          <div className="px-3 pt-1 pb-1 text-[10px] font-semibold uppercase tracking-wider text-gray-400">
            {mode === "meeting" ? t("langPicker.other") : t("langPicker.all")}
          </div>
        )}

        {filteredAll.map((opt) => (
          <button
            key={`all-${opt.code}`}
            type="button"
            aria-pressed={value === opt.code}
            onClick={() => onChange(opt.code)}
            className={`flex w-full items-center justify-between px-3 py-1.5 text-sm hover:bg-gray-50 text-start ${
              value === opt.code ? "text-blue-600 font-medium" : "text-gray-800"
            }`}
          >
            <span>
              <bdi>{fmt.languageName(opt.code)}</bdi>{" "}
              <span dir="ltr" className="text-xs text-gray-400">({opt.code})</span>
            </span>
            {value === opt.code && <span className="text-blue-600" aria-hidden="true">✓</span>}
          </button>
        ))}

        {hasNoResults && (
          <div className="px-3 py-2 text-sm text-gray-400">{t("langPicker.none")}</div>
        )}
      </div>
    </div>
  );
}
