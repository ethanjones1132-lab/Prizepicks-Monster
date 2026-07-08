import { useCallback, useEffect, useState } from 'react';
import { prizepicksApi } from '../services/prizepicks';
import type { AppNotification } from '../types/prizepicks';

function notificationIcon(nt: string): string {
  switch (nt) {
    case 'game_starting': return '🏈';
    case 'game_final': return '✅';
    case 'prediction_graded':
    case 'prediction_win': return '📈';
    case 'prediction_loss': return '📉';
    case 'prediction_push': return '↔️';
    case 'grading_complete': return '📊';
    default: return '🔔';
  }
}

function timeAgo(iso: string): string {
  const ms = Date.now() - new Date(iso).getTime();
  const mins = Math.floor(ms / 60000);
  if (mins < 1) return 'just now';
  if (mins < 60) return `${mins}m ago`;
  const hrs = Math.floor(mins / 60);
  if (hrs < 24) return `${hrs}h ago`;
  const days = Math.floor(hrs / 24);
  return `${days}d ago`;
}

export function NotificationCenter() {
  const [notifications, setNotifications] = useState<AppNotification[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const fetchNotifications = useCallback(async () => {
    try {
      setLoading(true);
      setError(null);
      const data = await prizepicksApi.getNotifications(100);
      setNotifications(data);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    fetchNotifications();
  }, [fetchNotifications]);

  const handleMarkRead = useCallback(async (id: string) => {
    try {
      await prizepicksApi.markNotificationRead(id);
      setNotifications((prev) =>
        prev.map((n) => (n.id === id ? { ...n, read: true } : n)),
      );
    } catch { /* ignore */ }
  }, []);

  const handleMarkAllRead = useCallback(async () => {
    try {
      await prizepicksApi.markAllNotificationsRead();
      setNotifications((prev) => prev.map((n) => ({ ...n, read: true })));
    } catch { /* ignore */ }
  }, []);

  const handleDismiss = useCallback(async (id: string) => {
    try {
      await prizepicksApi.dismissNotification(id);
      setNotifications((prev) => prev.filter((n) => n.id !== id));
    } catch { /* ignore */ }
  }, []);

  const active = notifications.filter((n) => !n.dismissed);
  const unreadCount = active.filter((n) => !n.read).length;

  return (
    <section className="page notificationCenterPage">
      <header className="notificationCenterHeader">
        <div>
          <h2>🔔 Notifications</h2>
          <p className="muted">
            Game-day alerts, prediction grading results, and system notifications.
          </p>
        </div>
        <div className="notificationCenterActions">
          <span className="notificationCount muted small">
            {active.length} notification{active.length !== 1 ? 's' : ''}
            {unreadCount > 0 && (
              <span className="unreadBadge"> {unreadCount} unread</span>
            )}
          </span>
          {unreadCount > 0 && (
            <button className="markAllReadBtn" onClick={handleMarkAllRead}>
              Mark all read
            </button>
          )}
          <button className="refreshBtn" onClick={fetchNotifications} disabled={loading}>
            {loading ? '↻' : '↻ Refresh'}
          </button>
        </div>
      </header>

      {error && (
        <div className="notificationError">
          <span>Failed to load notifications: {error}</span>
          <button onClick={fetchNotifications}>Retry</button>
        </div>
      )}

      {loading && notifications.length === 0 && (
        <div className="notificationEmpty">
          <span className="muted">Loading notifications…</span>
        </div>
      )}

      {!loading && active.length === 0 && !error && (
        <div className="notificationEmpty">
          <span className="muted large">📭 No notifications yet</span>
          <p className="muted small">
            Notifications appear here when games start, predictions are graded, or
            system events occur. Place predictions through the Analyst chat to get
            started.
          </p>
        </div>
      )}

      {active.length > 0 && (
        <div className="notificationList">
          {active.map((n) => (
            <div
              key={n.id}
              className={`notificationRow ${n.read ? 'read' : 'unread'}`}
              onClick={() => !n.read && handleMarkRead(n.id)}
            >
              <span className="notificationIcon">
                {notificationIcon(n.notification_type)}
              </span>
              <div className="notificationContent">
                <span className="notificationTitle">{n.title}</span>
                <span className="notificationBody">{n.body}</span>
                {n.player_name && (
                  <span className="notificationPlayer">{n.player_name}</span>
                )}
              </div>
              <div className="notificationMeta">
                <span className="notificationTime">{timeAgo(n.created_at)}</span>
                <button
                  className="notificationDismiss"
                  onClick={(e) => {
                    e.stopPropagation();
                    handleDismiss(n.id);
                  }}
                  title="Dismiss"
                >
                  ✕
                </button>
              </div>
            </div>
          ))}
        </div>
      )}
    </section>
  );
}
