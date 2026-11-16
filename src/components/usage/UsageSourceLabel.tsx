import { useTranslation } from "react-i18next";
import { getUsageSourceLabel, type UsageSource } from "./source";

export function UsageSourceLabel({ source }: { source: UsageSource }) {
  const { t } = useTranslation();
  return <span>{getUsageSourceLabel(source, t)}</span>;
}
