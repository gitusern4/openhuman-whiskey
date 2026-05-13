/**
 * Shared types for the Whiskey execution layer UI.
 * Mirrors the Rust structs in execution_commands.rs.
 */

export interface SessionState {
  daily_pnl: number;
  session_loss_count: number;
  consecutive_losses: number;
  kill_engaged: boolean;
  walk_away_active: boolean;
  walk_away_ends_at: number | null;
}

export interface ProposalShape {
  proposal_hash: string;
  instrument: string;
  action: string;
  qty: number;
  entry_price: number | null;
  stop_loss_ticks: number;
  take_profit_ticks: number;
  r_estimate_dollars: number;
  confidence_pct: number;
  playbook_match_id: string | null;
  countdown_seconds: number;
}
