// The reference implementation's request/hold/return sequence (MIT), rendered as packets without birds.
// See third_party/routing-reference/LICENSE.
import { useEffect, useRef, type CSSProperties } from "react";
import type { RoutingTrace } from "@/lib/api/routingTrace";

type Position = "source" | "hub" | number;
type Leg = { wire: number; reverse?: boolean };
type Flight = {
  legs: Leg[];
  duration: number;
  back?: boolean;
  error?: boolean;
  cancelled?: boolean;
  to: Position;
};
type Trace = RoutingTrace;

export function RoutingPackets({
  row,
  paths,
  flights,
  onFinished,
  active,
  replay = false,
  speed = 1,
}: {
  row?: Trace;
  paths: string[];
  flights?: {
    row: Trace;
    paths: string[];
    color?: string;
    holdOffset?: number;
  }[];
  onFinished?: (id: string) => void;
  active: boolean;
  replay?: boolean;
  speed?: number;
}) {
  const entries = flights ?? [{ row, paths }];
  return (
    <svg className="rt-sky" aria-hidden="true">
      {entries.map((entry) => (
        <PacketFlight
          key={entry.row?.id ?? "idle"}
          {...entry}
          active={active}
          replay={replay}
          speed={speed}
          onFinished={onFinished}
        />
      ))}
    </svg>
  );
}

function PacketFlight({
  row,
  paths,
  color,
  holdOffset = 0,
  active,
  replay,
  speed,
  onFinished,
}: {
  row?: Trace;
  paths: string[];
  color?: string;
  holdOffset?: number;
  active: boolean;
  replay: boolean;
  speed: number;
  onFinished?: (id: string) => void;
}) {
  const svg = useRef<SVGGElement>(null);
  const player = useRef<ReturnType<typeof packetPlayer>>();
  const finished = useRef(onFinished);
  finished.current = onFinished;
  useEffect(() => {
    if (!svg.current) return;
    const current = packetPlayer(svg.current, (id) => finished.current?.(id));
    player.current = current;
    return () => {
      current.dispose();
      player.current = undefined;
    };
  }, []);
  useEffect(() => {
    player.current?.update(row, active, replay, speed);
  }, [row, active, replay, speed]);
  useEffect(() => {
    player.current?.layout(holdOffset);
  }, [paths, holdOffset]);
  return (
    <g
      ref={svg}
      data-flight-id={row?.id}
      style={color ? ({ "--rt-agent": color } as CSSProperties) : undefined}
    >
      <g className="rt-packet" visibility="hidden">
        <circle className="rt-packet-halo" r="10" />
        <circle className="pkt" r="4.5" />
      </g>
      {paths.map((path, index) => (
        <path className="rt-packet-path" d={path} key={index} />
      ))}
    </g>
  );
}

function packetPlayer(svg: SVGGElement, onFinished: (id: string) => void) {
  const packet = svg.querySelector<SVGGElement>(".rt-packet")!;
  const media = window.matchMedia?.("(prefers-reduced-motion: reduce)");
  let reduced = media?.matches ?? false;
  let id: string | undefined;
  let replaying = false;
  let activeMode = false;
  let allowed = false;
  let rate = 1;
  let seen = 0;
  let ended = false;
  let reported = false;
  let position: Position = "source";
  let destination: Position = "source";
  let queue: Flight[] = [];
  let flight: Flight | undefined;
  let started: number | undefined;
  let elapsed = 0;
  let frame: number | undefined;
  let disposed = false;
  let holdOffset = 0;
  const wires = () => [
    ...svg.querySelectorAll<SVGPathElement>(".rt-packet-path"),
  ];

  function hide() {
    packet.setAttribute("visibility", "hidden");
    packet.classList.remove("held", "back", "err", "cancelled");
  }
  function reportFinished() {
    if (!reported && id) {
      reported = true;
      onFinished(id);
    }
  }
  function cancelFrame() {
    if (frame !== undefined) cancelAnimationFrame(frame);
    frame = undefined;
  }
  function clear() {
    cancelFrame();
    queue = [];
    flight = undefined;
    started = undefined;
    elapsed = 0;
    position = "source";
    destination = "source";
    hide();
  }
  function pose(legs: Leg[], progress: number, inset = 0) {
    const paths = wires();
    if (
      !legs.length ||
      legs.some(
        (leg) =>
          typeof paths[leg.wire]?.getTotalLength !== "function" ||
          typeof paths[leg.wire]?.getPointAtLength !== "function",
      )
    )
      return false;
    // Re-read lengths after layout, so a moving or held dot stays on its wire.
    let lengths: number[];
    try {
      lengths = legs.map((leg) => paths[leg.wire].getTotalLength());
    } catch {
      return false;
    }
    const total = lengths.reduce((sum, length) => sum + length, 0);
    if (!Number.isFinite(total) || total <= 0) return false;
    let distance = total * Math.max(0, Math.min(progress, 1));
    if (inset) distance = Math.max(total * 0.15, distance - inset);
    let index = 0;
    while (index < legs.length - 1 && distance > lengths[index]) {
      distance -= lengths[index++];
    }
    let point: DOMPoint;
    try {
      point = paths[legs[index].wire].getPointAtLength(
        legs[index].reverse ? lengths[index] - distance : distance,
      );
    } catch {
      return false;
    }
    if (!Number.isFinite(point.x) || !Number.isFinite(point.y)) return false;
    const slot = Math.ceil(inset / 12);
    const offset =
      inset > total * 0.85 ? (slot % 2 ? 1 : -1) * Math.ceil(slot / 2) * 8 : 0;
    packet.setAttribute(
      "transform",
      `translate(${point.x} ${point.y + offset})`,
    );
    packet.setAttribute("visibility", "visible");
    return true;
  }
  function hold() {
    if (ended && position === "source") reportFinished();
    if (!allowed || reduced || ended || position === "source") return hide();
    const wire = position === "hub" ? 0 : position + 1;
    if (pose([{ wire }], 1, holdOffset)) packet.classList.add("held");
  }
  function schedule() {
    if (
      disposed ||
      reduced ||
      document.hidden ||
      !allowed ||
      frame !== undefined
    )
      return;
    if (flight || queue.length) frame = requestAnimationFrame(tick);
  }
  function tick(time: number) {
    frame = undefined;
    started ??= time - elapsed / rate;
    if (!flight) flight = queue.shift();
    while (flight) {
      packet.classList.toggle("back", !!flight.back);
      packet.classList.toggle("err", !!flight.error);
      packet.classList.toggle("cancelled", !!flight.cancelled);
      packet.classList.remove("held");
      elapsed = (time - started) * rate;
      if (!pose(flight.legs, elapsed / flight.duration)) {
        // Geometry may arrive a render later. Layout resumes the same flight.
        started = undefined;
        elapsed = 0;
        hide();
        return;
      }
      if (elapsed < flight.duration) break;
      position = flight.to;
      started += flight.duration / rate;
      flight = queue.shift();
    }
    if (flight) schedule();
    else {
      started = undefined;
      elapsed = 0;
      hold();
    }
  }
  function home(error: boolean, cancelled = false) {
    const legs: Leg[] = [];
    if (typeof destination === "number")
      legs.push({ wire: destination + 1, reverse: true });
    if (destination !== "source") legs.push({ wire: 0, reverse: true });
    if (legs.length)
      queue.push({
        legs,
        duration: legs.length > 1 ? 1400 : 620,
        back: true,
        error,
        cancelled,
        to: "source",
      });
    destination = "source";
  }
  function update(
    row: Trace | undefined,
    active: boolean,
    replay: boolean,
    speed: number,
  ) {
    const nextRate = Math.max(0.25, Math.min(speed, 8));
    if (nextRate !== rate) {
      started = undefined;
      rate = nextRate;
    }
    const reset =
      row?.id !== id ||
      replay !== replaying ||
      active !== activeMode ||
      (row?.steps.length ?? 0) < seen;
    if (reset || !active) {
      clear();
      seen = 0;
      ended = false;
      reported = false;
      id = row?.id;
      replaying = replay;
      activeMode = active;
      // A sub-second request can already be done in its first polling snapshot.
      allowed =
        active &&
        !!row &&
        (replay ||
          row.state === "routing" ||
          row.state === "streaming" ||
          row.startedAt + row.durationMs >= Date.now() - 5000);
      if (allowed) {
        queue.push({ legs: [{ wire: 0 }], duration: 620, to: "hub" });
        destination = "hub";
      }
    }
    if (!row || !allowed) return;
    if (reduced) {
      if (!["routing", "streaming"].includes(row.state)) reportFinished();
      return;
    }
    for (const step of row.steps.slice(seen)) {
      if (
        step.kind === "attempt" &&
        step.candidate != null &&
        step.candidate >= 0
      ) {
        if (typeof destination === "number") {
          queue.push({
            legs: [{ wire: destination + 1, reverse: true }],
            duration: 620,
            back: true,
            to: "hub",
          });
        }
        queue.push({
          legs: [{ wire: step.candidate + 1 }],
          duration: 780,
          to: step.candidate,
        });
        destination = step.candidate;
      } else if (
        ["rejected", "first_token_timeout"].includes(step.kind) &&
        typeof destination === "number"
      ) {
        queue.push({
          legs: [{ wire: destination + 1, reverse: true }],
          duration: 620,
          back: true,
          error: true,
          to: "hub",
        });
        destination = "hub";
      } else if (
        ["completed", "failed", "cancelled"].includes(step.kind) &&
        !ended
      ) {
        home(step.kind === "failed", step.kind === "cancelled");
        ended = true;
      }
    }
    seen = row.steps.length;
    if (!["routing", "streaming"].includes(row.state) && !ended) {
      home(row.state === "failed", row.state === "cancelled");
      ended = true;
    }
    schedule();
  }
  function visibility() {
    cancelFrame();
    started = undefined;
    if (!document.hidden) schedule();
  }
  function motion(event: MediaQueryListEvent) {
    reduced = event.matches;
    if (reduced) {
      clear();
      if (ended) reportFinished();
    }
  }
  document.addEventListener("visibilitychange", visibility);
  media?.addEventListener("change", motion);
  return {
    update,
    layout(offset = 0) {
      holdOffset = offset;
      if (flight) {
        pose(flight.legs, elapsed / flight.duration);
        schedule();
      } else if (queue.length) schedule();
      else hold();
    },
    dispose() {
      disposed = true;
      clear();
      document.removeEventListener("visibilitychange", visibility);
      media?.removeEventListener("change", motion);
    },
  };
}
