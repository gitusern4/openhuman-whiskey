/**
 * Whiskey fork — TradingView Desktop CDP bridge (route wrapper).
 *
 * The canonical UX is now via TK's Mods → TradingView bridge section.
 * This route (/tradingview-bridge) is kept for backward compatibility.
 * The inner logic lives in TvBridgePanelBody.
 *
 * → moved to TK's Mods (canonical home for all trading features).
 */
import SettingsHeader from '../components/SettingsHeader';
import { useSettingsNavigation } from '../hooks/useSettingsNavigation';
import TvBridgePanelBody from './TvBridgePanelBody';

const TradingViewBridgePanel = () => {
  const { navigateBack, breadcrumbs } = useSettingsNavigation();

  return (
    <div className="flex h-full w-full flex-col bg-stone-50">
      <SettingsHeader breadcrumbs={breadcrumbs} onBack={navigateBack} title="TradingView bridge" />
      <div className="flex-1 space-y-4 overflow-y-auto p-6">
        <TvBridgePanelBody />
      </div>
    </div>
  );
};

export default TradingViewBridgePanel;
