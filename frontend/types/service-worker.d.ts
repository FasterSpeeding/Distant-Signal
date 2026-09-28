// Ambient types for the classic (non-module) service-worker scripts in
// public/ (sw.js and the sw-cache-rules.js it importScripts()). Only
// tsconfig.sw.json includes this file; tsconfig.json excludes it because it
// augments WebWorker-lib interfaces that the DOM lib does not have.
//
// lib.webworker.d.ts types the global `self` as the generic
// `WorkerGlobalScope & typeof globalThis`, and TypeScript cannot redeclare a
// global `var` with a narrower type. Every script tsconfig.sw.json checks
// runs in a ServiceWorkerGlobalScope, so the members sw.js uses are merged
// into WorkerGlobalScope here. The alternative, a top-level typed alias of
// `self` in sw.js, would land in the global lexical scope that
// importScripts() shares (see the NOTE in sw.js).

interface WorkerGlobalScope {
  // Assigned onto `self` by sw-cache-rules.js for sw.js to call. TypeScript
  // reads that file as a CommonJS module (it has `module.exports`), so its
  // functions are not globals here; the types come from its exports.
  isCacheable: typeof import('../public/sw-cache-rules.js').isCacheable;
  parsePushPayload: typeof import('../public/sw-cache-rules.js').parsePushPayload;
  sameOriginNotificationUrl: typeof import('../public/sw-cache-rules.js').sameOriginNotificationUrl;

  readonly clients: Clients;
  readonly registration: ServiceWorkerRegistration;
  skipWaiting(): Promise<void>;
  addEventListener<K extends keyof ServiceWorkerGlobalScopeEventMap>(
    type: K,
    listener: (this: ServiceWorkerGlobalScope, ev: ServiceWorkerGlobalScopeEventMap[K]) => unknown,
    options?: boolean | AddEventListenerOptions,
  ): void;
}

// A push message's JSON payload: crates/notifier/src/send.rs's
// `NotificationPayload`, all four fields strings.
interface PushNotificationPayload {
  title: string;
  body: string;
  url: string;
  tag: string;
}
