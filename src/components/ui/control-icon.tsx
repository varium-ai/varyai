import { cn } from "@/lib/utils";
import checkIcon from "@/assets/ui/select-check.svg";
import downIcon from "@/assets/ui/chevron-down.svg";

/** Original16px Figma control glyphs. */
export function ControlIcon({
  name,
  className,
}: {
  name: "check" | "chevron-down";
  className?: string;
}) {
  return (
    <img
      aria-hidden="true"
      alt=""
      src={name === "check" ? checkIcon : downIcon}
      width={16}
      height={16}
      className={cn(
        "block h-4 w-4 shrink-0",
        name === "chevron-down" && "dark:grayscale dark:brightness-[1.9]",
        className,
      )}
    />
  );
}
