import { useId, type ReactNode } from "react";
import { Switch } from "@/components/ui/switch";

export function SettingsGroup({
  title,
  children,
}: {
  title: string;
  children: ReactNode;
}) {
  const id = useId();
  return (
    <section aria-labelledby={id} className="settings-group">
      <h2 id={id}>{title}</h2>
      <div className="settings-list">{children}</div>
    </section>
  );
}

interface SettingRowProps {
  title: string;
  description: string;
  children: ReactNode;
}

export function SettingRow({ title, description, children }: SettingRowProps) {
  return (
    <div className="setting-row">
      <div className="setting-copy">
        <h3>{title}</h3>
        <p>{description}</p>
      </div>
      {children}
    </div>
  );
}

export function SettingSwitch({
  title,
  description,
  checked,
  onCheckedChange,
}: Omit<SettingRowProps, "children"> & {
  checked: boolean;
  onCheckedChange: (value: boolean) => void;
}) {
  return (
    <SettingRow title={title} description={description}>
      <Switch
        aria-label={title}
        checked={checked}
        onCheckedChange={onCheckedChange}
      />
    </SettingRow>
  );
}
