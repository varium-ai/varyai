import * as React from "react";
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectLabel,
  SelectTrigger,
  SelectValue,
} from "./select";

type OptionProps = React.OptionHTMLAttributes<HTMLOptionElement>;
interface OptionSelectProps
  extends Omit<
    React.ComponentPropsWithoutRef<typeof SelectTrigger>,
    "value" | "defaultValue" | "onChange"
  > {
  value: string;
  onValueChange?: (value: string) => void;
}
const encode = (value: string) => `value:${value}`;

/** Renders existing option inventories in the application's themed listbox. */
export const OptionSelect = React.forwardRef<
  HTMLButtonElement,
  OptionSelectProps
>(({ value, onValueChange, children, disabled, ...triggerProps }, ref) => {
  function options(
    nodes: React.ReactNode,
    groupDisabled = false,
  ): React.ReactNode {
    return React.Children.map(nodes, (node) => {
      if (!React.isValidElement(node)) return null;
      if (node.type === React.Fragment)
        return options(node.props.children, groupDisabled);
      if (node.type === "optgroup")
        return (
          <SelectGroup>
            <SelectLabel>{node.props.label}</SelectLabel>
            {options(node.props.children, groupDisabled || node.props.disabled)}
          </SelectGroup>
        );
      if (node.type !== "option") return null;
      const props = node.props as OptionProps;
      const optionValue = String(props.value ?? props.children ?? "");
      return (
        <SelectItem
          value={encode(optionValue)}
          disabled={groupDisabled || props.disabled}
          data-option-value={optionValue}
        >
          {props.children}
        </SelectItem>
      );
    });
  }
  return (
    <Select
      value={encode(value)}
      onValueChange={(next) => onValueChange?.(next.slice(6))}
      disabled={disabled}
    >
      <SelectTrigger
        ref={ref}
        disabled={disabled}
        value={value}
        {...triggerProps}
      >
        <SelectValue />
      </SelectTrigger>
      <SelectContent>{options(children)}</SelectContent>
    </Select>
  );
});
OptionSelect.displayName = "OptionSelect";
