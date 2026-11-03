import { useQuery } from "@tanstack/react-query";
import { proxyApi } from "@/lib/api/proxy";
import { LongConversationSettings } from "@/components/agents/LongConversationSettings";

export function LongConversationFallback() {
  const installations = useQuery({
    queryKey: ["agent-installations"],
    queryFn: proxyApi.getAgentInstallations,
    staleTime: 30000,
  });
  if (
    !installations.data ||
    installations.isError ||
    installations.data.codex ||
    installations.data.claude ||
    installations.data.grokbuild
  )
    return null;
  return <LongConversationSettings />;
}
