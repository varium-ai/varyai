export function RoutingEmptyIllustration() {
  return (
    <svg
      className="rt-empty-illustration"
      width="128"
      height="80"
      viewBox="0 0 160 100"
      fill="none"
      aria-hidden="true"
      focusable="false"
    >
      <ellipse className="rt-empty-ground" cx="80" cy="91" rx="61" ry="4" />
      <g className="rt-empty-lines" strokeWidth="1.5" strokeLinecap="round">
        <path d="M57 49h22m0 0V29a8 8 0 0 1 8-8h26M79 49h34M79 49v20a8 8 0 0 0 8 8h26" />
        <path d="M16 28h28a8 8 0 0 1 8 8v25" />
      </g>
      <rect
        className="rt-empty-node"
        x="20"
        y="33"
        width="37"
        height="33"
        rx="8"
      />
      <g className="rt-empty-lines" strokeWidth="2" strokeLinecap="round">
        <path d="M30 44h16M30 50h10M30 56h13" />
      </g>
      <g className="rt-empty-node">
        <rect x="113" y="10" width="30" height="22" rx="6" />
        <rect x="113" y="38" width="30" height="22" rx="6" />
        <rect x="113" y="66" width="30" height="22" rx="6" />
      </g>
      <g className="rt-empty-lines" strokeWidth="2" strokeLinecap="round">
        <path d="M124 21h8M124 49h8M124 77h8" />
      </g>
      <circle className="rt-empty-hub" cx="79" cy="49" r="11" />
      <path
        className="rt-empty-arrow"
        d="M74 49h10m-4-4 5 4-5 4"
        strokeWidth="1.5"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
      <circle className="rt-empty-spark" cx="62" cy="15" r="2" />
      <circle className="rt-empty-spark" cx="150" cy="52" r="1.5" />
    </svg>
  );
}
