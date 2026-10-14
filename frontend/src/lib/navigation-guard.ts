let message: string | null = null;

export function setNavigationGuard(next: string | null) {
  message = next;
}

export function confirmNavigation(): boolean {
  if (message === null) return true;
  return window.confirm(message);
}
