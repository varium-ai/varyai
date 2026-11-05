import { createContext, useContext, type HTMLAttributes } from "react";
import "./account-label.css";

const AccountMaskContext = createContext<{ masked: boolean; names: string[] }>({
  masked: false,
  names: [],
});

export function AccountMask({
  as: Element = "div",
  masked,
  names,
  ...props
}: HTMLAttributes<HTMLElement> & {
  as?: "div" | "section";
  masked: boolean;
  names?: string[];
}) {
  const parent = useContext(AccountMaskContext);
  return (
    <AccountMaskContext.Provider
      value={{ masked, names: names ?? parent.names }}
    >
      <Element {...props} />
    </AccountMaskContext.Provider>
  );
}

function accountPattern(names: string[]) {
  const escape = (name: string) => name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const accounts = [...new Set(names.map((name) => name.trim()))]
    .filter((name) => name.length > 1 && !name.includes("@"))
    .sort((a, b) => b.length - a.length)
    .map(
      (name) => `(?<![\\p{L}\\p{N}_.-])${escape(name)}(?![\\p{L}\\p{N}_.-])`,
    );
  return new RegExp(
    ["[\\w.+*•-]+@[\\w*•-]+(?:\\.[\\w*•-]+)+", ...accounts].join("|"),
    "gu",
  );
}

export function maskedAccountText(
  text: string,
  masked: boolean,
  names: string[] = [],
): string {
  return masked
    ? text.replace(accountPattern(names), (account) =>
        account.includes("@")
          ? account.replace(/[^@.]/g, "•")
          : account.replace(/./gu, "•"),
      )
    : text;
}

export function useAccountText() {
  const { masked, names } = useContext(AccountMaskContext);
  return (text: string) => maskedAccountText(text, masked, names);
}

export function AccountLabel({
  children,
  title = false,
}: {
  children: string;
  title?: boolean;
}) {
  const { masked, names } = useContext(AccountMaskContext);
  if (!masked)
    return title ? <span title={children}>{children}</span> : <>{children}</>;
  const parts = [];
  let last = 0;
  for (const match of children.matchAll(accountPattern(names))) {
    const start = match.index;
    if (start > last) parts.push(children.slice(last, start));
    parts.push(
      <span className="account-mask" key={start}>
        {Array.from(match[0], (character, index) =>
          character === "@" || character === "."
            ? character
            : "abcdefghkmnoprstuvwxz"[
                ((character.codePointAt(0) ?? 0) + index * 7) % 20
              ],
        ).join("")}
      </span>,
    );
    last = start + match[0].length;
  }
  if (!last)
    return title ? <span title={children}>{children}</span> : <>{children}</>;
  if (last < children.length) parts.push(children.slice(last));
  return (
    <span
      className="account-label"
      title={title ? maskedAccountText(children, true, names) : undefined}
    >
      {parts}
    </span>
  );
}
