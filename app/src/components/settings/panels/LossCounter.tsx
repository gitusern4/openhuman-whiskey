/**
 * LossCounter — shows consecutive losses vs. max allowed.
 * Turns red at 2 consecutive losses, locked style at max.
 */
import type { SessionState } from './types';

interface Props {
  session: SessionState;
  maxConsecutiveLosses: number;
}

export default function LossCounter({ session, maxConsecutiveLosses }: Props) {
  const { consecutive_losses } = session;
  const isAtMax = consecutive_losses >= maxConsecutiveLosses;
  const isWarning = consecutive_losses >= 2;

  const color = isAtMax ? '#dc2626' : isWarning ? '#ea580c' : '#16a34a';
  const label = isAtMax ? 'LOCKED' : `${consecutive_losses} / ${maxConsecutiveLosses}`;

  return (
    <div
      data-testid="loss-counter"
      style={{
        display: 'flex',
        alignItems: 'center',
        gap: 8,
        padding: '8px 12px',
        borderRadius: 8,
        background: isAtMax ? '#fee2e2' : isWarning ? '#ffedd5' : '#f0fdf4',
        border: `1px solid ${color}`,
      }}>
      <span style={{ color: '#6b7280', fontSize: '0.85rem' }}>Losses today</span>
      <span
        data-testid="loss-counter-value"
        style={{ fontWeight: 700, color, fontSize: '1rem', marginLeft: 'auto' }}>
        {label}
      </span>
    </div>
  );
}
