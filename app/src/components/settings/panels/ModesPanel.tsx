/**
 * Whiskey fork — agent-mode picker (route wrapper).
 *
 * The canonical UX is now via TK's Mods → AI Mode section. This route
 * (/modes) is kept for backward compatibility — description updated to
 * direct users to TK's Mods. The inner logic lives in ModesPanelBody.
 *
 * → moved to TK's Mods (canonical home for all trading features).
 */
import SettingsHeader from '../components/SettingsHeader';
import { useSettingsNavigation } from '../hooks/useSettingsNavigation';
import ModesPanelBody from './ModesPanelBody';

export type { ModeDescriptor } from './ModesPanelBody';

const ModesPanel = () => {
  const { navigateBack, breadcrumbs } = useSettingsNavigation();

  return (
    <div className="z-10 relative" data-testid="modes-panel-root">
      <SettingsHeader
        title="Modes"
        showBackButton={true}
        onBack={navigateBack}
        breadcrumbs={breadcrumbs}
      />
      <div className="p-4 space-y-4">
        <ModesPanelBody />
      </div>
    </div>
  );
};

export default ModesPanel;
