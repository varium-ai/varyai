export type CredentialStatus =
  | "valid"
  | "expired"
  | "not_found"
  | "parse_error";

export interface QuotaTier {
  name: string;
  utilization: number; // 0-100
  resetsAt: string | null;
  usedValueUsd?: number | null;
  maxValueUsd?: number | null;
  planLabel?: string | null;
}

export interface ExtraUsage {
  isEnabled: boolean;
  monthlyLimit: number | null;
  usedCredits: number | null;
  utilization: number | null;
  currency: string | null;
}

export interface CodexQuotaDetails {
  allowed?: boolean | null;
  limitReached?: boolean | null;
  modelUsage: Record<
    string,
    {
      available?: boolean | null;
      available_at?: unknown;
      credits_would_enable?: boolean | null;
    }
  >;
  credits?: {
    has_credits?: boolean | null;
    unlimited?: boolean | null;
    overage_limit_reached?: boolean | null;
    balance?: string | null;
  } | null;
  resetCredits?: {
    available_count?: number | null;
    applicable_available_count?: number | null;
  } | null;
  spendControl?: {
    reached?: boolean | null;
    individual_limit?: number | null;
  } | null;
}
export interface SubscriptionQuota {
  codex?: CodexQuotaDetails | null;
  tool: string;
  credentialStatus: CredentialStatus;
  credentialMessage: string | null;
  success: boolean;
  tiers: QuotaTier[];
  extraUsage: ExtraUsage | null;
  error: string | null;
  queriedAt: number | null;
}
