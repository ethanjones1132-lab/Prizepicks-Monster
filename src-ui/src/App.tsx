import './index.css';
import { useCallback, useEffect, useState } from 'react';
import { ChatView } from './components/ChatView';
import { LogViewer } from './components/LogViewer';
import { MLPredictorPanel } from './components/MLPredictorPanel';
import { NotificationCenter } from './components/NotificationCenter';
import { PrizePicksPredictionsPanel } from './components/PrizePicksPredictionsPanel';
import { PrizePicksView } from './components/PrizePicksView';
import { PropsView } from './components/PropsView';
import { SettingsView } from './components/SettingsView';
import { WhatsNewModal, hasUnseenWhatsNew } from './components/WhatsNewModal';
import { WHATS_NEW_STORAGE_KEY } from './data/whatsNewData';
import { prizepicksApi } from './services/prizepicks';

type Tab = 'props' | 'prizepicks' | 'chat' | 'predictions' | 'ml' | 'logs' | 'notifications' | 'settings';

export default function App() {
  const [activeTab, setActiveTab] = useState<Tab>('props');
  const [unreadCount, setUnreadCount] = useState(0);
  const [showWhatsNew, setShowWhatsNew] = useState(false);
  const [unseenWhatsNew, setUnseenWhatsNew] = useState(() => {
    try {
      return hasUnseenWhatsNew(localStorage.getItem(WHATS_NEW_STORAGE_KEY));
    } catch {
      return true;
    }
  });

  const fetchUnread = useCallback(async () => {
    try {
      const count = await prizepicksApi.getUnreadNotificationCount();
      setUnreadCount(count);
    } catch {
      // Silently fail — the notification center will show errors
    }
  }, []);

  useEffect(() => {
    fetchUnread();
    const interval = setInterval(fetchUnread, 30_000);
    return () => clearInterval(interval);
  }, [fetchUnread]);

  // Reset unread poll when the notification tab is opened
  useEffect(() => {
    if (activeTab === 'notifications') {
      fetchUnread();
    }
  }, [activeTab, fetchUnread]);

  const handleWhatsNewOpen = () => {
    setShowWhatsNew(true);
  };

  const handleWhatsNewClose = () => {
    setShowWhatsNew(false);
    setUnseenWhatsNew(false);
    try {
      localStorage.setItem(WHATS_NEW_STORAGE_KEY, '2026-07-17');
    } catch {
      // silently ignore
    }
  };

  return (
    <div className="appShell">
      {showWhatsNew && <WhatsNewModal onClose={handleWhatsNewClose} />}

      <aside className="sidebar">
        <div className="brand">
          <div className="logo">PP</div>
          <div>
            <strong>PrizePicks Monster</strong>
            <span>DFS player prop intelligence</span>
          </div>
        </div>

        {[
          { id: 'props', label: '🎯 Prop board' },
          { id: 'prizepicks', label: '📊 PrizePicks dashboard' },
          { id: 'chat', label: '🧠 Analyst chat' },
          { id: 'predictions', label: '📈 Prediction log' },
          { id: 'ml', label: '🤖 ML predictor' },
          { id: 'logs', label: '🪵 Logs' },
          { id: 'notifications', label: '🔔 Notifications', badge: unreadCount },
          { id: 'settings', label: '⚙️ Settings' },
        ].map((tab) => (
          <button
            key={tab.id}
            className={`navButton ${activeTab === tab.id ? 'active' : ''}`}
            onClick={() => setActiveTab(tab.id as Tab)}
          >
            <span>{tab.label}</span>
            {tab.badge !== undefined && tab.badge > 0 && (
              <span className="navBadge">{tab.badge > 99 ? '99+' : tab.badge}</span>
            )}
          </button>
        ))}

        <button
          className="navButton whatsnewNavBtn"
          onClick={handleWhatsNewOpen}
          title="What's New in PrizePicks Monster"
        >
          <span>🎉 What's New</span>
          {unseenWhatsNew && <span className="whatsnewNavDot" />}
        </button>
      </aside>

      <main className="main">
        {activeTab === 'props' && <PropsView />}
        {activeTab === 'prizepicks' && <PrizePicksView />}
        {activeTab === 'chat' && <ChatView />}
        {activeTab === 'predictions' && (
          <section className="page prizepicksPage">
            <header className="prizepicksHeader">
              <div>
                <h2>Prediction log</h2>
                <p className="muted">Player prop picks with Over/Under grading and PnL tracking.</p>
              </div>
            </header>
            <PrizePicksPredictionsPanel />
          </section>
        )}
        {activeTab === 'ml' && (
          <section className="page prizepicksPage">
            <header className="prizepicksHeader">
              <div>
                <h2>ML predictor</h2>
                <p className="muted">
                  Scikit-learn GradientBoosting trained on resolved predictions
                  with line-movement features. Requires ≥10 resolved props.
                </p>
              </div>
            </header>
            <MLPredictorPanel />
          </section>
        )}
        {activeTab === 'logs' && <LogViewer />}
        {activeTab === 'notifications' && <NotificationCenter />}
        {activeTab === 'settings' && <SettingsView />}
      </main>
    </div>
  );
}
