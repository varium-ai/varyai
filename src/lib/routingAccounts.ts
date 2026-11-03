// Account folding follows the reference implementation's routing plugin (MIT).
// See third_party/routing-reference/LICENSE. Only gateway candidates supply observations.
import type { RoutingAccount, RoutingTrace } from "@/lib/api/routingTrace";

export interface FoldedRoutingAccount {
  id: string;
  kind: "account" | "key" | "provider";
  providerId: string;
  providerName: string;
  tried: number;
  answered: number;
  failed: number;
  failures: Record<string, number>;
  lastAnswered?: number;
  models: string[];
  observations: RoutingAccount[];
}

export function foldRoutingAccounts(
  requests: RoutingTrace[],
  current: RoutingAccount[],
  live: boolean,
): FoldedRoutingAccount[] {
  const accounts = new Map<string, FoldedRoutingAccount>();
  const reading = new Map(
    current
      .filter((row) => row.id)
      .map((row) => [JSON.stringify([row.id, row.model]), row]),
  );
  for (const request of [...requests].sort(
    (a, b) => a.startedAt - b.startedAt,
  )) {
    const candidates = request.candidates.map((candidate) => {
      // Older traces can identify a provider/model, never a credential.
      const recorded = candidate.account ?? {
        providerId: candidate.providerId,
        providerName: candidate.providerName,
        model: candidate.model,
        restUntil: 0,
        windows: [],
      };
      const id =
        recorded.id ??
        JSON.stringify(["provider", recorded.providerId, recorded.model]);
      let account = accounts.get(id);
      if (!account) {
        account = {
          id,
          kind: recorded.kind ?? "provider",
          providerId: recorded.providerId,
          providerName: recorded.providerName,
          tried: 0,
          answered: 0,
          failed: 0,
          failures: {},
          models: [],
          observations: [],
        };
        accounts.set(id, account);
      }
      account.providerName = recorded.providerName;
      const observation =
        live && recorded.id
          ? (reading.get(JSON.stringify([recorded.id, recorded.model])) ??
            recorded)
          : recorded;
      const index = account.observations.findIndex(
        (row) => row.model === observation.model,
      );
      if (index < 0) account.observations.push(observation);
      else account.observations[index] = observation;
      return account;
    });
    let pending:
      | { candidate: number; account: FoldedRoutingAccount }
      | undefined;
    for (const step of request.steps) {
      if (
        step.kind === "attempt" &&
        step.candidate != null &&
        candidates[step.candidate]
      ) {
        pending = {
          candidate: step.candidate,
          account: candidates[step.candidate],
        };
        continue;
      }
      if (
        !pending ||
        (step.candidate != null && step.candidate !== pending.candidate)
      )
        continue;
      if (step.kind === "cancelled") {
        pending = undefined;
        continue;
      }
      if (
        ![
          "rejected",
          "failed",
          "completed",
          "first_token_timeout",
          "context",
        ].includes(step.kind)
      )
        continue;
      const account = pending.account;
      account.tried++;
      if (step.kind === "completed" && (step.status ?? 200) < 400) {
        account.answered++;
        account.lastAnswered = Math.max(
          account.lastAnswered ?? 0,
          request.startedAt + step.atMs,
        );
        const model = request.candidates[pending.candidate].model;
        if (!account.models.includes(model)) account.models.push(model);
      } else {
        account.failed++;
        const reason =
          step.kind === "first_token_timeout"
            ? "slow"
            : step.kind === "context"
              ? "context"
              : String(step.status ?? "failed");
        account.failures[reason] = (account.failures[reason] ?? 0) + 1;
      }
      pending = undefined;
    }
  }
  return [...accounts.values()].sort(
    (a, b) =>
      a.providerName.localeCompare(b.providerName) ||
      a.providerId.localeCompare(b.providerId) ||
      a.id.localeCompare(b.id),
  );
}
