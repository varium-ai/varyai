// The reference implementation's request/hold/return sequence (MIT), rendered as packets without birds.
// See third_party/routing-reference/LICENSE.
import { useEffect, useRef } from "react";
import type { RoutingTrace } from "@/lib/api/routingTrace";

type Position = "source" | "hub" | number;
type Leg = { wire: number; reverse?: boolean };
type Flight = {
  legs: Leg[];
  duration: number;
  back?: boolean;
  error?: boolean;
  to: Position;
};
type Trace = RoutingTrace;

export function RoutingPackets({
  row,
  paths,
  active,
  replay = false,
  speed = 1,
}: {
  row?: Trace;
  paths: string[];
  active: boolean;
  replay?: boolean;
  speed?: number;
}) {
  const svg = useRef<SVGSVGElement>(null);
  const player = useRef<ReturnType<typeof packetPlayer>>();
  useEffect(() => {
    if (!svg.current) return;
    const current = packetPlayer(svg.current);
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
    player.current?.layout();
  }, [paths]);
  return (
    <svg className="rt-sky" ref={svg} aria-hidden="true">
      <g className="rt-packet" visibility="hidden">
        <circle className="rt-packet-halo" r="10" />
        <circle className="pkt" r="4.5" />
      </g>
      {paths.map((path, index) => (
        <path className="rt-packet-path" d={path} key={index} />
      ))}
    </svg>
  );
}

function packetPlayer(svg: SVGSVGElement) {
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
  let position: Position = "source";
  let destination: Position = "source";
  let queue: Flight[] = [];
  let flight: Flight | undefined;
  let started: number | undefined;
  let elapsed = 0;
  let frame: number | undefined;
  let disposed = false;
  const wires = () => [
    ...svg.querySelectorAll<SVGPathElement>(".rt-packet-path"),
  ];

  function hide() {
    packet.setAttribute("visibility", "hidden");
    packet.classList.remove("held", "back", "err");
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
  function pose(legs: Leg[], progress: number) {
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
    packet.setAttribute("transform", `translate(${point.x} ${point.y})`);
    packet.setAttribute("visibility", "visible");
    return true;
  }
  function hold() {
    if (!allowed || reduced || ended || position === "source") return hide();
    const wire = position === "hub" ? 0 : position + 1;
    if (pose([{ wire }], 1)) packet.classList.add("held");
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
  function home(error: boolean) {
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
          row.startedAt >= Date.now() - 5000);
      if (allowed) {
        queue.push({ legs: [{ wire: 0 }], duration: 620, to: "hub" });
        destination = "hub";
      }
    }
    if (!row || !allowed || reduced) return;
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
        home(step.kind !== "completed");
        ended = true;
      }
    }
    seen = row.steps.length;
    if (!["routing", "streaming"].includes(row.state) && !ended) {
      home(row.state !== "completed");
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
    if (reduced) clear();
  }
  document.addEventListener("visibilitychange", visibility);
  media?.addEventListener("change", motion);
  return {
    update,
    layout() {
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
