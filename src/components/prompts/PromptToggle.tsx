import React from "react";

interface PromptToggleProps {
  enabled: boolean;
  onChange: (enabled: boolean) => void;
  disabled?: boolean;
}

const PromptToggle: React.FC<PromptToggleProps> = ({
  enabled,
  onChange,
  disabled = false,
}) => {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={enabled}
      disabled={disabled}
      onClick={() => onChange(!enabled)}
      className={`
        peer inline-flex h-6 w-11 shrink-0 items-center rounded-full border-2 border-transparent shadow transition-colors
        focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2
        ${enabled ? "bg-primary" : "bg-gray-200 dark:bg-gray-900"}
        ${disabled ? "opacity-50 cursor-not-allowed" : "cursor-pointer"}
      `}
    >
      <span
        className={`
          pointer-events-none block h-5 w-5 rounded-full bg-white dark:bg-gray-400 shadow-lg ring-0 transition-transform
          ${enabled ? "translate-x-5" : "translate-x-0"}
        `}
      />
    </button>
  );
};

export default PromptToggle;
