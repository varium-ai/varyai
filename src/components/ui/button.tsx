import * as React from "react";
import { Slot } from "@radix-ui/react-slot";
import { cva, type VariantProps } from "class-variance-authority";
import { cn } from "@/lib/utils";

const buttonVariants = cva(
  "inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-md text-[13px] font-medium transition-colors duration-[var(--interaction-duration)] ease-[var(--interaction-easing)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-card disabled:pointer-events-none disabled:[&:not([aria-busy=true])]:bg-muted disabled:[&:not([aria-busy=true])]:text-[var(--disabled-foreground)]",
  {
    variants: {
      variant: {
        default:
          "bg-primary text-primary-foreground hover:bg-[var(--primary-hover)] active:bg-[var(--primary-hover)]",
        destructive:
          "bg-[var(--danger-surface)] text-destructive hover:bg-[color-mix(in_srgb,var(--danger-surface),currentColor_10%)] active:bg-[color-mix(in_srgb,var(--danger-surface),currentColor_10%)]",
        outline:
          "border border-border bg-card text-secondary-foreground hover:bg-[var(--hover-surface)] active:bg-accent",
        secondary:
          "border border-border bg-card text-secondary-foreground hover:bg-[var(--hover-surface)] active:bg-accent",
        ghost:
          "text-secondary-foreground hover:bg-[var(--hover-surface)] active:bg-accent",
        mcp: "bg-emerald-500 text-white hover:bg-emerald-600 dark:bg-emerald-600 dark:hover:bg-emerald-700",
        link: "text-primary underline-offset-4 hover:underline",
      },
      size: {
        default: "h-9 px-4 py-2",
        sm: "h-8 px-3 text-xs",
        lg: "h-10 rounded-md px-8",
        icon: "h-8 w-8 rounded-sm p-1.5 [&_svg]:h-4 [&_svg]:w-4",
      },
    },
    defaultVariants: {
      variant: "default",
      size: "default",
    },
  },
);

export interface ButtonProps
  extends React.ButtonHTMLAttributes<HTMLButtonElement>,
    VariantProps<typeof buttonVariants> {
  asChild?: boolean;
}

const Button = React.forwardRef<HTMLButtonElement, ButtonProps>(
  ({ className, variant, size, asChild = false, ...props }, ref) => {
    const Comp = asChild ? Slot : "button";
    return (
      <Comp
        className={cn(buttonVariants({ variant, size, className }))}
        data-variant={variant ?? "default"}
        ref={ref}
        {...props}
      />
    );
  },
);
Button.displayName = "Button";

export { Button, buttonVariants };
