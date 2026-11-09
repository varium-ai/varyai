import { PlusCircle } from "lucide-react";

export type SubscriptionAccountAction = {
  key: string;
  label: string;
  onClick: () => void;
};

export function SubscriptionAccountActions({
  actions,
  disabled,
}: {
  actions: SubscriptionAccountAction[];
  disabled: boolean;
}) {
  return (
    <>
      {actions.map((action) => (
        <button
          key={action.key}
          type="button"
          className="cx-add-account"
          disabled={disabled}
          onClick={action.onClick}
        >
          <PlusCircle size={16} aria-hidden />
          <span>{action.label}</span>
        </button>
      ))}
    </>
  );
}
