import { useCallback, useEffect, useState } from "react";
import type { RoutingTrace } from "@/lib/api/routingTrace";

const MAX_SOURCES = 4;
const SOURCE_LINGER_MS = 12000;
// A finished trace keeps replaying its flight when first observed within this
// window, so opening the routing page right after sending a request from an
// agent still shows the routing animation.
export const FINISHED_REPLAY_WINDOW_MS = 60000;

export function routeInProgress(row: RoutingTrace) {
  return row.state === "routing" || row.state === "streaming";
}

export function routingSourceKey(row: RoutingTrace) {
  return JSON.stringify([row.agent, row.model]);
}

export function liveRoutingSources(rows: RoutingTrace[], now: number) {
  const finishedAt = (row: RoutingTrace) => row.startedAt + row.durationMs;
  const latestFinished = new Map<string, RoutingTrace>();
  for (const row of rows) {
    if (routeInProgress(row)) continue;
    const latest = latestFinished.get(row.agent);
    if (!latest || row.startedAt > latest.startedAt) {
      latestFinished.set(row.agent, row);
    }
  }
  const ordered = [
    ...rows.filter(routeInProgress),
    ...[...latestFinished.values()]
      .filter((row) => finishedAt(row) > now - SOURCE_LINGER_MS)
      .sort((first, second) => finishedAt(second) - finishedAt(first)),
  ];
  const sources = new Map<string, RoutingTrace>();
  const agents = new Set<string>();
  for (const request of ordered) {
    if (!routeInProgress(request) && agents.has(request.agent)) continue;
    const key = routingSourceKey(request);
    if (sources.has(key)) continue;
    sources.set(key, request);
    agents.add(request.agent);
    if (sources.size === MAX_SOURCES) break;
  }
  return sources.size ? [...sources.values()] : rows.slice(0, 1);
}

export function useRoutingFlights(
  row: RoutingTrace | undefined,
  concurrent: RoutingTrace[],
  enabled: boolean,
) {
  const [state, setState] = useState<{
    flights: RoutingTrace[];
    finished: string[];
    observed: string[];
  }>({ flights: [], finished: [], observed: [] });
  useEffect(() => {
    setState((previous) => {
      if (!enabled)
        return previous.observed.length
          ? { flights: [], finished: [], observed: [] }
          : previous;
      const inputs = new Map(concurrent.map((trace) => [trace.id, trace]));
      if (row) inputs.set(row.id, row);
      const retained = new Set(previous.flights.map((trace) => trace.id));
      const finished = new Set(previous.finished);
      const observed = [...inputs.keys()];
      const agents = new Set<string>();
      const flights = [...inputs.values()]
        .sort((a, b) => b.startedAt - a.startedAt)
        .filter((trace) => {
          if (finished.has(trace.id)) return false;
          if (
            !retained.has(trace.id) &&
            !routeInProgress(trace) &&
            (trace.startedAt + trace.durationMs <
              Date.now() - FINISHED_REPLAY_WINDOW_MS ||
              (trace.id !== row?.id &&
                (!previous.observed.length ||
                  previous.observed.includes(trace.id))))
          )
            return false;
          if (!agents.has(trace.agent) && agents.size >= MAX_SOURCES)
            return false;
          agents.add(trace.agent);
          return true;
        })
        .slice(0, 60)
        .sort((a, b) => a.startedAt - b.startedAt || a.id.localeCompare(b.id));
      if (
        flights.length === previous.flights.length &&
        flights.every((trace, index) => trace === previous.flights[index]) &&
        observed.length === previous.observed.length &&
        observed.every((id, index) => id === previous.observed[index])
      )
        return previous;
      return { flights, finished: previous.finished, observed };
    });
  }, [row, concurrent, enabled]);
  const finish = useCallback((id: string) => {
    setState((previous) => ({
      ...previous,
      flights: previous.flights.filter((trace) => trace.id !== id),
      finished: [
        ...previous.finished.filter((entry) => entry !== id),
        id,
      ].slice(-256),
    }));
  }, []);
  return { flights: enabled ? state.flights : [], finish };
}
