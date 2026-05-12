/**
 * DailyPnLGauge — horizontal bar showing today's P&L vs daily max loss.
 * Orange at 60%, red at 80%, locked at 100%.
 * Not collapsible — always visible per §8 of the research doc.
 */
interface Props {
  dailyPnl: number;
  dailyMaxLossUsd: number;
}

export default function DailyPnLGauge({ dailyPnl, dailyMaxLossUsd }: Props) {
  // P&L is negative when losing; we compute how much of the daily budget is consumed.
  const consumed = Math.max(0, -dailyPnl);
  const pct = dailyMaxLossUsd > 0 ? Math.min(consumed / dailyMaxLossUsd, 1) : 0;
  const pctDisplay = Math.round(pct * 100);

  const barColor =
    pct >= 1.0 ? '#dc2626' : pct >= 0.8 ? '#dc2626' : pct >= 0.6 ? '#ea580c' : '#16a34a';

  const locked = pct >= 1.0;

  return (
    <div data-testid="daily-pnl-gauge" style={{ padding: '8px 12px' }}>
      <div
        style={{
          display: 'flex',
          justifyContent: 'space-between',
          fontSize: '0.82rem',
          marginBottom: 4,
        }}
      >
        <span style={{ color: '#6b7280' }}>Daily P&amp;L</span>
        <span
          data-testid="daily-pnl-value"
          style={{ fontWeight: 600, color: dailyPnl < 0 ? '#dc2626' : '#16a34a' }}
        >
          ${dailyPnl.toFixed(2)} / -${dailyMaxLossUsd.toFixed(0)}
          {locked && (
            <span
              data-testid="daily-pnl-locked"
              style={{ marginLeft: 6, color: '#dc2626', fontWeight: 700 }}
            >
              LOCKED
            </span>
          )}
        </span>
      </div>
      <div
        style={{
          height: 8,
          borderRadius: 4,
          background: '#e5e7eb',
          overflow: 'hidden',
        }}
      >
        <div
          data-testid="daily-pnl-bar"
          style={{
            height: '100%',
            width: `${pctDisplay}%`,
            background: barColor,
            transition: 'width 0.3s ease',
            borderRadius: 4,
          }}
        />
      </div>
    </div>
  );
}
