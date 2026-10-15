'use client';

import { useEffect } from 'react';
import { useRouter } from 'next/navigation';

export default function AiRedirectPage() {
  const router = useRouter();
  useEffect(() => { router.replace('/settings?tab=ai'); }, [router]);
  return null;
}
