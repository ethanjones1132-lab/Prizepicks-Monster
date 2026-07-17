import { useEffect, useState } from 'react';
import { WHATS_NEW_ENTRIES, WHATS_NEW_STORAGE_KEY, type WhatsNewEntry } from '../data/whatsNewData';

interface Props {
  onClose: () => void;
}

/**
 * "What's New" changelog modal — shows recent feature additions
 * from the maintenance passes. Marks itself as seen on open.
 */
export function WhatsNewModal({ onClose }: Props) {
  const [entries] = useState<WhatsNewEntry[]>(() => WHATS_NEW_ENTRIES);

  // Mark as seen when the modal opens
  useEffect(() => {
    try {
      localStorage.setItem(WHATS_NEW_STORAGE_KEY, entries[0]?.date ?? '');
    } catch {
      // localStorage unavailable — silently ignore
    }
  }, [entries]);

  // Close on Escape key
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  }, [onClose]);

  // Close on backdrop click
  const handleBackdropClick = (e: React.MouseEvent) => {
    if (e.target === e.currentTarget) onClose();
  };

  return (
    <div className="whatsnewBackdrop" onClick={handleBackdropClick}>
      <div className="whatsnewModal" role="dialog" aria-label="What's New in PrizePicks Monster">
        <div className="whatsnewHeader">
          <h2>🎉 What's New</h2>
          <span className="muted small">PrizePicks Monster changelog</span>
          <button
            className="whatsnewClose"
            onClick={onClose}
            aria-label="Close"
            title="Close"
          >
            ✕
          </button>
        </div>

        <div className="whatsnewList">
          {entries.map((entry) => (
            <div key={entry.date} className="whatsnewEntry">
              <div className="whatsnewEntryMeta">
                <span className="whatsnewDate">{entry.date}</span>
                <span className="whatsnewTitle">{entry.title}</span>
              </div>
              <ul className="whatsnewBullets">
                {entry.bullets.map((b, i) => (
                  <li key={i}>{b}</li>
                ))}
              </ul>
            </div>
          ))}
        </div>

        <div className="whatsnewFooter">
          <span className="muted small">
            See <code>PRIORITIES.md</code> in the repo for the full changelog.
          </span>
          <button className="whatsnewGotIt" onClick={onClose}>
            Got it
          </button>
        </div>
      </div>
    </div>
  );
}

/**
 * Check if there are unseen "What's New" entries by comparing
 * the stored last-seen date against the most recent entry's date.
 */
export function hasUnseenWhatsNew(lastSeen: string | null): boolean {
  if (!lastSeen) return WHATS_NEW_ENTRIES.length > 0;
  const latest = WHATS_NEW_ENTRIES[0]?.date;
  if (!latest) return false;
  return latest > lastSeen;
}
