import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import ReactMarkdown from 'react-markdown';
import rehypeRaw from 'rehype-raw';
import rehypeSanitize from 'rehype-sanitize';

import {
  aiChat,
  aiGetDefaultModel,
  aiGetModels,
  AvailableModel,
  ChatMessage,
  formatError,
  getAuthStatus,
} from '../lib/tauri';

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface AiAssistantProps {
  instanceId?: string | null;
  crashLog?: string | null;
  crashSignatures?: string | null;
  suspects?: string | null;
  onClose: () => void;
}

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

export function AiAssistant({
  instanceId,
  crashLog,
  crashSignatures,
  suspects,
  onClose,
}: AiAssistantProps) {
  const [authenticated, setAuthenticated] = useState<boolean | null>(null);
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [input, setInput] = useState('');
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [models, setModels] = useState<AvailableModel[]>([]);
  const [selectedModel, setSelectedModel] = useState<string | null>(null);

  const scrollRef = useRef<HTMLDivElement>(null);

  // --- Auth check on mount ---
  useEffect(() => {
    getAuthStatus().then(setAuthenticated).catch(() => setAuthenticated(false));
  }, []);

  // --- Load models on mount ---
  useEffect(() => {
    let cancelled = false;
    Promise.all([aiGetModels(), aiGetDefaultModel()]).then(
      ([rawModels, defaultModel]) => {
        if (cancelled) return;
        setModels(rawModels);
        setSelectedModel(defaultModel);
      },
    );
    return () => {
      cancelled = true;
    };
  }, []);

  // --- Auto-scroll to bottom on new messages ---
  useEffect(() => {
    if (scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
  }, [messages, loading]);

  // --- Build context for first message only ---
  const context = useMemo(
    () =>
      messages.length === 0
        ? {
            instance_id: instanceId ?? null,
            crash_log: crashLog ?? null,
            crash_signatures: crashSignatures ?? null,
            suspects: suspects ?? null,
          }
        : null,
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [instanceId, crashLog, crashSignatures, suspects, messages.length],
  );

  // --- Send handler ---
  const handleSend = useCallback(async () => {
    const trimmed = input.trim();
    if (!trimmed || loading) return;

    const userMsg: ChatMessage = { role: 'user', content: trimmed };
    const updated = [...messages, userMsg];
    setMessages(updated);
    setInput('');
    setLoading(true);
    setError(null);

    try {
      const response = await aiChat(updated, context, selectedModel);
      setMessages((prev) => [
        ...prev,
        { role: 'assistant', content: response.content },
      ]);
    } catch (e) {
      setError(formatError(e));
    } finally {
      setLoading(false);
    }
  }, [input, loading, messages, context, selectedModel]);

  // --- Keyboard shortcut: Enter to send, Shift+Enter for newline ---
  const handleKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
  };

  // --- Retry handler ---
  const handleRetry = useCallback(async () => {
    if (messages.length === 0) return;

    // Re-send the last user message (drop any failed assistant response)
    const userMessages = messages.filter((m) => m.role === 'user');
    if (userMessages.length === 0) return;

    const messagesToSend = userMessages;

    setMessages(messagesToSend);
    setLoading(true);
    setError(null);

    try {
      const response = await aiChat(messagesToSend, context, selectedModel);
      setMessages((prev) => [...prev, { role: 'assistant', content: response.content }]);
    } catch (e) {
      setError(formatError(e));
    } finally {
      setLoading(false);
    }
  }, [messages, context, selectedModel]);

  // --- Auth gate ---
  if (authenticated === false) {
    return (
      <div className="flex h-full w-full flex-col rounded-xl border border-border bg-background">
        <div className="flex items-center justify-between border-b border-border px-4 py-3">
          <h2 className="text-sm font-semibold">AI Assistant</h2>
          <button
            onClick={onClose}
            className="text-muted-foreground hover:text-foreground"
            aria-label="Close"
          >
            &times;
          </button>
        </div>
        <div className="flex flex-1 flex-col items-center justify-center gap-3 px-6 text-center">
          <div className="text-3xl" aria-hidden="true">
            &#129302;
          </div>
          <p className="text-sm text-muted-foreground">
            Sign in with GitHub to use the AI assistant. Your GitHub account
            provides free access to AI models via GitHub Models.
          </p>
          <p className="text-xs text-muted-foreground">
            No separate API key needed.
          </p>
        </div>
      </div>
    );
  }

  // --- Loading auth ---
  if (authenticated === null) {
    return (
      <div className="flex h-full w-full flex-col items-center justify-center rounded-xl border border-border bg-background">
        <p className="text-sm text-muted-foreground">Loading…</p>
      </div>
    );
  }

  // --- Main chat UI ---
  return (
    <div className="flex h-full w-full flex-col overflow-hidden rounded-xl border border-border bg-background">
      {/* Header */}
      <div className="flex items-center justify-between border-b border-border px-4 py-3">
        <h2 className="text-sm font-semibold">AI Assistant</h2>
        <div className="flex items-center gap-2">
          {models.length > 0 && (
            <div className="flex items-center gap-1">
              <label
                htmlFor="ai-model-select"
                className="text-[11px] text-muted-foreground"
              >
                Model:
              </label>
              <select
                id="ai-model-select"
                value={selectedModel ?? ''}
                onChange={(e) => setSelectedModel(e.target.value)}
                className="rounded-md border border-border bg-background px-2 py-1 text-[11px] text-foreground outline-none focus:ring-1 focus:ring-brand-500"
              >
                {models.map((m) => (
                  <option key={m.id} value={m.id}>
                    {m.name}
                  </option>
                ))}
              </select>
            </div>
          )}
          <button
            onClick={onClose}
            className="text-muted-foreground hover:text-foreground"
            aria-label="Close"
          >
            &times;
          </button>
        </div>
      </div>

      {/* Privacy note */}
      {messages.length === 0 && (
        <div className="border-b border-border px-4 py-2">
          <p className="text-[11px] text-muted-foreground">
            Your crash data is sent to GitHub Models for analysis. This uses
            your GitHub account — no separate API key needed.
          </p>
        </div>
      )}

      {/* Messages list */}
      <div
        ref={scrollRef}
        className="flex flex-1 flex-col gap-3 overflow-y-auto p-4"
      >
        {messages.length === 0 && !loading && (
          <div className="flex flex-1 items-center justify-center">
            <p className="text-xs text-muted-foreground">
              Ask about crashes, mods, or anything Agora-related.
            </p>
          </div>
        )}

        {messages.map((msg, i) =>
          msg.role === 'user' ? (
            <div key={i} className="flex justify-end">
              <div className="max-w-[80%] rounded-xl bg-primary px-4 py-2 text-sm text-primary-foreground">
                {msg.content}
              </div>
            </div>
          ) : (
            <div key={i} className="flex justify-start">
              <div className="max-w-[80%] rounded-xl bg-card px-4 py-2 text-sm text-card-foreground">
                <div className="prose prose-sm prose-neutral dark:prose-invert max-w-none break-words">
                  <ReactMarkdown rehypePlugins={[rehypeRaw, rehypeSanitize]}>
                    {msg.content}
                  </ReactMarkdown>
                </div>
              </div>
            </div>
          ),
        )}

        {/* Loading indicator */}
        {loading && (
          <div className="flex justify-start">
            <div className="rounded-xl bg-card">
              <span className="text-sm text-muted-foreground">
                Thinking
                <span className="inline-flex gap-0.5">
                  <span className="dot1">.</span>
                  <span className="dot2">.</span>
                  <span className="dot3">.</span>
                </span>
              </span>
            </div>
          </div>
        )}

        {/* Error display */}
        {error && (
          <div className="rounded-lg border border-destructive/20 bg-destructive/10 px-4 py-3">
            <p className="text-sm text-destructive">{error}</p>
            <button
              onClick={handleRetry}
              className="mt-2 text-xs text-destructive underline hover:text-destructive/80"
            >
              Retry
            </button>
          </div>
        )}
      </div>

      {/* Input area */}
      <div className="border-t border-border p-3">
        <div className="flex gap-2">
          <textarea
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={handleKeyDown}
            placeholder="Ask about crashes, mods, or anything…"
            rows={2}
            className="flex-1 resize-none rounded-lg border border-input bg-background px-3 py-2 text-sm text-foreground outline-none placeholder-muted-foreground focus:border-primary focus:ring-1 focus:ring-brand-500"
          />
          <button
            onClick={handleSend}
            disabled={loading || !input.trim()}
            className="self-end rounded-lg bg-primary px-4 py-2 text-sm font-medium text-primary-foreground transition-colors hover:bg-primary/90 disabled:cursor-not-allowed disabled:opacity-50"
          >
            Send
          </button>
        </div>
      </div>
    </div>
  );
}
