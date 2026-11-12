import { useCallback, useEffect, useState } from "react";
import type { RoutingTrace } from "@/lib/api/routingTrace";

export function routeInProgress(row: RoutingTrace) {
  return row.state === "routing" || row.state === "streaming";
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
            (trace.startedAt + trace.durationMs < Date.now() - 5000 ||
              (trace.id !== row?.id &&
                (!previous.observed.length ||
                  previous.observed.includes(trace.id))))
          )
            return false;
          if (!agents.has(trace.agent) && agents.size >= 4) return false;
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
