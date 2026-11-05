import type { ComponentProps } from "react";
import { useTranslation } from "react-i18next";
import type { AppId } from "@/lib/api";
import { SortableWorkspaceRow } from "@/components/SortableWorkspaceRow";

export function SortableAgentRow({
  app,
  name,
  sortable,
  ...props
}: ComponentProps<"article"> & {
  app: AppId;
  name: string;
  sortable: boolean;
}) {
  const { t } = useTranslation();
  return (
    <SortableWorkspaceRow
      {...props}
      id={app}
      sortable={sortable}
      handleClassName="mp-agent-drag-handle"
      handleLabel={t("agentConnections.sorting.handle", { app: name })}
      handleRole={t("agentConnections.sorting.role")}
      handleHint={t("agentConnections.sorting.hint")}
    />
  );
}
