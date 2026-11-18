import { useSyncExternalStore } from "react";
import { useQueryClient, type QueryClient } from "@tanstack/react-query";
import type { AppId } from "@/lib/api";

type Listener = () => void;
type Messages = Partial<Record<AppId, string>>;

function createFeedbackStore() {
  let messages: Messages = {};
  const timers = new Map<AppId, ReturnType<typeof setTimeout>>();
  const listeners = new Set<Listener>();
  function clear(app: AppId) {
    clearTimeout(timers.get(app));
    timers.delete(app);
    if (!(app in messages)) return;
    const next = { ...messages };
    delete next[app];
    messages = next;
    listeners.forEach((listener) => listener());
  }
  return {
    snapshot: () => messages,
    subscribe(listener: Listener) {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    clear,
    show(app: AppId, message: string) {
      clearTimeout(timers.get(app));
      messages = { ...messages, [app]: message };
      listeners.forEach((listener) => listener());
      timers.set(
        app,
        setTimeout(() => clear(app), 8000),
      );
    },
  };
}

const stores = new WeakMap<
  QueryClient,
  ReturnType<typeof createFeedbackStore>
>();
export function useAgentFeedback() {
  const client = useQueryClient();
  let store = stores.get(client);
  if (!store) {
    store = createFeedbackStore();
    stores.set(client, store);
  }
  return {
    messages: useSyncExternalStore(store.subscribe, store.snapshot),
    show: store.show,
    clear: store.clear,
  };
}
