// What a page shares beyond its own tree: the window it shares with the shell and the persistent
// panel's module (their snapshots, set as window properties, and the events that say one moved),
// and the few values every part of the page reads (whether it may show what is out of date, the
// server's notice).

import { useEffect, useReducer, useSyncExternalStore } from "react";

type Shared = Record<string, unknown>;

/** The window's value `key` of the page protocol (`reviewQuickDraft`, `reviewPanelReady`…). */
export function snapshot(key: string): unknown {
  return (window as unknown as Shared)[key];
}

/** Set the window's value `key` of the page protocol. */
export function remember(key: string, value: unknown): void {
  (window as unknown as Shared)[key] = value;
}

/** Say `event` (`review:frame`…) on the window, for every module that listens. */
export function publish(event: string): void {
  dispatchEvent(new Event(event));
}

/** The array `value` is, else an empty one. */
export function list(value: unknown): unknown[] {
  return Array.isArray(value) ? value : [];
}

/** `value`'s field `key`, whatever `value` is. */
export function field(value: unknown, key: string): unknown {
  return typeof value === "object" && value !== null ? (value as Shared)[key] : undefined;
}

/** Draw again each time the window says one of `events`. */
export function useWindowEvents(events: readonly string[]): void {
  const [, heard] = useReducer((count: number) => count + 1, 0);
  useEffect(() => {
    for (const event of events) addEventListener(event, heard);
    return () => {
      for (const event of events) removeEventListener(event, heard);
    };
  }, [events]);
}

/** A value the page's parts share, which each part that reads it draws again on. */
export interface Store<T> {
  get(): T;
  set(value: T): void;
  subscribe(listener: () => void): () => void;
}

export function store<T>(initial: T): Store<T> {
  let value = initial;
  const listeners = new Set<() => void>();
  return {
    get: () => value,
    set(next) {
      if (Object.is(value, next)) return;
      value = next;
      for (const listener of listeners) listener();
    },
    subscribe(listener) {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
  };
}

/** `shared`'s value, the part drawn again each time it changes (on a server's render too). */
export function useStore<T>(shared: Store<T>): T {
  return useSyncExternalStore(shared.subscribe, shared.get, shared.get);
}

/**
 * Whether the page may show what is out of date: drawn from this device's cache and not yet
 * checked, or reconnecting; the page's notices set it. The owner's actions wait meanwhile,
 * disabled and saying CHECKING: one given on a card that changed since would act on what the
 * owner never saw.
 */
export const STALE = store(false);

/** The server's own notice, as its feed last said it; empty for none. */
export const SERVER_NOTICE = store("");
