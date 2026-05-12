/**
 * WalkAwayLockout — 5-minute UI lockout when triggered by a single loss
 * exceeding 0.75R of the daily budget.
 *
 * Triggers: single loss > walk_away_trigger_loss_fraction * daily_max_loss_usd.
 * Renders a full-panel overlay with a countdown. Order entry is blocked.
 */
import { useEffect, useState } from 'react';

interface Props {
  active: boolean;
  endsAtUnix: number | null;
}

function formatRemaining(seconds: number): string {
  const m = Math.floor(seconds / 60);
  const s = seconds % 60;
  return `${m}:${String(s).padStart(2, '0')}`;
}

export default function WalkAwayLockout({ active, endsAtUnix }: Props) {
  const [remaining, setRemaining] = useState<number>(0);

  useEffect(() => {
    if (!active || endsAtUnix === null) {
      setRemaining(0);
      return;
    }
    const update = () => {
      const now = Math.floor(Date.now() / 1000);
      setRemaining(Math.max(0, endsAtUnix - now));
    };
    update();
    const id = setInterval(update, 1000);
    return () => clearInterval(id);
  }, [active, endsAtUnix]);

  if (!active) return null;

  return (
    <div
      data-testid="walk-away-lockout"
      role="alert"
      aria-live="polite"
      style={{
        background: '#fef2f2',
        border: '2px solid #dc2626',
        borderRadius: 10,
        padding: 16,
        textAlign: 'center',
        marginBottom: 12,
      }}>
      <div style={{ fontWeight: 700, color: '#dc2626', fontSize: '1rem', marginBottom: 4 }}>
        Walk Away — Take a Break
      </div>
      <div style={{ color: '#6b7280', fontSize: '0.85rem', marginBottom: 8 }}>
        A single loss exceeded your walk-away threshold. Step back and reset.
      </div>
      {remaining > 0 && (
        <div
          data-testid="walk-away-countdown"
          style={{
            fontWeight: 700,
            fontSize: '2rem',
            color: '#dc2626',
            fontVariantNumeric: 'tabular-nums',
          }}>
          {formatRemaining(remaining)}
        </div>
      )}
      {remaining <= 0 && (
        <div data-testid="walk-away-done" style={{ fontWeight: 600, color: '#16a34a' }}>
          Lockout complete — proceed with intention.
        </div>
      )}
    </div>
  );
}
