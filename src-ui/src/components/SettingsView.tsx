import { useCallback, useEffect, useState } from 'react';
import { bankrollApi, configApi } from '../services/tauri';
import type { ApiStatus, AppConfig, BankrollConfig, ModelInfo } from '../types';
import { prizepicksApi } from '../services/prizepicks';
import type { NotificationSettings } from '../types/prizepicks';

const EMPTY_CONFIG: AppConfig = {
  openrouter_api_key: '',
  openrouter_base_url: 'https://openrouter.ai/api/v1',
  selected_model: 'nvidia/nemotron-3-super-120b-a12b:free',
  system_prompt: '',
  max_context_players: 50,
  openweathermap_api_key: '',
  api_sports_key: '',
  opticodds_api_key: '',
  odds_api_key: '',
  risk_tolerance: 'moderate',
  preferred_leagues: ['NFL'],
  stat_weighting: 'balanced',
  output_format: 'json_plus_text',
  theme: 'dark',
  prizepicks_email: '',
  prizepicks_password: '',
  prizepicks_poll_interval_secs: 60,
  discord_webhook_url: '',
  telegram_bot_token: '',
  telegram_chat_id: '',
  bot_daily_picks_enabled: true,
  bot_game_alerts_enabled: true,
  bot_grading_results_enabled: true,
  bot_daily_picks_time: '08:00',
};

const EMPTY_BANKROLL_CONFIG: BankrollConfig = {
  total_bankroll: 1000,
  initial_bankroll: 1000,
  kelly_fraction: 0.25,
  max_bet_pct: 0.05,
  min_bet: 5,
  default_odds: -110,
  strategy: 'Kelly',
  player_risk_multipliers: {},
  daily_bet_limit: 200,
  weekly_bet_limit: 500,
};

const DEFAULT_NOTIFICATION_SETTINGS: NotificationSettings = {
  enabled: true,
  game_starting_enabled: true,
  game_final_enabled: true,
  prediction_graded_enabled: true,
  grading_complete_enabled: true,
  poll_interval_secs: 60,
  game_starting_minutes_before: 30,
  show_os_notifications: true,
};

function maskSecret(value: string): string {
  if (!value) return '';
  if (value.length <= 8) return '••••••••';
  return `${value.slice(0, 4)}…${value.slice(-4)}`;
}

export function SettingsView() {
  const [config, setConfig] = useState<AppConfig>(EMPTY_CONFIG);
  const [bankrollConfig, setBankrollConfig] =
    useState<BankrollConfig>(EMPTY_BANKROLL_CONFIG);
  const [models, setModels] = useState<ModelInfo[]>([]);
  const [apiKeyInput, setApiKeyInput] = useState('');
  const [prizepicksPasswordInput, setPrizePicksPasswordInput] = useState('');
  const [discordInput, setDiscordInput] = useState('');
  const [telegramTokenInput, setTelegramTokenInput] = useState('');
  const [leaguesInput, setLeaguesInput] = useState('NFL');
  const [apiStatus, setApiStatus] = useState<ApiStatus | null>(null);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [savingBankroll, setSavingBankroll] = useState(false);
  const [testing, setTesting] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [bankrollMessage, setBankrollMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notifSettings, setNotifSettings] = useState<NotificationSettings>(DEFAULT_NOTIFICATION_SETTINGS);
  const [savingNotif, setSavingNotif] = useState(false);
  const [notifMessage, setNotifMessage] = useState<string | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    setBankrollMessage(null);
    try {
      const [cfg, modelList, bankroll, notif] = await Promise.all([
        configApi.get(),
        configApi.getAvailableModels(),
        bankrollApi.getConfig(),
        prizepicksApi.getNotificationSettings(),
      ]);
      setConfig(cfg);
      setBankrollConfig(bankroll);
      setModels(modelList);
      setLeaguesInput(cfg.preferred_leagues.join(', '));
      setApiKeyInput('');
      setPrizePicksPasswordInput('');
      setDiscordInput('');
      setTelegramTokenInput('');
      setNotifSettings(notif);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const handleSave = async () => {
    setSaving(true);
    setMessage(null);
    setError(null);
    try {
      const next: AppConfig = {
        ...config,
        openrouter_api_key: apiKeyInput.trim() || config.openrouter_api_key,
        prizepicks_password: prizepicksPasswordInput.trim() || config.prizepicks_password,
        discord_webhook_url: discordInput.trim() || config.discord_webhook_url,
        telegram_bot_token: telegramTokenInput.trim() || config.telegram_bot_token,
        preferred_leagues: leaguesInput
          .split(',')
          .map((s) => s.trim())
          .filter(Boolean),
      };
      await configApi.save(next);
      setConfig(next);
      setApiKeyInput('');
      setPrizePicksPasswordInput('');
      setDiscordInput('');
      setTelegramTokenInput('');
      setMessage('Settings saved.');
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setSaving(false);
    }
  };

  const handleSaveBankroll = async () => {
    setSavingBankroll(true);
    setBankrollMessage(null);
    setError(null);
    try {
      await bankrollApi.saveConfig(bankrollConfig);
      setBankrollMessage('Bankroll controls saved.');
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setSavingBankroll(false);
    }
  };

  const handleSaveNotif = async () => {
    setSavingNotif(true);
    setNotifMessage(null);
    setError(null);
    try {
      await prizepicksApi.saveNotificationSettings(notifSettings);
      setNotifMessage('Notification settings saved.');
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setSavingNotif(false);
    }
  };

  const handleTestConnection = async () => {
    setTesting(true);
    setMessage(null);
    setError(null);
    try {
      if (apiKeyInput.trim()) {
        await configApi.save({ ...config, openrouter_api_key: apiKeyInput.trim() });
      }
      const status = await configApi.checkApiStatus();
      setApiStatus(status);
      if (status.connected) {
        setMessage(
          status.model_available
            ? 'OpenRouter connected — model available.'
            : 'OpenRouter connected — selected model may be unavailable.',
        );
      } else {
        setError(status.error ?? 'Connection failed.');
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setTesting(false);
    }
  };

  if (loading) {
    return (
      <section className="page">
        <div className="card">
          <div className="state">Loading settings…</div>
        </div>
      </section>
    );
  }

  return (
    <section className="page settingsPage">
      <header className="prizepicksHeader">
        <div>
          <h2>Settings</h2>
          <p className="muted">
            OpenRouter, model selection, risk controls, and notification hooks. Secrets are stored locally at{' '}
            <code>~/.openclaw/prizepicks-monster/config.json</code>.
          </p>
        </div>
        <div className="panelToolbar">
          <button type="button" className="ghostBtn" onClick={() => void load()}>
            Reload
          </button>
          <button type="button" className="primaryBtn" disabled={saving} onClick={() => void handleSave()}>
            {saving ? 'Saving…' : 'Save settings'}
          </button>
        </div>
      </header>

      {message && <div className="banner success">{message}</div>}
      {error && <div className="banner error">{error}</div>}

      <div className="settingsGrid">
        <div className="card">
          <h3>OpenRouter</h3>
          <div className="formGrid">
            <label>
              API key
              <input
                type="password"
                placeholder={config.openrouter_api_key ? maskSecret(config.openrouter_api_key) : 'sk-or-v1-…'}
                value={apiKeyInput}
                onChange={(e) => setApiKeyInput(e.target.value)}
                autoComplete="off"
              />
            </label>
            <label>
              Base URL
              <input
                value={config.openrouter_base_url}
                onChange={(e) => setConfig({ ...config, openrouter_base_url: e.target.value })}
              />
            </label>
            <label>
              Model
              <select
                value={config.selected_model}
                onChange={(e) => setConfig({ ...config, selected_model: e.target.value })}
              >
                {models.map((m) => (
                  <option key={m.id} value={m.id}>
                    {m.name} ({m.provider}) — {m.cost}
                  </option>
                ))}
              </select>
            </label>
            <label>
              Max context players
              <input
                type="number"
                min={10}
                max={200}
                value={config.max_context_players}
                onChange={(e) =>
                  setConfig({ ...config, max_context_players: Number(e.target.value) })
                }
              />
            </label>
          </div>
          <div className="settingsActions">
            <button
              type="button"
              className="ghostBtn"
              disabled={testing}
              onClick={() => void handleTestConnection()}
            >
              {testing ? 'Testing…' : 'Test connection'}
            </button>
            {apiStatus && (
              <span className={`statusPill ${apiStatus.connected ? 'ok' : 'bad'}`}>
                {apiStatus.connected ? 'Connected' : 'Disconnected'}
                {apiStatus.credits_remaining ? ` · ${apiStatus.credits_remaining}` : ''}
              </span>
            )}
          </div>
        </div>

        <div className="card">
          <h3>Analysis preferences</h3>
          <div className="formGrid">
            <label>
              Risk tolerance
              <select
                value={config.risk_tolerance}
                onChange={(e) => setConfig({ ...config, risk_tolerance: e.target.value })}
              >
                <option value="conservative">Conservative</option>
                <option value="moderate">Moderate</option>
                <option value="aggressive">Aggressive</option>
              </select>
            </label>
            <label>
              Stat weighting
              <select
                value={config.stat_weighting}
                onChange={(e) => setConfig({ ...config, stat_weighting: e.target.value })}
              >
                <option value="season_avg">Season average</option>
                <option value="last3">Last 3 games</option>
                <option value="matchup_adjusted">Matchup adjusted</option>
                <option value="balanced">Balanced</option>
              </select>
            </label>
            <label>
              Output format
              <select
                value={config.output_format}
                onChange={(e) => setConfig({ ...config, output_format: e.target.value })}
              >
                <option value="json_first">JSON first</option>
                <option value="text_only">Text only</option>
                <option value="json_plus_text">JSON + text</option>
              </select>
            </label>
            <label>
              Preferred leagues
              <input
                value={leaguesInput}
                onChange={(e) => setLeaguesInput(e.target.value)}
                placeholder="NFL, NBA, MLB"
              />
            </label>
            <label>
              Theme
              <select
                value={config.theme}
                onChange={(e) => setConfig({ ...config, theme: e.target.value })}
              >
                <option value="dark">Dark</option>
                <option value="light">Light</option>
              </select>
            </label>
          </div>
        </div>

        <div className="card">
          <h3>Bankroll controls</h3>
          <p className="muted">
            Paper-sim sizing limits. Max bet caps Kelly recommendations and is saved locally.
          </p>
          <div className="formGrid">
            <label>
              Total bankroll $
              <input
                type="number"
                min={0}
                step={1}
                value={bankrollConfig.total_bankroll}
                onChange={(e) =>
                  setBankrollConfig({ ...bankrollConfig, total_bankroll: Number(e.target.value) })
                }
              />
            </label>
            <label>
              Initial bankroll $
              <input
                type="number"
                min={0}
                step={1}
                value={bankrollConfig.initial_bankroll}
                onChange={(e) =>
                  setBankrollConfig({ ...bankrollConfig, initial_bankroll: Number(e.target.value) })
                }
              />
            </label>
            <label>
              Kelly fraction
              <input
                type="number"
                min={0}
                max={1}
                step={0.05}
                value={bankrollConfig.kelly_fraction}
                onChange={(e) =>
                  setBankrollConfig({ ...bankrollConfig, kelly_fraction: Number(e.target.value) })
                }
              />
            </label>
            <label>
              Max bet (%)
              <input
                type="number"
                min={0}
                max={100}
                step={0.5}
                value={(bankrollConfig.max_bet_pct * 100).toFixed(2)}
                onChange={(e) =>
                  setBankrollConfig({ ...bankrollConfig, max_bet_pct: Number(e.target.value) / 100 })
                }
              />
            </label>
            <label>
              Min bet $
              <input
                type="number"
                min={0}
                step={1}
                value={bankrollConfig.min_bet}
                onChange={(e) =>
                  setBankrollConfig({ ...bankrollConfig, min_bet: Number(e.target.value) })
                }
              />
            </label>
            <label>
              Default odds
              <input
                type="number"
                step={1}
                value={bankrollConfig.default_odds}
                onChange={(e) =>
                  setBankrollConfig({ ...bankrollConfig, default_odds: Number(e.target.value) })
                }
              />
            </label>
            <label>
              Strategy
              <select
                value={bankrollConfig.strategy}
                onChange={(e) =>
                  setBankrollConfig({ ...bankrollConfig, strategy: e.target.value })
                }
              >
                <option value="Kelly">Kelly Criterion</option>
                <option value="FlatBet">Flat Bet</option>
                <option value="PercentageOfBankroll">% of Bankroll</option>
                <option value="ConfidenceAdjustedKelly">Confidence-adjusted Kelly</option>
              </select>
            </label>
            <label>
              Daily bet limit $
              <input
                type="number"
                min={0}
                step={1}
                value={bankrollConfig.daily_bet_limit}
                onChange={(e) =>
                  setBankrollConfig({ ...bankrollConfig, daily_bet_limit: Number(e.target.value) })
                }
              />
            </label>
            <label>
              Weekly bet limit $
              <input
                type="number"
                min={0}
                step={1}
                value={bankrollConfig.weekly_bet_limit}
                onChange={(e) =>
                  setBankrollConfig({ ...bankrollConfig, weekly_bet_limit: Number(e.target.value) })
                }
              />
            </label>
          </div>
          <div className="settingsActions">
            <button
              type="button"
              className="primaryBtn"
              disabled={savingBankroll}
              onClick={() => void handleSaveBankroll()}
            >
              {savingBankroll ? 'Saving…' : 'Save bankroll controls'}
            </button>
            {bankrollMessage && <span className="statusPill ok">{bankrollMessage}</span>}
          </div>
        </div>

        <div className="card">
          <h3>PrizePicks & data keys</h3>
          <div className="formGrid">
            <label>
              PrizePicks email
              <input
                value={config.prizepicks_email}
                onChange={(e) => setConfig({ ...config, prizepicks_email: e.target.value })}
              />
            </label>
            <label>
              PrizePicks password
              <input
                type="password"
                placeholder={config.prizepicks_password ? maskSecret(config.prizepicks_password) : 'Optional'}
                value={prizepicksPasswordInput}
                onChange={(e) => setPrizePicksPasswordInput(e.target.value)}
                autoComplete="off"
              />
            </label>
            <label>
              Poll interval (seconds)
              <input
                type="number"
                min={15}
                max={600}
                value={config.prizepicks_poll_interval_secs}
                onChange={(e) =>
                  setConfig({ ...config, prizepicks_poll_interval_secs: Number(e.target.value) })
                }
              />
            </label>
            <label>
              OpenWeatherMap key
              <input
                type="password"
                placeholder={config.openweathermap_api_key ? 'Set' : 'Optional'}
                value={config.openweathermap_api_key}
                onChange={(e) => setConfig({ ...config, openweathermap_api_key: e.target.value })}
              />
            </label>
            <label>
              API-Sports key
              <input
                type="password"
                placeholder={config.api_sports_key ? 'Set' : 'Optional'}
                value={config.api_sports_key}
                onChange={(e) => setConfig({ ...config, api_sports_key: e.target.value })}
              />
            </label>
            <label>
              OpticOdds API key
              <input
                type="password"
                placeholder={config.opticodds_api_key ? 'Set' : 'Optional'}
                value={config.opticodds_api_key}
                onChange={(e) => setConfig({ ...config, opticodds_api_key: e.target.value })}
              />
            </label>
            <label>
              The Odds API key
              <input
                type="password"
                placeholder={config.odds_api_key ? 'Set' : 'Optional'}
                value={config.odds_api_key}
                onChange={(e) => setConfig({ ...config, odds_api_key: e.target.value })}
              />
            </label>
          </div>
        </div>

        <div className="card">
          <h3>Notifications & bot</h3>
          <div className="formGrid">
            <label>
              Discord webhook
              <input
                type="password"
                placeholder={config.discord_webhook_url ? maskSecret(config.discord_webhook_url) : 'https://discord.com/api/webhooks/…'}
                value={discordInput}
                onChange={(e) => setDiscordInput(e.target.value)}
              />
            </label>
            <label>
              Telegram bot token
              <input
                type="password"
                placeholder={config.telegram_bot_token ? 'Set' : 'Optional'}
                value={telegramTokenInput}
                onChange={(e) => setTelegramTokenInput(e.target.value)}
              />
            </label>
            <label>
              Telegram chat ID
              <input
                value={config.telegram_chat_id}
                onChange={(e) => setConfig({ ...config, telegram_chat_id: e.target.value })}
              />
            </label>
            <label>
              Daily picks time
              <input
                value={config.bot_daily_picks_time}
                onChange={(e) => setConfig({ ...config, bot_daily_picks_time: e.target.value })}
                placeholder="08:00"
              />
            </label>
          </div>
          <div className="toggleRow">
            <label className="toggleLabel">
              <input
                type="checkbox"
                checked={config.bot_daily_picks_enabled}
                onChange={(e) => setConfig({ ...config, bot_daily_picks_enabled: e.target.checked })}
              />
              Daily picks
            </label>
            <label className="toggleLabel">
              <input
                type="checkbox"
                checked={config.bot_game_alerts_enabled}
                onChange={(e) => setConfig({ ...config, bot_game_alerts_enabled: e.target.checked })}
              />
              Game alerts
            </label>
            <label className="toggleLabel">
              <input
                type="checkbox"
                checked={config.bot_grading_results_enabled}
                onChange={(e) => setConfig({ ...config, bot_grading_results_enabled: e.target.checked })}
              />
              Grading results
            </label>
          </div>
        </div>

        <div className="card">
          <h3>In-app notifications</h3>
          <p className="muted">
            Configure which in-app events trigger notifications in the Notification Center. See the{' '}
            <code>🔔 Notifications</code> tab to view history.
          </p>
          <div className="toggleRow">
            <label className="toggleLabel">
              <input
                type="checkbox"
                checked={notifSettings.enabled}
                onChange={(e) => setNotifSettings({ ...notifSettings, enabled: e.target.checked })}
              />
              Enable notifications
            </label>
            <label className="toggleLabel">
              <input
                type="checkbox"
                checked={notifSettings.show_os_notifications}
                onChange={(e) =>
                  setNotifSettings({ ...notifSettings, show_os_notifications: e.target.checked })
                }
              />
              OS notifications
            </label>
          </div>
          <div className="toggleRow">
            <label className="toggleLabel">
              <input
                type="checkbox"
                checked={notifSettings.game_starting_enabled}
                disabled={!notifSettings.enabled}
                onChange={(e) =>
                  setNotifSettings({ ...notifSettings, game_starting_enabled: e.target.checked })
                }
              />
              Game starting
            </label>
            <label className="toggleLabel">
              <input
                type="checkbox"
                checked={notifSettings.game_final_enabled}
                disabled={!notifSettings.enabled}
                onChange={(e) =>
                  setNotifSettings({ ...notifSettings, game_final_enabled: e.target.checked })
                }
              />
              Game final
            </label>
            <label className="toggleLabel">
              <input
                type="checkbox"
                checked={notifSettings.prediction_graded_enabled}
                disabled={!notifSettings.enabled}
                onChange={(e) =>
                  setNotifSettings({ ...notifSettings, prediction_graded_enabled: e.target.checked })
                }
              />
              Prediction graded
            </label>
            <label className="toggleLabel">
              <input
                type="checkbox"
                checked={notifSettings.grading_complete_enabled}
                disabled={!notifSettings.enabled}
                onChange={(e) =>
                  setNotifSettings({ ...notifSettings, grading_complete_enabled: e.target.checked })
                }
              />
              Grading complete
            </label>
          </div>
          <div className="formGrid">
            <label>
              Poll interval (seconds)
              <input
                type="number"
                min={15}
                max={600}
                value={notifSettings.poll_interval_secs}
                disabled={!notifSettings.enabled}
                onChange={(e) =>
                  setNotifSettings({ ...notifSettings, poll_interval_secs: Number(e.target.value) })
                }
              />
            </label>
            <label>
              Game starting alert (minutes before)
              <input
                type="number"
                min={0}
                max={120}
                value={notifSettings.game_starting_minutes_before}
                disabled={!notifSettings.enabled || !notifSettings.game_starting_enabled}
                onChange={(e) =>
                  setNotifSettings({
                    ...notifSettings,
                    game_starting_minutes_before: Number(e.target.value),
                  })
                }
              />
            </label>
          </div>
          <div className="settingsActions">
            <button
              type="button"
              className="primaryBtn"
              disabled={savingNotif}
              onClick={() => void handleSaveNotif()}
            >
              {savingNotif ? 'Saving…' : 'Save notification settings'}
            </button>
            {notifMessage && <span className="statusPill ok">{notifMessage}</span>}
          </div>
        </div>

        <div className="card settingsWide">
          <h3>System prompt</h3>
          <p className="muted">Override the default PrizePicks Monster analyst persona. Leave blank to reload the built-in prompt on next app start.</p>
          <textarea
            className="promptArea"
            rows={12}
            value={config.system_prompt}
            onChange={(e) => setConfig({ ...config, system_prompt: e.target.value })}
          />
        </div>
      </div>
    </section>
  );
}
