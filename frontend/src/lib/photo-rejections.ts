import type { RejectedPhoto } from '@/types/matchmaking';
import type { Translate } from '@/lib/conditions';

const REASON_KEY = {
  not_image: 'photosPanel.reasonNotImage',
  too_large: 'photosPanel.reasonTooLarge',
  duplicate: 'photosPanel.reasonDuplicate',
  limit: 'photosPanel.reasonLimit',
  unreadable: 'photosPanel.reasonUnreadable',
} as const;

/** One line per refused file, in the interface language. Unknown codes fall back to naming the file only. */
export function describeRejections(rejected: RejectedPhoto[], t: Translate): string {
  return rejected
    .map((r) => {
      const key = REASON_KEY[r.reason as keyof typeof REASON_KEY];
      return key ? t(key, { name: r.name }) : t('photosPanel.reasonOther', { name: r.name });
    })
    .join(' · ');
}
