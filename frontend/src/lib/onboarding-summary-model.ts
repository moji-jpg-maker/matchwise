interface OnboardingSummaryModelStatusInput {
  selectedModel: string;
  recommendedModel: string;
  selectedModelReady: boolean;
}

interface OnboardingSummaryModelStatus {
  selectedSummaryModel: string;
  summaryModelDownloaded: boolean;
}

const SUMMARY_MODEL_SIZES_MB: Record<string, number> = {
  'qwen3.5:2b': 1221,
  'qwen3.5:4b': 2614,
  'gemma3:1b': 1019,
  'gemma3:4b': 2374,
};

export function resolveOnboardingSummaryModelStatus({
  selectedModel,
  recommendedModel,
  selectedModelReady,
}: OnboardingSummaryModelStatusInput): OnboardingSummaryModelStatus {
  const selectedSummaryModel = selectedModel || recommendedModel;

  return {
    selectedSummaryModel,
    summaryModelDownloaded: Boolean(selectedSummaryModel && selectedModelReady),
  };
}

export function getSummaryModelSizeMb(model: string): number {
  return SUMMARY_MODEL_SIZES_MB[model] ?? 0;
}

export function getDownloadTotalMb(totalMb: number | null | undefined, model: string): number {
  return totalMb || getSummaryModelSizeMb(model);
}

export interface SizeLabelFormatters {
  t: (key: 'builtinMgr.sizeMib' | 'builtinMgr.sizeGib', params?: Record<string, string | number>) => string;
  number: (value: number, options?: Intl.NumberFormatOptions) => string;
}

export function formatSummaryModelSizeLabelFromMb(sizeMb: number, formatters?: SizeLabelFormatters): string {
  if (sizeMb === 0) {
    return '';
  }

  if (sizeMb >= 1024) {
    if (formatters) {
      const value = formatters.number(sizeMb / 1024, { maximumFractionDigits: 1, minimumFractionDigits: 1 });
      return formatters.t('builtinMgr.sizeGib', { value });
    }
    return `~${(sizeMb / 1024).toFixed(1)} GiB`;
  }

  if (formatters) {
    return formatters.t('builtinMgr.sizeMib', { value: formatters.number(sizeMb) });
  }
  return `~${sizeMb} MiB`;
}

export function getSummaryModelSizeLabel(model: string): string {
  return formatSummaryModelSizeLabelFromMb(getSummaryModelSizeMb(model));
}
