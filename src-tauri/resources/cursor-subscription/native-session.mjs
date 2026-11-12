import { createHash, randomBytes } from "node:crypto"
import { mkdirSync, readFileSync, readdirSync, renameSync, rmSync, writeFileSync } from "node:fs"
import { join } from "node:path"

// Retention is process-local and optional. A lost owner rebuilds Chat history
// on a fresh Run; it never resubmits a result to an uncertain native exec.
// Checkpoints are retained evidence, not restart handles.
export const NATIVE_LIMITS = {
  resultWait: 120_000,
  lifetime: 15 * 60_000,
  // How much of the lifetime a retained Run must have left to take a tool
  // batch's results and go on; with less, the step starts on a fresh Run
  // from the supplied history. One step can be a long answer: a Pi turn on
  // grok-4.7 went on with 494 s left and was still answering at the
  // deadline (plugins#616, yetone/magpie#4419). At most two thirds of the
  // lifetime, so a short one set in a test still has room to continue.
  continueReserve: 10 * 60_000,
  batchWait: 30_000,
  // How long a stateless step whose calls came waits for Cursor's count of
  // them (update 27) with nothing but heartbeats before it hands the calls
  // that came to the caller (plugins#550). A count comes within ms of the
  // step's last call; a Run that sent calls and then only heartbeats has
  // stopped, and the caller's next request goes on from the calls' results.
  countWait: 60_000,
  drain: 6000,
  replayWait: 120_000,
  blobBytes: 64 << 20,
  outputBytes: 16 << 20,
  argumentBytes: 1 << 20,
  argumentFields: 100_000,
  argumentDepth: 64,
  outboundBytes: 16 << 20,
  outboundMessages: 256,
  // How long a KV reply waits for room in the outbound queue (Cursor's own
  // flow control taking the writes before it) before the Run fails.
  writeWait: 30_000,
  // How long a Run waits on Cursor before it ends as gone quiet
  // (yetone/magpie#2672): idle, with no message at all, as @cursor/sdk's
  // stall detector ends one (30 s there; Cursor's client retries, this one
  // can't once output went out); quiet, with heartbeats and nothing else,
  // under the 5 minutes magpie keeps a silent reply alive. Only time spent
  // waiting on Cursor counts, never a Run parked for the caller's results.
  idle: 60_000,
  quiet: 240_000,
  perAccount: 8,
  perProcess: 32,
}

const HANDLE = /^call_cursor_[a-f0-9]+_\d+_\d+$/
const hash = (s) => createHash("sha256").update(s).digest("hex")
const canonical = (v) => JSON.stringify(v, (_k, x) => x && typeof x === "object" && !Array.isArray(x)
  ? Object.fromEntries(Object.keys(x).sort().map((k) => [k, x[k]])) : x)
const bytes = (v) => Buffer.byteLength(canonical(v))
const later = (fn, ms) => {
  const timer = setTimeout(fn, ms)
  timer.unref?.()
  return timer
}

export class NativeSessionError extends Error {
  constructor(status, message) {
    super(message)
    this.status = status
  }
}
const invalid = (message, status = 409) => { throw new NativeSessionError(status, message) }
const failureOf = (e) => ({ status: e?.status ?? 502, message: e?.message ?? String(e) })

// kindOf is what a frame of AgentServerMessage is, for Quiet: a heartbeat
// alone, or what an error names as the last thing Cursor sent
// (names after cursor-agent's agent.v1 InteractionUpdate, 2026.10.01)
const UPDATES = {
  1: "text", 2: "tool call started", 3: "tool call completed", 4: "thinking", 5: "thinking done",
  6: "user message", 7: "partial tool call", 8: "token count", 9: "summary", 10: "summary started",
  11: "summary done", 12: "shell output", 14: "turn end", 15: "tool call delta", 16: "step started",
  17: "step done", 27: "tool count", 31: "model call completed",
}
const MESSAGES = { 2: "tool call", 3: "checkpoint", 4: "blob request" }
function kindOf(f) {
  if (f.end) return "stream end"
  try {
    const fs = nativeFields(f.data)
    const updates = fs.filter((m) => m.num === 1).flatMap((m) => nativeFields(m.data))
    if (fs.length && fs.every((m) => m.num === 1) && updates.length && updates.every((u) => u.num === 13)) return null
    const u = updates.find((u) => u.num !== 13)
    if (u) return UPDATES[u.num] ?? `update #${u.num}`
    return MESSAGES[fs[0]?.num] ?? `message #${fs[0]?.num}`
  } catch {
    return "malformed message"
  }
}

// Quiet ends a Run Cursor has stopped answering. Cursor can leave a Run's
// stream open after some thinking with nothing more to come, no text, call,
// count or TurnEnded, while the plugin's heartbeats keep it alive: the
// reply then waited until the agent gave up (305 s, yetone/magpie#2672).
// Only time spent waiting on Cursor's next frame counts: not this side
// handling one, not a slow reader of the reply, not a paused Run.
// meta names the Run for a wire trace (path, model); with the trace off it
// is unused.
export class Quiet {
  constructor(limits, stop, meta = {}) {
    this.limits = limits
    this.stop = stop
    this.heard = 0 // ms waited since Cursor's last frame that wasn't a heartbeat
    this.heartbeats = 0
    this.last = ""
    this.since = null
    this.paused = false
    this.waiting = false
    this.why = ""
    // what the error says of a checkpoint (plugins#550): its number in the
    // Run, which of its step's it was (step is 0 where the path doesn't
    // count steps), and the kinds of the frames before it, run-length
    this.checkpoints = 0
    this.step = 0
    this.stepCheckpoints = 0
    this.trail = []
    this.trace = WireTrace.open(meta)
  }
  arm() {
    clearTimeout(this.timer)
    if (this.paused || !this.waiting || this.why || this.done) return
    this.since = Date.now()
    this.timer = later(() => this.fire(), Math.max(0, Math.min(this.limits.idle, this.limits.quiet - this.heard)))
  }
  // waited stops the clock: Cursor answered, or the Run paused
  waited() {
    clearTimeout(this.timer)
    if (this.since != null) this.heard += Date.now() - this.since
    this.since = null
  }
  fire() {
    const gap = Date.now() - this.since
    const what = this.heard + gap >= this.limits.quiet ? `${this.heartbeats} heartbeats and nothing else for ${Math.round((this.heard + gap) / 1000)} s` : `no message for ${Math.round(gap / 1000)} s`
    this.why = `Cursor went quiet: ${what}${this.last ? ", its last message " + this.lastSaid() : ""} (try again)`
    this.trace?.event("quiet", { heard: this.heard + gap, heartbeats: this.heartbeats })
    this.gone?.()
    this.stop({ status: 502, message: this.why })
    this.failed({ status: 502, message: this.why })
  }
  // lastSaid is the last frame's kind as the error names it; a checkpoint
  // with its number, its place in the step and what came before it
  lastSaid() {
    if (this.last !== "checkpoint") return this.last
    const row = this.trail.at(-1)?.[1] ?? 1
    const before = this.trail.slice(0, -1).map(([k, n]) => n > 1 ? `${k} ×${n}` : k).join(", ")
    const nth = this.stepCheckpoints === 1 ? "the first" : `number ${this.stepCheckpoints}`
    return `checkpoint #${this.checkpoints} of the Run`
      + (this.step ? `, ${nth} of step ${this.step}` : "")
      + (row > 1 ? `, ${row} in a row` : "")
      + `, after ${before || "nothing else"}`
  }
  // nextStep is the Run's next model call (NativeSession counts them)
  nextStep(n) {
    this.step = n
    this.stepCheckpoints = 0
    this.trace?.event("step", { n })
  }
  // failed is how the Run ended, when it failed: a wire trace, if on, is
  // written then (once)
  failed(error) {
    this.trace?.write(error, this)
  }
  // pause stops the clock while the Run waits on the caller's tool results;
  // resume starts it again from nothing, for Cursor's next model call
  pause() {
    this.waited()
    this.paused = true
    this.trace?.event("pause")
  }
  resume() {
    this.paused = false
    this.heard = 0
    this.heartbeats = 0
    this.trace?.event("resume")
    this.arm()
  }
  // frames passes on a Run's frames; once Cursor has gone quiet, the read
  // that was waiting ends in its error, whether or not the stream does
  async *frames(stream) {
    const it = (stream[Symbol.asyncIterator] ?? stream[Symbol.iterator]).call(stream)
    try {
      for (;;) {
        this.waiting = true
        this.arm()
        let r
        try {
          const next = Promise.resolve(it.next())
          next.catch(() => {}) // a read left behind by gone fails unheard
          // a wake of this read's own: one that lived as long as the stream
          // would hold every race's result, each frame, until it ended
          r = await Promise.race([next, new Promise((wake) => { this.gone = wake })])
        } catch (e) {
          if (!this.why) throw e
        } finally {
          this.gone = null
          this.waiting = false
          this.waited()
        }
        if (this.why) throw Object.assign(new NativeSessionError(502, this.why), { quiet: true })
        if (r.done) {
          this.trace?.event("eof")
          return
        }
        const kind = kindOf(r.value)
        this.trace?.frame("in", r.value, kind)
        if (kind == null) this.heartbeats++
        else {
          [this.heard, this.heartbeats, this.last] = [0, 0, kind]
          if (kind === "checkpoint") {
            this.checkpoints++
            this.stepCheckpoints++
          }
          if (this.trail.at(-1)?.[0] === kind) this.trail.at(-1)[1]++
          else {
            this.trail.push([kind, 1])
            if (this.trail.length > 4) this.trail.shift()
          }
        }
        yield r.value
      }
    } finally {
      this.done = true
      clearTimeout(this.timer)
      // not waited for: a read may still be pending on a stream that didn't
      // end, and the reader's error handling (a failed protocol check failing
      // the Run before a sealed batch is read) must not wait behind it
      Promise.resolve(it.return?.()).catch(() => {})
    }
  }
}

// The wire trace (plugins#550, off by default; the plugin's Wire trace
// option): a failed Run's frames, in and out, written to a JSON file for a
// bug report. A frame is kept as its kind, size, top-level field numbers
// with their wire types and sizes, and those fields' own field numbers and
// sizes one level down: the shape of the protocol, never its bytes. No
// text, thinking, tool name or argument, blob or blob id, checkpoint
// content, token, header, account or conversation id is read into it; the
// error's message is scrubbed of anything shaped like an email, id or
// token. traceDir is the folder, or "" while off, asked at each Run's start.
let traceDir = () => ""
export function setWireTrace(dir) {
  traceDir = typeof dir === "function" ? dir : () => dir || ""
}
const TRACE_HEAD = 2000 // the Run's first frames, kept
const TRACE_TAIL = 3000 // and its last, the ones in between counted only
const TRACE_FILES = 20 // the newest traces kept in the folder
// shape is a message's fields as [number, wire, size] (a varint's size 0),
// with children: the fields' own fields, for a field that is a message
function shape(data, children) {
  return nativeFields(data).map((f) => {
    const own = [f.num, f.wire, f.wire === 0 ? 0 : f.data.length]
    if (!children || f.wire !== 2) return own
    try {
      return [...own, nativeFields(f.data).map((c) => [c.num, c.wire, c.wire === 0 ? 0 : c.data.length])]
    } catch {
      return own
    }
  })
}
export function scrub(s) {
  return String(s ?? "")
    .replace(/[\w.+-]+@[\w-]+(\.[\w-]+)+/g, "<email>")
    .replace(/[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/gi, "<id>")
    .replace(/(auth0|user|google-oauth2|github)\|[\w-]+/gi, "<account>")
    .replace(/[A-Za-z0-9_\-+/=.]{24,}/g, "<redacted>")
    .slice(0, 2000)
}
// detail is what a trace keeps of an inbound frame past its shape, all of
// it numbers of Cursor's protocol, never content (plugins#550): a tool
// count's number, the kind of call (ToolCall's field) a call update is
// of, and how many calls a checkpoint's state waits on
function detail(data) {
  const out = {}
  for (const m of nativeFields(data)) {
    if (m.num === 1) {
      for (const u of nativeFields(m.data)) {
        if (u.num === 27) out.count = num(nativeFields(u.data), 1)
        else if (u.num === 2 || u.num === 3 || u.num === 7) {
          const tc = field(nativeFields(u.data), 2)
          const kind = tc && nativeFields(tc.data)[0]
          if (kind) out.tool = kind.wire === 2 ? [kind.num, nativeFields(kind.data)[0]?.num ?? 0] : [kind.num]
        }
      }
    } else if (m.num === 3) out.pending = pendingCalls(m.data)
  }
  return out
}
export class WireTrace {
  static open(meta) {
    let dir = ""
    try { dir = traceDir() } catch {}
    return dir ? new WireTrace(dir, meta) : null
  }
  constructor(dir, meta = {}) {
    this.dir = dir
    this.meta = { path: String(meta.path ?? ""), model: String(meta.model ?? "") }
    // why a Run went stateless: the retention fallback's reason
    if (meta.why) this.meta.why = String(meta.why)
    this.started = Date.now()
    this.head = []
    this.tail = []
    this.dropped = 0
  }
  push(entry) {
    if (this.written) return
    if (this.head.length < TRACE_HEAD) return void this.head.push(entry)
    this.tail.push(entry)
    if (this.tail.length > TRACE_TAIL) {
      this.tail.shift()
      this.dropped++
    }
  }
  // frame is one frame of the Run: "in" from Cursor ({data} or {end, data}),
  // "out" a message this side sent
  frame(dir, f, kind) {
    const t = Date.now() - this.started
    const data = dir === "in" ? f.data : f
    const entry = { t, dir, size: data?.length ?? 0 }
    if (dir === "in") entry.kind = kind ?? "heartbeat"
    if (dir === "in" && f.end) entry.end = true
    else try { entry.fields = shape(data, true) } catch { entry.malformed = true }
    if (dir === "in" && !f.end) try { Object.assign(entry, detail(data)) } catch {}
    this.push(entry)
  }
  event(name, extra = {}) {
    this.push({ t: Date.now() - this.started, event: name, ...extra })
  }
  // write saves the trace once, when the Run failed; a write that fails
  // leaves the Run's error as it was
  write(error, quiet) {
    if (this.written) return
    this.written = true
    try {
      const at = new Date(this.started).toISOString().replace(/[:.]/g, "-")
      const body = {
        trace: 1,
        plugin: "@magpie-community/opencode-cursor-auth",
        version: version(),
        ...this.meta,
        started: new Date(this.started).toISOString(),
        ms: Date.now() - this.started,
        error: { status: error?.status ?? 0, message: scrub(error?.message) },
        quiet: quiet ? { last: quiet.last, checkpoints: quiet.checkpoints, step: quiet.step, stepCheckpoints: quiet.stepCheckpoints, heartbeats: quiet.heartbeats, heard: quiet.heard, trail: quiet.trail } : undefined,
        frames: [...this.head, ...(this.dropped ? [{ dropped: this.dropped }] : []), ...this.tail],
      }
      mkdirSync(this.dir, { recursive: true })
      const file = join(this.dir, `${at}-${randomBytes(3).toString("hex")}.json`)
      writeFileSync(file + ".tmp", JSON.stringify(body) + "\n", { mode: 0o600 })
      renameSync(file + ".tmp", file)
      this.file = file
      const old = readdirSync(this.dir).filter((n) => n.endsWith(".json")).sort()
      for (const n of old.slice(0, Math.max(0, old.length - TRACE_FILES))) rmSync(join(this.dir, n), { force: true })
    } catch {}
    this.head = this.tail = []
  }
}
// version is the package's, for a trace to say
function version() {
  try {
    return JSON.parse(readFileSync(new URL("./package.json", import.meta.url), "utf8")).version ?? ""
  } catch {
    return ""
  }
}

// Unlike the compatibility codec, malformed protobuf tails must fail a live
// continuation. Dropping one could turn a truncated batch into a valid handoff.
export function nativeFields(input, maxFields = Infinity) {
  const b = Buffer.from(input)
  const out = []
  let at = 0
  const uv = () => {
    let n = 0
    let scale = 1
    for (let i = 0; i < 10 && at < b.length; i++) {
      const c = b[at++]
      n += (c & 127) * scale
      if (c < 128 && Number.isSafeInteger(n)) return n
      scale *= 128
    }
    invalid("malformed native protobuf", 502)
  }
  while (at < b.length) {
    const key = uv()
    const num = Math.floor(key / 8)
    const wire = key % 8
    if (!num || num > 0x1fffffff) invalid("malformed native protobuf field", 502)
    let n = 0
    let data = Buffer.alloc(0)
    if (wire === 0) n = uv()
    else if (wire === 2 || wire === 1 || wire === 5) {
      const size = wire === 2 ? uv() : wire === 1 ? 8 : 4
      if (at + size > b.length) invalid("truncated native protobuf", 502)
      data = b.subarray(at, at + size)
      at += size
    } else invalid("unsupported native protobuf wire type", 502)
    if (out.length >= maxFields) invalid("native protobuf field limit reached", 502)
    out.push({ num, wire, n, data })
  }
  return out
}
// Decode every known Value kind, including overwritten oneof/map fields, so
// corrupt bytes cannot become different, valid JSON through prefix decoding.
// Missing scalar/map values keep protobuf defaults; unknown fields are skipped
// only after their complete wire envelope has passed validation.
export function nativeMcpArgs(input, limits = NATIVE_LIMITS) {
  if (input.length > limits.argumentBytes) invalid("native MCP argument byte limit reached", 502)
  let remaining = limits.argumentFields
  const read = (b) => {
    const fs = nativeFields(b, remaining)
    remaining -= fs.length
    return fs
  }
  const typed = (f, wire) => {
    if (f.wire !== wire) invalid("invalid native MCP argument field type", 502)
  }
  const utf8 = (b) => {
    try { return new TextDecoder("utf-8", { fatal: true, ignoreBOM: true }).decode(b) }
    catch { invalid("invalid UTF-8 in native MCP arguments", 502) }
  }
  function mapEntry(b, depth) {
    let key = "", value = null
    for (const f of read(b)) {
      if (f.num === 1) { typed(f, 2); key = utf8(f.data) }
      else if (f.num === 2) { typed(f, 2); value = decodeValue(f.data, depth) }
    }
    return [key, value]
  }
  function decodeStruct(b, depth) {
    const entries = []
    for (const f of read(b)) if (f.num === 1) {
      typed(f, 2)
      entries.push(mapEntry(f.data, depth))
    }
    return Object.fromEntries(entries)
  }
  function decodeValue(b, depth = 0) {
    if (depth > limits.argumentDepth) invalid("native MCP argument nesting limit reached", 502)
    let value = null
    for (const f of read(b)) {
      if (f.num < 1 || f.num > 6) continue
      typed(f, [0, 1, 2, 0, 2, 2][f.num - 1])
      switch (f.num) {
        case 1:
          if (f.n !== 0) invalid("invalid native protobuf NullValue", 502)
          value = null
          break
        case 2:
          value = f.data.readDoubleLE(0)
          if (!Number.isFinite(value)) invalid("nonfinite native MCP argument number", 502)
          break
        case 3: value = utf8(f.data); break
        case 4: value = f.n !== 0; break
        case 5: value = decodeStruct(f.data, depth + 1); break
        case 6:
          value = []
          for (const item of read(f.data)) if (item.num === 1) {
            typed(item, 2)
            value.push(decodeValue(item.data, depth + 1))
          }
          break
      }
    }
    return value
  }
  const fs = read(input), entries = []
  for (const f of fs) {
    if ([1, 3, 4, 5, 9].includes(f.num)) { typed(f, 2); utf8(f.data) }
    else if (f.num === 2) { typed(f, 2); entries.push(mapEntry(f.data, 0)) }
    else if (f.num === 6) typed(f, 2)
    else if (f.num === 7) typed(f, 0)
  }
  return { fields: fs, args: Object.fromEntries(entries) }
}

const field = (fs, n) => fs.find((f) => f.num === n)
const str = (fs, n) => field(fs, n)?.data.toString("utf8") ?? ""
const num = (fs, n) => field(fs, n)?.n ?? 0

// StepCalls is what Cursor says of a step's calls besides its execs
// (plugins#550). InteractionUpdate 2 (tool_call_started), 3
// (tool_call_completed) and 7 (partial_tool_call) each carry the call's
// call_id (field 1). A call Cursor completed by itself, never started on
// the client and never sent as an exec, is in tool_requests_listed's count
// all the same, and no exec of it will come: 0.2.34 waited for it while
// Cursor waited for the one call it had sent, 240 s, heartbeats only.
// parse is the path's field reader.
export class StepCalls {
  constructor(parse) {
    this.parse = parse
    this.started = new Set()
    this.execs = new Set() // the short strings of each exec's args: its tool_call_id among them
    this.partial = new Set() // calls the model is writing in this step
    this.byCursor = new Set()
    this.listedAfter = false // whether tool_requests_listed came after the latest call/update
  }
  // update takes an InteractionUpdate's own field (num) and its fields; it
  // says whether a call was completed by Cursor: one this step wrote (7)
  // and completed (3) with no start on the client (2) and no exec. A
  // completion of an earlier step's call is never counted again.
  update(n, uf) {
    if (n !== 2 && n !== 3 && n !== 7) return false
    this.listedAfter = false
    const id = str(uf, 1)
    if (!id) return false
    if (n === 2) this.started.add(id)
    if (n === 7) this.partial.add(id)
    if (n !== 3) return false
    if (!this.partial.has(id) || this.started.has(id) || this.execs.has(id) || this.byCursor.has(id)) return false
    this.byCursor.add(id)
    return true
  }
  // exec notes an exec's args, whatever its kind: each kind keeps its
  // tool_call_id at a field of its own (ShellArgs 4, ReadArgs 2, McpArgs 3).
  // If #3 came before its exec, it was not completed by Cursor after all.
  exec(data) {
    this.listedAfter = false
    try {
      for (const e of this.parse(data)) {
        if (e.wire !== 2 || e.num === 15 || e.num === 19) continue
        for (const a of this.parse(e.data)) if (a.wire === 2 && a.data.length && a.data.length <= 512) {
          const s = a.data.toString("utf8")
          this.execs.add(s)
          this.byCursor.delete(s)
        }
      }
    } catch {}
  }
  listed() {
    this.listedAfter = true
  }
  get completed() {
    return this.byCursor.size
  }
  // writing is how many calls the step wrote (7) that are neither started
  // on the client, sent as an exec, nor completed by Cursor: a call of
  // Cursor's own still running, or one still to be sent
  get writing() {
    let n = 0
    for (const id of this.partial) if (!this.started.has(id) && !this.execs.has(id) && !this.byCursor.has(id)) n++
    return n
  }
  // nextStep starts the next step's count
  nextStep() {
    this.partial.clear()
    this.byCursor.clear()
    this.listedAfter = false
  }
}
// pendingCalls is how many calls a checkpoint (ConversationStateStructure)
// says Cursor waits on: its pending_tool_calls, field 4; null when it
// can't be read
export function pendingCalls(data, parse = nativeFields) {
  try {
    return parse(data).filter((f) => f.num === 4).length
  } catch {
    return null
  }
}

function argsOf(s) {
  try {
    const v = typeof s === "string" ? JSON.parse(s) : s
    if (v && typeof v === "object" && !Array.isArray(v)) return v
  } catch {}
  invalid("invalid tool arguments in native continuation", 400)
}
// These conversions only decide whether a retained exec can be continued.
// Unsupported parts make that request use the legacy history builder instead
// of imposing a new Chat request contract on all callers.
function assistantDisplay(message) {
  const parts = typeof message.content === "string" ? [{ type: "text", text: message.content }] : message.content ?? []
  if (!Array.isArray(parts) || parts.some((p) => !["text", "reasoning"].includes(p?.type) || typeof p.text !== "string")) invalid("unsupported native assistant display")
  const reasoning = [message.reasoning_content, message.reasoning, message.reasoning_text].find((v) => typeof v === "string" && v.trim()) ?? ""
  return {
    text: parts.filter((p) => p.type === "text").map((p) => p.text).join(""),
    reasoning: reasoning + parts.filter((p) => p.type === "reasoning").map((p) => p.text).join(""),
  }
}
function resultParts(message, limits) {
  const parts = typeof message.content === "string" ? [{ type: "text", text: message.content }] : message.content
  if (!Array.isArray(parts)) invalid("unsupported native tool result")
  let retained = 0
  return parts.map((part) => {
    if (part?.type === "text" && typeof part.text === "string") {
      retained += Buffer.byteLength(part.text)
      if (retained > limits.outputBytes) invalid("native tool result limit reached")
      return { text: part.text }
    }
    const url = part?.type === "image_url" ? (typeof part.image_url === "string" ? part.image_url : part.image_url?.url)
      : part?.type === "image" && part.source?.type === "base64" ? `data:${part.source.media_type};base64,${part.source.data}` : ""
    const image = /^data:(image\/(?:png|jpeg|gif|webp));base64,([A-Za-z0-9+/]*={0,2})$/.exec(url ?? "")
    if (!image) invalid("unsupported native tool result content")
    const data = Buffer.from(image[2], "base64")
    retained += data.length
    if (retained > limits.outputBytes) invalid("native tool result limit reached")
    if (!data.length || data.toString("base64").replace(/=+$/, "") !== image[2].replace(/=+$/, "")) invalid("invalid native result image")
    return { data, mimeType: image[1] }
  })
}
function normalizeMessage(m) {
  if (m?.role !== "assistant") return m
  const { text, reasoning } = assistantDisplay(m)
  const out = { ...m, content: text }
  delete out.reasoning
  delete out.reasoning_content
  delete out.reasoning_text
  if (reasoning) out.reasoning_content = reasoning
  if (m.tool_calls) out.tool_calls = m.tool_calls.map((c) => ({
    ...c, type: c.type ?? "function", function: { ...c.function, arguments: argsOf(c.function?.arguments ?? "{}") },
  }))
  return out
}
const historyDigest = (messages) => hash(canonical(messages.map(normalizeMessage)))
function controlsOf(chat, selector, tools) {
  const controls = { ...chat }
  for (const k of ["messages", "stream", "stream_options"]) delete controls[k]
  return hash(canonical({ selector, controls, tools }))
}
const toolAttachment = (m) => m?.role === "user" && Array.isArray(m.content) && m.content.length > 1
  && m.content[0]?.type === "text" && m.content[0].text === "Attached image(s) from tool result:"
  && m.content.slice(1).every((p) => p?.type === "image_url")
// A complete result batch is validated before reserving or writing any exec.
// Only the current exported assistant may omit display reasoning. All earlier
// accepted history must match, including instruction order and attachments.
function matchResults(batch, messages) {
  if (historyDigest(messages.slice(0, batch.prefixLength)) !== batch.prefixDigest) invalid("native continuation history diverged")
  const suffix = messages.slice(batch.prefixLength)
  const assistants = []
  while (suffix[assistants.length]?.role === "assistant") assistants.push(suffix[assistants.length])
  if (!assistants.length) invalid("native continuation is missing its assistant tool calls", 400)
  let content = ""
  let reasoning = ""
  const calls = []
  for (const m of assistants) {
    const unknown = Object.keys(m).filter((k) => !["role", "content", "tool_calls", "reasoning", "reasoning_content", "reasoning_text"].includes(k))
    if (unknown.length) invalid("unsupported assistant fields in native continuation", 400)
    const display = assistantDisplay(m)
    content += display.text
    reasoning += display.reasoning
    for (const c of m.tool_calls ?? []) {
      if (c.type && c.type !== "function") invalid("unsupported native continuation tool call", 400)
      calls.push({ id: c.id, name: c.function?.name, args: argsOf(c.function?.arguments ?? "{}") })
    }
  }
  if (content !== batch.content || reasoning && reasoning !== batch.reasoning) invalid("native continuation assistant output changed")
  const expected = batch.calls.map((c) => ({ id: c.id, name: c.name, args: argsOf(c.args) }))
  const sorted = (v) => [...v].sort((a, b) => String(a.id).localeCompare(String(b.id)))
  if (canonical(sorted(calls)) !== canonical(sorted(expected))) invalid("native continuation tool calls changed")
  let results = suffix.slice(assistants.length)
  // Pi's exact image suffix has no call IDs. Only a single complete result
  // gives it unambiguous ownership; all other shapes rebuild legacy history.
  if (batch.calls.length === 1 && results.length === 2 && results[0]?.role === "tool" && toolAttachment(results[1])) {
    const [result, attachment] = results
    const content = typeof result.content === "string" ? [{ type: "text", text: result.content }] : result.content
    if (!Array.isArray(content)) invalid("unsupported native tool result")
    results = [{ ...result, content: [...content, ...attachment.content.slice(1)] }]
  }
  if (results.some((m) => m?.role !== "tool")) invalid("new input cannot be interleaved with pending native results")
  const ids = new Set()
  for (const m of results) {
    if (ids.has(m.tool_call_id)) invalid("duplicate native tool result", 400)
    ids.add(m.tool_call_id)
    if (!expected.some((c) => c.id === m.tool_call_id)) invalid("unknown native tool result", 400)
  }
  if (ids.size !== expected.length) invalid("all parallel native tool results must be submitted together", 400)
  return [...results].sort((a, b) => a.tool_call_id.localeCompare(b.tool_call_id))
}

class Segment {
  constructor(owner, chat) {
    this.owner = owner
    this.prefixLength = chat.messages.length
    this.prefixDigest = historyDigest(chat.messages)
    this.events = []
    this.calls = []
    // answered: the step's execs the bridge answered itself (a refusal, or
    // the MCP tool list). Cursor's tool_requests_listed counts them too.
    this.answered = 0
    this.content = ""
    this.reasoning = ""
    this.expected = null
    this.closed = false
    this.clean = false
    this.waiters = new Set()
  }
  wake() {
    for (const wake of this.waiters) wake()
    this.waiters.clear()
  }
  emit(e) {
    this.owner.retain(bytes(e))
    if (e.text) this.content += e.text
    if (e.reasoning) this.reasoning += e.reasoning
    this.events.push(e)
    this.wake()
  }
  finish(stop) {
    for (const [index, c] of this.calls.entries()) {
      const e = { tool: { index, id: c.id, name: c.name, args: c.args } }
      this.owner.retain(bytes(e))
      this.events.push(e)
    }
    this.owner.retain(bytes(stop))
    this.events.push(stop)
    this.closed = true
    this.wake()
  }
  fail(error) {
    // Errors also override a sealed, as yet undelivered handoff.
    this.events = [{ error }]
    this.closed = true
    this.wake()
  }
  iterator({ replay = false } = {}) {
    const segment = this
    let at = 0
    let errorSent = false
    return {
      async next() {
        while (!segment.closed && at >= segment.events.length) await new Promise((r) => segment.waiters.add(r))
        if (!replay && segment.owner.error) {
          if (errorSent) return { done: true }
          errorSent = true
          return { value: { error: segment.owner.error }, done: false }
        }
        if (at < segment.events.length) {
          const event = segment.events[at++]
          if (replay && event.stop) return { value: { ...event, usage: {
            prompt_tokens: 0, completion_tokens: 0, total_tokens: 0,
            prompt_tokens_details: { cached_tokens: 0, cache_write_tokens: 0 },
            magpie_native: { accounting: "replayed" },
          } }, done: false }
          return { value: event, done: false }
        }
        return { done: true }
      },
      async return() {
        if (!replay) await segment.owner.cancel("the Chat response was cancelled")
        return { done: true }
      },
      [Symbol.asyncIterator]() { return this },
    }
  }
}

// What a Run cut at its absolute deadline says. It most often lands
// mid-answer, the 200 long sent, so the agent reads only these words; the
// conversation is not lost (asked again, the history goes on a fresh Run),
// and they say so in words agents retry on: "timed out" (Pi, magpie's
// passingSaid) and "retry your request" (plugins#616, yetone/magpie#4419).
export const LIFETIME_EXPIRED = "native Run lifetime expired: the Run timed out; retry your request"

export class NativeSessionPool {
  constructor(limits = NATIVE_LIMITS) {
    this.limits = limits
    this.entries = new Set()
    this.handles = new Map()
    this.selectors = new Map()
  }
  // selector is conversationID(chat, session): session/cache key AND first
  // user identity. Task subagents sharing a session must not own its parent.
  prepare(tok, chat, selector, tools) {
    const fallback = (reason) => ({ fallback: reason })
    if (!Array.isArray(chat.messages) || bytes(chat) > this.limits.outputBytes) return fallback("history retention limit")
    const account = hash(tok)
    const controls = controlsOf(chat, selector, tools)
    const trailing = []
    const end = chat.messages.length - (toolAttachment(chat.messages.at(-1)) ? 2 : 1)
    for (let i = end; i >= 0 && chat.messages[i]?.role === "tool"; i--) trailing.push(chat.messages[i].tool_call_id)
    const handles = trailing.filter((id) => typeof id === "string" && HANDLE.test(id))
    if (handles.length) {
      // Look up ownership before inspecting native history. Restarted callers
      // only need the same permissive history shape the stateless path accepts.
      const found = handles.map((id) => this.handles.get(id))
      if (found.some((v) => !v)) return fallback("owner unavailable")
      const { owner, batch } = found[0]
      if (found.some((v) => v.owner !== owner || v.batch !== batch)) return fallback("different native batches")
      if (owner.account !== account || owner.controls !== controls || owner.error) return fallback("owner incompatible or expired")
      try {
        const results = matchResults(batch, chat.messages)
        // Encoding is pure for the entire batch before any reservation or write.
        const encoded = results.map((m) => owner.encodeResult(m))
        const digest = hash(canonical(results))
        if (batch.resultDigest) {
          // An uncertain or concurrent write is never submitted twice to this
          // exec. History reconstruction uses provided results as imported data.
          if (batch.resultDigest !== digest || !batch.next?.clean) return fallback("result already submitted or busy")
          return { owner, batch, replay: true, segment: batch.next }
        }
        if (owner.state !== "awaiting-results" || owner.segment !== batch || !batch.clean) return fallback("owner busy")
        // A retained Run's absolute timer spans all tool roundtrips. Rebuild
        // near its deadline before writing this batch: otherwise a new Chat
        // segment can lose its transport after already exposing output.
        const reserve = Math.min(owner.limits.continueReserve, owner.limits.lifetime * 2 / 3)
        if (owner.expiresAt - Date.now() <= reserve) {
          owner.fail({ status: 410, message: "native Run retired before lifetime deadline" }, "expired")
          return fallback("owner near lifetime deadline")
        }
        return { owner, batch, results, encoded, digest, chat }
      } catch (e) {
        if (!(e instanceof NativeSessionError)) throw e
        return fallback("history incompatible with retained owner")
      }
    }
    // Pending ancestors, new input, and terminal old handles are ordinary
    // imported history, even while a parent Run is still awaiting its results.
    // A fresh user turn whose old native calls all have results imports them
    // into a NEW owner; those results are never submitted to their old exec.
    const historical = chat.messages.flatMap((m) => (m?.tool_calls ?? []).filter((c) => HANDLE.test(c?.id ?? "")))
    if (historical.length) {
      const answered = new Set(chat.messages.filter((m) => m?.role === "tool").map((m) => m.tool_call_id))
      const last = chat.messages.at(-1)
      const freshTurn = last?.role === "user" && !toolAttachment(last)
      if (!freshTurn || historical.some((c) => !answered.has(c.id))) return fallback("historical native calls")
    }
    if (selector && this.selectors.has(account + ":" + selector)) {
      if (!historical.length) return fallback("owner busy")
      // New input after results the caller already gave its native calls
      // (Claude Code sends a reminder beside a tool result, which a chat
      // request carries as a user message after the tool message). The
      // reservation's holder, a parent awaiting results, is left as it is:
      // this Run takes no reservation, and only its own calls resume it.
      return { account, selector, controls, selected: false, chat }
    }
    return { account, selector, controls, selected: !!selector, chat }
  }
  create(prepared, resolution, wire) {
    if (prepared.fallback || prepared.selected && this.selectors.has(prepared.account + ":" + prepared.selector)) return null
    const crowded = () => this.entries.size >= this.limits.perProcess || [...this.entries].filter((e) => e.account === prepared.account).length >= this.limits.perAccount
    while (crowded()) {
      const accountFull = [...this.entries].filter((e) => e.account === prepared.account).length >= this.limits.perAccount
      const finished = [...this.entries].find((e) => ["complete", "failed", "cancelled", "expired"].includes(e.state) && (!accountFull || e.account === prepared.account))
      if (!finished) return null
      this.forget(finished)
    }
    let owner
    try { owner = new NativeSession(this, prepared, resolution, wire) }
    catch (e) {
      if (!(e instanceof NativeSessionError)) throw e
      return null
    }
    // A Run of this conversation still live may be abandoned after this one
    // starts: what it then owes is bounded before this start, so this Run's
    // own dashboard events are never collected as its debt.
    // One that waits on the caller's results owes its step now, for this
    // Run to collect, rather than when it is abandoned at resultWait.
    const at = Date.now()
    for (const e of this.entries) {
      if (e.account !== owner.account || !e.selector || e.selector !== owner.selector) continue
      e.laterRunAt ??= at
      if (e.state === "awaiting-results") e.accounting?.handover()
    }
    this.entries.add(owner)
    if (prepared.selected) this.selectors.set(owner.account + ":" + owner.selector, owner)
    return owner
  }
  release(owner) {
    const key = owner.account + ":" + owner.selector
    if (this.selectors.get(key) === owner) this.selectors.delete(key)
  }
  forget(owner) {
    owner.accounting?.abandon()
    clearTimeout(owner.replayTimer)
    this.release(owner)
    this.entries.delete(owner)
    for (const [id, h] of this.handles) if (h.owner === owner) this.handles.delete(id)
    owner.blobs.clear()
    owner.checkpoint = null
    owner.execs.clear()
    owner.nativeCalls.clear()
    owner.stepUsages.clear()
    owner.lastStepUsage = null
  }
  async clear() {
    await Promise.all([...this.entries].map((e) => e.cancel("native owner removed")))
    for (const e of this.entries) {
      clearTimeout(e.replayTimer)
      this.forget(e)
    }
  }
}

class NativeSession {
  constructor(pool, prepared, resolution, wire) {
    Object.assign(this, prepared)
    this.pool = pool
    this.limits = pool.limits
    this.resolution = resolution
    this.wire = wire
    this.identity = randomBytes(12).toString("hex")
    this.state = "opening"
    this.ac = new AbortController()
    this.execs = new Map()
    this.nativeCalls = new Map()
    // what Cursor said of the step's calls besides its execs (plugins#550)
    this.stepCalls = new StepCalls(nativeFields)
    this.blobs = new Map()
    this.knownHashes = new Set()
    this.blobSize = 0
    this.retainedBytes = 0
    this.sequence = 0
    this.segments = 0
    // the model calls of the turn so far, and what was said before the
    // latest one began: the last step's own share of the turn (#156)
    this.steps = 1
    this.stepSaid = 0
    this.stepUsages = new Map()
    this.lastStepUsage = null
    this.said = 0
    this.wrote = 0
    this.terminal = false
    this.segment = new Segment(this, prepared.chat)
    this.retain(bytes(prepared.chat))
    this.expireIn(this.limits.lifetime)
    this.quiet = new Quiet(this.limits, (e) => this.fail(e), { path: "native", model: resolution?.id })
    this.quiet.nextStep(this.steps)
  }
  // expireIn sets the Run's absolute deadline ms from now: from its start,
  // the whole lifetime; a test brings it in after the first answer
  expireIn(ms) {
    clearTimeout(this.lifetimeTimer)
    this.expiresAt = Date.now() + ms
    this.lifetimeTimer = later(() => this.fail({ status: 410, message: LIFETIME_EXPIRED }, "expired"), ms)
  }
  retain(n) {
    if (this.retainedBytes + n > this.limits.outputBytes) invalid("native output/history retention limit reached", 503)
    this.retainedBytes += n
  }
  attach(signal, segment = this.segment) {
    if (signal?.aborted) {
      this.fail({ status: 410, message: "the Chat request was already cancelled" }, "cancelled")
      return
    }
    const abort = () => this.fail({ status: 410, message: "the Chat request was cancelled" }, "cancelled")
    signal?.addEventListener("abort", abort, { once: true })
    segment.detach = () => signal?.removeEventListener("abort", abort)
  }
  delivered(segment = this.segment) {
    if (this.error) return
    segment.clean = true
    segment.detach?.()
    if (this.state === "complete" && segment === this.segment) this.accounting?.delivered()
  }
  closeTransport() {
    if (this.transportClosed) return
    this.transportClosed = true
    clearInterval(this.runHeartbeat)
    clearInterval(this.execHeartbeat)
    clearTimeout(this.batchTimer)
    clearTimeout(this.resultTimer)
    clearTimeout(this.drainTimer)
    clearTimeout(this.lifetimeTimer)
    this.quiet.pause()
    this.ac.abort()
    this.transport?.close()
  }
  fail(error, state = "failed") {
    if (this.error || this.state === "complete") return
    this.error = error
    this.state = state
    // a cancelled Run is the caller's doing, not a failure to trace
    if (state !== "cancelled") this.quiet.failed(error)
    this.segment.fail(error)
    for (const segment of this.batches ?? []) segment.detach?.()
    this.segment.detach?.()
    this.closeTransport()
    // A failed transport has no reusable execs. Forget its handles immediately;
    // subsequent caller-supplied history takes the stateless path.
    this.pool.forget(this)
  }
  async cancel(message) {
    this.fail({ status: 410, message }, "cancelled")
    await this.pumpTask
  }
  bind(transport, blobs, estimate) {
    if (this.transportClosed || this.error) {
      transport.close()
      return
    }
    this.transport = transport
    this.estimate = estimate
    this.stepEstimate = estimate
    this.blobs = blobs
    this.knownHashes = new Set(blobs.keys())
    this.blobSize = [...blobs.values()].reduce((n, b) => n + b.length, 0)
    if (this.blobSize > this.limits.blobBytes) invalid("native bootstrap blob limit reached", 503)
    if (this.error) return this.closeTransport()
    this.state = "running"
    const { pb } = this.wire
    const heartbeat = (message) => {
      try { this.send(message) } catch (e) { this.fail(failureOf(e)) }
    }
    this.runHeartbeat = setInterval(() => heartbeat(pb().bytes(7, Buffer.alloc(0)).done()), 5000)
    this.execHeartbeat = setInterval(() => {
      for (const c of this.execs.values()) if (!c.submitted) heartbeat(pb().bytes(5, pb().bytes(3, pb().varint(1, c.requestID))).done())
    }, 3000)
    this.runHeartbeat.unref?.()
    this.execHeartbeat.unref?.()
  }
  send(message) {
    if (this.error || this.transportClosed) invalid("native transport is no longer writable", 410)
    this.quiet.trace?.frame("out", message)
    this.transport.send(message)
  }
  // reply answers one of Cursor's KV requests once the outbound queue has
  // room. Cursor asks for every history blob at once, hundreds for a long
  // conversation, faster than a slow link takes the replies: the decoder
  // waits for the queue rather than failing the Run (#142).
  async reply(message) {
    await this.transport.room?.(message.length + 5)
    this.send(message)
  }
  execClose(id) {
    this.send(this.wire.pb().bytes(5, this.wire.pb().bytes(1, this.wire.pb().varint(1, id))).done())
  }
  encodeResult(message) {
    const { pb } = this.wire
    const success = pb()
    for (const item of resultParts(message, this.limits)) {
      if (item.text !== undefined) success.bytes(1, pb().bytes(1, pb().str(1, item.text)))
      else success.bytes(1, pb().bytes(2, pb().bytes(1, item.data).str(2, item.mimeType)))
    }
    if (message.is_error === true || message.isError === true) success.varint(2, 1)
    return pb().bytes(1, success).done() // McpResult.success, not stored McpToolResult
  }
  resume(prepared, signal) {
    if (prepared.replay) return this.response(prepared.segment, true)
    const batch = prepared.batch
    if (this.error || this.state !== "awaiting-results" || this.segment !== batch || batch.resultDigest) invalid("native continuation is already reserved")
    const next = new Segment(this, prepared.chat)
    this.attach(signal, next)
    if (this.error) invalid(this.error.message, this.error.status)
    // Reserve synchronously. Competing continuations cannot interleave writes.
    batch.resultDigest = prepared.digest
    batch.next = next
    this.segment = next
    this.state = "submitting"
    // a step owed at a later Run's start and not collected yet is this Run's
    // to count again, in its own final
    this.accounting?.resumed()
    clearTimeout(this.resultTimer)
    clearTimeout(this.batchTimer)
    this.batchTimer = null
    this.nextStep(prepared.estimate)
    try {
      for (let i = 0; i < prepared.results.length; i++) {
        const call = batch.calls.find((c) => c.id === prepared.results[i].tool_call_id)
        const msg = this.wire.pb().varint(1, call.requestID)
        if (call.execID) msg.str(15, call.execID)
        msg.bytes(11, prepared.encoded[i])
        this.resultSubmitted = true
        this.send(this.wire.pb().bytes(2, msg).done())
        call.submitted = true
        this.execClose(call.requestID)
      }
      this.state = "running"
      this.quiet.resume()
    } catch (e) {
      const error = failureOf(e)
      this.fail({ status: error.status, message: `${error.message}; native result submission failed; its outcome is uncertain and cannot be replayed` })
    }
    return this.response(next)
  }
  response(segment = this.segment, replay = false) {
    return {
      events: segment.iterator({ replay }),
      cancel: () => replay ? Promise.resolve() : this.cancel("the Chat response was cancelled"),
      resolution: this.resolution,
      delivered: () => replay || this.delivered(segment),
      diagnostic: `same-run; bootstrap=imported-history; new-turn=stateless; missing-owner=stateless${replay ? "; replay=true" : ""}`,
    }
  }
  acceptCall(call) {
    if (!Number.isInteger(call.requestID)) invalid("native tool call is missing its exec request ID", 502)
    const prior = this.execs.get(call.requestID)
    const digest = hash(canonical({ requestID: call.requestID, execID: call.execID, nativeID: call.nativeID, name: call.name, args: argsOf(call.args) }))
    if (prior) {
      if (prior.digest !== digest) invalid("native exec identity changed during retransmission", 502)
      return
    }
    // A retransmission of the final exec can arrive after it sealed its batch.
    // Validate that identity first; only a new exec violates the sealed phase.
    if (this.terminal || this.state === "awaiting-results") invalid("native call arrived after its batch was sealed", 502)
    if (call.nativeID && this.nativeCalls.has(call.nativeID)) invalid("native tool-call ID was reused for another exec", 502)
    call.id = `call_cursor_${this.identity}_${this.segments}_${this.segment.calls.length}`
    call.digest = digest
    this.execs.set(call.requestID, call)
    if (call.nativeID) this.nativeCalls.set(call.nativeID, call)
    this.segment.calls.push(call)
    this.retain(bytes(call))
    this.said += call.args.length
    this.waitForBatch()
    this.sealBatch()
  }
  // nextStep is Cursor's next model call in the same turn: after the
  // caller's results, or after a step whose calls were all answered here.
  nextStep(estimate) {
    this.steps++
    this.stepCalls.nextStep()
    this.quiet.nextStep(this.steps)
    this.stepSaid = this.said
    this.lastStepUsage = null
    if (estimate != null) this.stepEstimate = estimate
  }
  modelCallCompleted(fields) {
    const id = str(fields, 1)
    const entry = {
      usage: this.wire.usageOf(fields, 1),
      measuredInput: fields.some((f) => [2, 4, 5].includes(f.num)),
      measuredOutput: fields.some((f) => f.num === 3),
      hasReasoning: fields.some((f) => f.num === 6),
    }
    this.stepUsages.set(id || `step:${this.steps}:${this.stepUsages.size}`, entry)
    this.lastStepUsage = entry
  }
  // waitForBatch gives the step batchWait from its latest call, answered
  // call, count, text or heartbeat, not from its first. Cursor sends each
  // call once the model has written its arguments, so a step of many long
  // calls arrives over longer than batchWait while never pausing that long
  // (#155), and one long call is written with only heartbeats meanwhile
  // (#377); a heartbeat-only stretch is bounded by quiet. The failure says
  // how much of the batch came, so a stall reads apart from a slow call.
  waitForBatch() {
    clearTimeout(this.batchTimer)
    const since = Date.now()
    this.batchTimer = later(() => {
      const s = this.segment
      if (s.expected == null && s.calls.length && !this.resultSubmitted) this.missingCount()
      else this.fail({ status: 502, message: `Cursor did not complete its declared native tool batch: ${s.calls.length} of ${s.expected ?? "?"} calls came, ${s.answered} answered here${this.stepCalls.completed ? `, ${this.stepCalls.completed} completed by Cursor` : ""}, none since ${Date.now() - since} ms` })
    }, this.limits.batchWait)
  }
  missingCount() {
    this.fail({ status: 502, message: "Cursor did not declare its native tool batch", retentionIneligible: !this.resultSubmitted })
  }
  // A call of the model's step the bridge answered itself: a refused tool (a
  // name it doesn't serve, Cursor's own tools) or get_mcp_tools. Cursor's
  // count is of every call the step made, so it counts toward the batch;
  // never handed to the caller (#142).
  answeredCall() {
    if (this.terminal || this.state === "awaiting-results") return
    this.segment.answered++
    if (this.batchTimer) this.waitForBatch()
    this.sealBatch()
  }
  // A call of the step Cursor completed by itself (plugins#550): in its
  // count, and never sent as an exec.
  completedByCursor() {
    if (this.terminal || this.state === "awaiting-results") return
    if (this.batchTimer) this.waitForBatch()
    this.sealBatch()
  }
  // A checkpoint whose state waits on just the calls handed to the caller,
  // after the step's count came at the end of the step's calls, is a step
  // Cursor has finished: what more the count says will not come (plugins#550).
  // When the count was sent before or between the step's calls, a mid-batch
  // checkpoint after an early call has only the calls sent so far in
  // pending_tool_calls and must not seal the batch early before the
  // remaining calls arrive.
  waitedOn(data) {
    const s = this.segment
    if (this.terminal || this.state === "awaiting-results" || s.closed || s.expected == null || !s.calls.length || !this.stepCalls.listedAfter) return
    if (pendingCalls(data) !== s.calls.length) return
    s.waitedOn = true
    this.sealBatch()
  }
  listed(count) {
    if (!Number.isSafeInteger(count) || count < 1) invalid("invalid native tool count", 502)
    if (this.segment.expected != null && this.segment.expected !== count) {
      // Conflicting counts leave the native step boundary ambiguous. Retire
      // this owner; Chat may rebuild from history only if no native result
      // was submitted to this Run.
      this.fail({ status: 502, message: "inconsistent native tool count", retentionIneligible: !this.resultSubmitted, continuityReason: "inconsistent-batch-count" })
      return
    }
    this.segment.expected = count
    this.stepCalls.listed()
    if (this.state === "awaiting-results") return // an identical retransmission
    this.waitForBatch()
    this.sealBatch()
  }
  sealBatch() {
    const segment = this.segment
    if (segment.expected == null) return
    const made = segment.calls.length + segment.answered
    if (made > segment.expected) invalid("native tool batch exceeded its declared count", 502)
    // the calls Cursor completed by itself make up the rest (plugins#550)
    if ((made + this.stepCalls.completed < segment.expected && !segment.waitedOn) || segment.closed) return
    if (!segment.calls.length) {
      // Every call of the step was answered here: Cursor goes on with the
      // turn by itself, and its next step lists a count of its own.
      segment.expected = null
      segment.answered = 0
      clearTimeout(this.batchTimer)
      this.batchTimer = null
      this.nextStep()
      return
    }
    clearTimeout(this.batchTimer)
    this.batchTimer = null
    this.state = "awaiting-results"
    // Cursor waits on the caller now: resultWait bounds the wait
    this.quiet.pause()
    this.batches ??= []
    this.batches.push(segment)
    for (const c of segment.calls) this.pool.handles.set(c.id, { owner: this, batch: segment })
    segment.finish({ stop: "tool_calls", usage: {
      prompt_tokens: 0, completion_tokens: 0, total_tokens: 0,
      prompt_tokens_details: { cached_tokens: 0, cache_write_tokens: 0 },
      magpie_native: { accounting: "pending" },
    } })
    this.segments++
    this.resultTimer = later(() => this.fail({ status: 410, message: "native tool-result deadline expired" }, "expired"), this.limits.resultWait)
  }
  store(id, data) {
    if (!id.length) invalid("native blob ID is empty", 502)
    const key = id.toString("hex")
    if (this.knownHashes.has(key) && hash(data) !== key) invalid("native content-addressed bootstrap blob changed", 502)
    const size = this.blobSize - (this.blobs.get(key)?.length ?? 0) + data.length
    if (size + (this.checkpoint?.bytes.length ?? 0) > this.limits.blobBytes) invalid("native blob retention limit reached", 503)
    // Incoming IDs are opaque: a fixed-root ID need not be SHA-256(data).
    this.blobs.set(key, Buffer.from(data))
    this.blobSize = size
    this.validateCheckpoint()
  }
  checkpointReceived(data) {
    if (data.length + this.blobSize > this.limits.blobBytes) invalid("native checkpoint retention limit reached", 503)
    nativeFields(data)
    this.checkpoint = { bytes: Buffer.from(data), sequence: ++this.sequence, status: "incomplete", terminal: this.terminal }
    this.validateCheckpoint()
  }
  validateCheckpoint() {
    if (!this.checkpoint) return
    const state = nativeFields(this.checkpoint.bytes)
    const missing = new Set()
    let supported = true
    const get = (id) => {
      const key = id.toString("hex")
      const b = this.blobs.get(key)
      if (!b) missing.add(key)
      return b
    }
    const refs = new Set([1, 3, 6, 7, 11, 13])
    const known = new Set(Array.from({ length: 34 }, (_, i) => i + 1).filter((n) => n !== 2))
    // These shapes contain additional state/reference contracts which are not
    // implemented by the caller-MCP bridge. Preserve their bytes without
    // claiming reference closure or restart eligibility.
    const complex = new Set([3, 6, 7, 11, 12, 13, 15, 16, 20, 24, 29, 30, 31, 32])
    for (const f of state) {
      if (!known.has(f.num) || complex.has(f.num)) supported = false
      if (refs.has(f.num) && f.data.length) get(f.data)
      if (f.num !== 8) continue
      const turn = get(f.data)
      if (!turn) continue
      const tf = nativeFields(turn)
      const agent = field(tf, 1)
      if (!agent || tf.some((f) => f.num !== 1)) { supported = false; continue }
      const agentFields = nativeFields(agent.data)
      if (!field(agentFields, 1)?.data.length) supported = false
      for (const a of agentFields) {
        if (a.num === 1) {
          const user = get(a.data)
          if (user) nativeFields(user)
        } else if (a.num === 2) {
          const step = get(a.data)
          if (step) {
            const sf = nativeFields(step)
            if (sf.length !== 1 || ![1, 2, 3].includes(sf[0].num)) supported = false
            else {
              const stepFields = nativeFields(sf[0].data)
              if (sf[0].num === 2 && (stepFields.length !== 1 || stepFields[0].num !== 15)) supported = false
            }
          }
        } else if (![3, 4, 5, 6].includes(a.num)) supported = false
      }
    }
    this.checkpoint.missing = missing.size
    this.checkpoint.status = !supported ? "unsupported" : missing.size ? "incomplete" : "complete"
    this.checkpoint.terminal = this.terminal
  }
  terminalSeen(fields) {
    if (this.segment.calls.length) invalid("Cursor ended a turn with unresolved native tool calls", 502)
    this.terminal = true
    this.state = "finalizing"
    this.usageFields = fields
    if (this.checkpoint) this.checkpoint.terminal = true
    clearTimeout(this.batchTimer)
    this.drainTimer = later(() => {
      Promise.resolve().then(() => this.complete()).catch((e) => this.fail(failureOf(e)))
    }, this.limits.drain)
  }
  complete() {
    if (this.completionTask) return this.completionTask
    if (this.error || this.state === "complete") return
    if (!this.terminal) invalid("native Run ended before TurnEnded or a complete final answer", 502)
    const { usageOf } = this.wire
    const turnInput = this.usageFields?.some((f) => [1, 3, 4].includes(f.num))
    const turnOutput = this.usageFields?.some((f) => f.num === 2)
    const last = this.lastStepUsage
    const measuredInput = turnInput || !!last?.measuredInput
    const measuredOutput = turnOutput || !!last?.measuredOutput
    const u = usageOf(this.usageFields ?? [])
    if (!turnInput && last?.measuredInput) Object.assign(u, { input: last.usage.input, cacheRead: last.usage.cacheRead, cacheWrite: last.usage.cacheWrite })
    if (!turnOutput && last?.measuredOutput) Object.assign(u, { output: last.usage.output, reasoning: last.usage.reasoning })
    if (!measuredInput) u.input = this.estimate
    if (!measuredOutput) u.output = Math.floor((this.said + 3) / 4)
    // A turn of several model calls: TurnEndedUpdate counts them all, but a
    // client reads the last response's prompt as how full its context is
    // (Codex compacted a 61K context at 1.1M, #156). When Cursor emitted
    // ModelCallCompletedUpdate (31) for the last step, it keeps its exact
    // count; otherwise it keeps the larger of its prompt estimate and the
    // turn's mean per call. The rest is billed as magpie_deferred.
    let earlier = null
    if (this.steps > 1) {
      if (last && (last.measuredInput || last.measuredOutput)) {
        const prev = [...this.stepUsages.values()].filter((e) => e !== last)
        const sum = (k) => prev.reduce((s, e) => s + e.usage[k], 0)
        const whole = u.input + u.cacheRead + u.cacheWrite
        const share = (total, own) => total > 0 ? Math.min(1, Math.max(own, total / this.steps) / total) : 0
        const fi = share(whole, turnInput ? this.stepEstimate ?? 0 : whole)
        const fo = share(u.output, Math.floor((this.said - this.stepSaid + 3) / 4))
        const own = {
          input: last.measuredInput ? last.usage.input : Math.round(u.input * fi),
          cacheRead: last.measuredInput ? last.usage.cacheRead : Math.round(u.cacheRead * fi),
          cacheWrite: last.measuredInput ? last.usage.cacheWrite : Math.round(u.cacheWrite * fi),
          output: last.measuredOutput ? last.usage.output : Math.round(u.output * fo),
          reasoning: last.hasReasoning ? last.usage.reasoning : last.measuredOutput ? Math.min(u.reasoning, last.usage.output) : Math.round(u.reasoning * fo),
        }
        earlier = {
          input: turnInput ? Math.max(0, u.input - own.input) : sum("input"),
          cacheRead: turnInput ? Math.max(0, u.cacheRead - own.cacheRead) : sum("cacheRead"),
          cacheWrite: turnInput ? Math.max(0, u.cacheWrite - own.cacheWrite) : sum("cacheWrite"),
          output: turnOutput ? Math.max(0, u.output - own.output) : sum("output"),
        }
        Object.assign(u, own)
      } else {
        const whole = u.input + u.cacheRead + u.cacheWrite
        const share = (total, own) => total > 0 ? Math.min(1, Math.max(own, total / this.steps) / total) : 0
        const fi = share(whole, measuredInput ? this.stepEstimate ?? 0 : whole)
        const fo = share(u.output, Math.floor((this.said - this.stepSaid + 3) / 4))
        const own = {
          input: Math.round(u.input * fi), cacheRead: Math.round(u.cacheRead * fi), cacheWrite: Math.round(u.cacheWrite * fi),
          output: Math.round(u.output * fo), reasoning: Math.round(u.reasoning * fo),
        }
        earlier = { input: u.input - own.input, cacheRead: u.cacheRead - own.cacheRead, cacheWrite: u.cacheWrite - own.cacheWrite, output: u.output - own.output }
        Object.assign(u, own)
      }
    }
    // What a later Run collected of this Run's event while this one waited on
    // results (handover) is in the turn's totals too: it comes off them, from
    // the earlier steps' share first, so the event counts once.
    const counted = this.accounting?.counted?.()
    if (counted) for (const k of ["input", "cacheRead", "cacheWrite", "output"]) {
      const off = Math.min(earlier?.[k] ?? 0, counted[k] ?? 0)
      if (earlier) earlier[k] -= off
      u[k] = Math.max(0, u[k] - ((counted[k] ?? 0) - off))
    }
    const prompt = u.input + u.cacheRead + u.cacheWrite
    this.finalUsage = {
      prompt_tokens: prompt, completion_tokens: u.output, total_tokens: prompt + u.output,
      prompt_tokens_details: { cached_tokens: u.cacheRead, cache_write_tokens: u.cacheWrite },
      ...(this.usageFields?.some((f) => f.num === 5) || last?.hasReasoning ? { completion_tokens_details: { reasoning_tokens: u.reasoning } } : {}),
      ...(!measuredInput || !measuredOutput ? { magpie_native: { accounting: "estimated" } } : {}),
    }
    this.state = "finalizing"
    this.closeTransport()
    const finish = (settled) => {
      if (this.error) return
      const more = earlier && settled ? Object.fromEntries(Object.keys(earlier).map((k) => [k, earlier[k] + settled[k]])) : earlier ?? settled
      const deferred = more && more.input + more.output + more.cacheRead + more.cacheWrite > 0
      this.segment.finish({ stop: "stop", usage: {
        ...this.finalUsage,
        ...(deferred ? { magpie_deferred: {
          prompt_tokens: more.input + more.cacheRead + more.cacheWrite,
          completion_tokens: more.output,
          cached_tokens: more.cacheRead,
          cache_write_tokens: more.cacheWrite,
        } } : {}),
      } })
      this.state = "complete"
      this.pool.release(this)
      this.replayTimer = later(() => this.pool.forget(this), this.limits.replayWait)
    }
    if (!this.accounting) return finish(null)
    this.completionTask = this.accounting.settle().then(finish)
    return this.completionTask
  }
  async pump(stream) {
    const { pb, exec, failure } = this.wire
    try {
      for await (const f of this.quiet.frames(stream)) {
        if (this.error || this.state === "complete") return
        if (f.end) {
          const e = failure(200, f.data)
          if (e.status < 200 || e.status > 299) throw new NativeSessionError(e.status, e.message)
          if (this.segment.calls.length && this.segment.expected == null) { this.missingCount(); return }
          if (!this.terminal && !this.segment.calls.length && this.segment.content) this.terminalSeen([])
          await this.complete()
          return
        }
        for (const m of nativeFields(f.data)) {
          switch (m.num) {
            case 1:
              for (const u of nativeFields(m.data)) {
                const uf = nativeFields(u.data)
                if (u.num === 1 || u.num === 4) {
                  const text = str(uf, 1)
                  if (this.terminal || this.state === "awaiting-results") invalid("native output arrived outside a running Chat segment", 502)
                  if (text) {
                    this.said += text.length
                    if (u.num === 1) this.wrote += text.length
                    this.segment.emit(u.num === 1 ? { text } : { reasoning: text })
                    if (this.batchTimer) this.waitForBatch()
                  }
                } else if (u.num === 14) {
                  if (!this.segment.content && !this.segment.calls.length) invalid("an empty reply (Cursor ended the turn without text or a tool call)", 502)
                  this.terminalSeen(uf)
                } else if (u.num === 31) {
                  this.modelCallCompleted(uf)
                } else if (u.num === 27) {
                  this.listed(num(uf, 1))
                  if (this.error) return
                } else if (u.num === 13) {
                  if (this.batchTimer && !this.terminal && this.state !== "awaiting-results") this.waitForBatch()
                } else if ((u.num === 2 || u.num === 3 || u.num === 7) && this.stepCalls.update(u.num, uf)) {
                  this.completedByCursor()
                  if (this.error) return
                }
              }
              break
            case 2: {
              this.stepCalls.exec(m.data)
              const c = exec(m.data, this.chatTools, (b) => this.send(b), (id) => this.execClose(id), null, true)
              if (c) this.acceptCall(c)
              // request_context is Cursor asking for the environment, not a call
              else if (!nativeFields(m.data).some((e) => e.num === 10)) this.answeredCall()
              break
            }
            case 3:
              this.checkpointReceived(m.data)
              this.waitedOn(m.data)
              break
            case 4: {
              const kv = nativeFields(m.data)
              const id = num(kv, 1)
              for (const k of kv) {
                if (k.num === 2) {
                  const want = field(nativeFields(k.data), 1)?.data.toString("hex") ?? ""
                  const b = this.blobs.get(want)
                  const res = b ? pb().bytes(1, b) : pb().bytes(2, pb().str(1, "blob not found"))
                  await this.reply(pb().bytes(3, pb().varint(1, id).bytes(2, res)).done())
                } else if (k.num === 3) {
                  try {
                    const set = nativeFields(k.data)
                    this.store(field(set, 1)?.data ?? Buffer.alloc(0), field(set, 2)?.data ?? Buffer.alloc(0))
                  } catch (e) {
                    this.send(pb().bytes(3, pb().varint(1, id).bytes(3, pb().bytes(1, pb().str(1, "native blob could not be retained")))).done())
                    throw e
                  }
                  await this.reply(pb().bytes(3, pb().varint(1, id).bytes(3, Buffer.alloc(0))).done())
                }
              }
              break
            }
            case 5:
              for (const c of nativeFields(m.data)) if (c.num === 1) {
                const id = num(nativeFields(c.data), 1)
                const pending = this.execs.get(id)
                if (pending && !pending.submitted) invalid("Cursor cancelled a pending native tool exec; late results cannot be submitted", 410)
              }
              break
            case 7:
              invalid("unsupported native interaction query", 502)
          }
        }
      }
      if (!this.error && this.state !== "complete") {
        if (this.terminal) await this.complete()
        else invalid("native reply broke off before completing its turn or tool batch: EOF", 502)
      }
    } catch (e) {
      if (!this.error && this.state !== "complete") this.fail(failureOf(e))
    }
  }
}

export const nativeSessions = new NativeSessionPool()
