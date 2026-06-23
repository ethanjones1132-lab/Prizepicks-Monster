import { useState, useCallback, useRef, useEffect } from 'react';
import { chatApi } from '../services/tauri';
import type { ChatMessage } from '../types';

export function useChat() {
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [isStreaming, setIsStreaming] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [sessionId, setSessionId] = useState<string | null>(null);
  const abortRef = useRef<AbortController | null>(null);

  const initSession = useCallback(async () => {
    abortRef.current?.abort();
    const session = await chatApi.newSession();
    setSessionId(session.id);
    setMessages([]);
    setError(null);
    return session.id;
  }, []);

  const clearError = useCallback(() => setError(null), []);

  const sendMessage = useCallback(
    async (content: string, stream = false) => {
      let sid = sessionId;
      if (!sid) {
        sid = await initSession();
      }

      abortRef.current?.abort();
      const abort = new AbortController();
      abortRef.current = abort;

      const userMsg: ChatMessage = {
        id: crypto.randomUUID(),
        role: 'user',
        content,
        timestamp: new Date().toISOString(),
      };
      setMessages((prev) => [...prev, userMsg]);
      setIsStreaming(true);
      setError(null);

      try {
        if (stream) {
          await chatApi.sendMessageStream(content, sid);
          const history = await chatApi.getHistory(sid);
          if (!abort.signal.aborted) {
            setMessages(history);
          }
        } else {
          const response = await chatApi.sendMessage(content, sid);
          if (!abort.signal.aborted) {
            const assistantMsg: ChatMessage = {
              id: crypto.randomUUID(),
              role: 'assistant',
              content: response.content,
              reasoning: response.reasoning,
              timestamp: new Date().toISOString(),
              tokens_used: response.tokens_used,
            };
            setMessages((prev) => [...prev, assistantMsg]);
          }
        }
      } catch (e) {
        if (!abort.signal.aborted) {
          const errMsg = e instanceof Error ? e.message : String(e);
          setError(errMsg);
        }
      } finally {
        if (!abort.signal.aborted) {
          setIsStreaming(false);
        }
        if (abortRef.current === abort) {
          abortRef.current = null;
        }
      }
    },
    [sessionId, initSession],
  );

  useEffect(() => {
    return () => {
      abortRef.current?.abort();
    };
  }, []);

  return {
    messages,
    isStreaming,
    error,
    sendMessage,
    initSession,
    sessionId,
    clearError,
  };
}
