import { ProviderIcon } from "@/components/ProviderIcon";
import { providerImportSources } from "@/config/providerImportSources";

export function ProviderImportSourceIcon({
  sourceId,
  name,
  size = 17,
}: {
  sourceId: string;
  name: string;
  size?: number;
}) {
  const source = providerImportSources.find((entry) => entry.id === sourceId);
  return source?.image ? (
    <img
      src={source.image}
      alt=""
      width={size}
      height={size}
      className="object-contain"
    />
  ) : (
    <ProviderIcon icon={source?.icon} name={name} size={size} />
  );
}
