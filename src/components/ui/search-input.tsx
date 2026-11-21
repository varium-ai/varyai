import * as React from "react";
import { Search } from "lucide-react";
import { Input, type InputProps } from "@/components/ui/input";
import { cn } from "@/lib/utils";

type SearchInputProps = Omit<InputProps, "type"> & {
  wrapperClassName?: string;
};

export const SearchInput = React.forwardRef<HTMLInputElement, SearchInputProps>(
  ({ wrapperClassName, className, ...props }, ref) => (
    <div className={cn("relative flex min-w-0 items-center", wrapperClassName)}>
      <Search
        aria-hidden="true"
        className="pointer-events-none absolute left-3 z-[1] h-4 w-4 text-muted-foreground"
      />
      <Input
        {...props}
        ref={ref}
        type="search"
        className={cn("pl-9", className)}
      />
    </div>
  ),
);
SearchInput.displayName = "SearchInput";
