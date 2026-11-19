import type { RoutingTrace, RoutingTraceStep } from "@/lib/api/routingTrace";

export function attemptCandidateIndex(
  trace: RoutingTrace,
  step: RoutingTraceStep,
) {
  const index = step.candidate;
  if (
    step.kind === "attempt" &&
    index != null &&
    Number.isInteger(index) &&
    trace.candidates[index]
  )
    return index;
  return undefined;
}
