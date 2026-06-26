import './index.css';
import { useState } from 'react';
import { ChatView } from './components/ChatView';
import { MLPredictorPanel } from './components/MLPredictorPanel';
import { PrizePicksPredictionsPanel } from './components/PrizePicksPredictionsPanel';
import { PrizePicksView } from './components/PrizePicksView';
import { PropsView } from './components/PropsView';
import { SettingsView } from './components/SettingsView';

type Tab = 'props' | 'prizepicks' | 'chat' | 'predictions' | 'ml' | 'settings';

export default function App() {
  const [activeTab, setActiveTab] = useState<Tab>('props');

  return (
    <div className="appShell">
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
          { id: 'settings', label: '⚙️ Settings' },
        ].map((tab) => (
          <button
            key={tab.id}
            className={`navButton ${activeTab === tab.id ? 'active' : ''}`}
            onClick={() => setActiveTab(tab.id as Tab)}
          >
            {tab.label}
          </button>
        ))}
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
        {activeTab === 'settings' && <SettingsView />}
      </main>
    </div>
  );
}
