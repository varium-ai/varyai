import { useRef } from "react";
import { ChevronDown, X } from "lucide-react";
import { useTranslation } from "react-i18next";
import {
  DropdownMenu,
  DropdownMenuTrigger,
  DropdownMenuContent,
  DropdownMenuCheckboxItem,
} from "@/components/ui/dropdown-menu";
import { routingPurposeTranslation } from "@/lib/routingPurposes";

export function RoutingPurposeFilter({
  purposes,
  selected,
  onChange,
}: {
  purposes: string[];
  selected: string[];
  onChange: (purposes: string[]) => void;
}) {
  const { t } = useTranslation();
  const trigger = useRef<HTMLButtonElement>(null);
  const options = [...new Set([...purposes, ...selected])].sort();
  if (options.length < 2 && !selected.length) return null;

  const name = (purpose: string) => {
    const key = routingPurposeTranslation(purpose);
    return key ? t(key) : purpose.slice(5);
  };
  const label = selected.length
    ? t("routingPage.selectedPurposes", {
        name: options
          .filter((purpose) => selected.includes(purpose))
          .map(name)
          .join(", "),
      })
    : t("routingPage.purpose");
  return (
    <div className={`rt-purpose-tools${selected.length ? " set" : ""}`}>
      <DropdownMenu modal={false}>
        <DropdownMenuTrigger asChild>
          <button
            type="button"
            className="text rt-purpose"
            ref={trigger}
            aria-label={label}
            title={`${t("routingPage.filterByPurpose")}${selected.length ? `\n${label}` : ""}`}
          >
            <span>{label}</span>
            <ChevronDown size={11} />
          </button>
        </DropdownMenuTrigger>
        <DropdownMenuContent
          align="end"
          className="mp-row-menu rt-purpose-menu"
          aria-label={t("routingPage.purpose")}
        >
          <DropdownMenuCheckboxItem
            checked={!selected.length}
            onSelect={(event) => event.preventDefault()}
            onCheckedChange={() => onChange([])}
          >
            {t("routingPage.allPurposes")}
          </DropdownMenuCheckboxItem>
          {options.map((purpose) => (
            <DropdownMenuCheckboxItem
              key={purpose}
              checked={selected.includes(purpose)}
              title={
                purpose === "unmarked"
                  ? t("routingPage.unmarkedPurposeHint")
                  : purpose === "kind:unknown" ||
                      !routingPurposeTranslation(purpose)
                    ? t("routingPage.unknownPurposeHint")
                    : undefined
              }
              onSelect={(event) => event.preventDefault()}
              onCheckedChange={(checked) =>
                onChange(
                  checked
                    ? [...selected, purpose]
                    : selected.filter((value) => value !== purpose),
                )
              }
            >
              {name(purpose)}
            </DropdownMenuCheckboxItem>
          ))}
        </DropdownMenuContent>
      </DropdownMenu>
      {selected.length > 0 && (
        <button
          type="button"
          className="text rt-purpose-clear"
          aria-label={t("routingPage.clearPurpose")}
          title={t("routingPage.clearPurpose")}
          onClick={() => {
            onChange([]);
            trigger.current?.focus({ preventScroll: true });
          }}
        >
          <X size={11} />
        </button>
      )}
    </div>
  );
}
