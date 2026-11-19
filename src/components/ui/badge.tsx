import * as React from "react";
import { cva, type VariantProps } from "class-variance-authority";

import { cn } from "@/lib/utils";

const badgeVariants = cva(
  "inline-flex min-h-[26px] items-center rounded border px-2 py-0.5 text-xs font-medium focus:outline-none focus:ring-2 focus:ring-ring focus:ring-offset-2",
  {
    variants: {
      variant: {
        default: "border-transparent bg-accent text-primary",
        secondary: "border-transparent bg-muted text-secondary-foreground",
        destructive:
          "border-transparent bg-[var(--danger-surface)] text-destructive",
        success:
          "border-transparent bg-[var(--success-surface)] text-[var(--success)]",
        warning:
          "border-transparent bg-[var(--warning-surface)] text-[var(--warning)]",
        outline: "text-foreground",
      },
    },
    defaultVariants: {
      variant: "default",
    },
  },
);

export interface BadgeProps
  extends React.HTMLAttributes<HTMLDivElement>,
    VariantProps<typeof badgeVariants> {}

function Badge({ className, variant, ...props }: BadgeProps) {
  return (
    <div className={cn(badgeVariants({ variant }), className)} {...props} />
  );
}

export { Badge, badgeVariants };
