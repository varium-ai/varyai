//! Bounded, content-free observations of response token-format failures.

use std::fmt;

use super::token_format;

macro_rules! fixed_enum {
    ($name:ident { $first:ident => $label:literal $(, $rest:ident => $other:literal)* $(,)? }) => {
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
        pub(super) enum $name { #[default] $first, $($rest),* }

        impl $name {
            fn label(self) -> &'static str {
                match self { Self::$first => $label, $(Self::$rest => $other),* }
            }
        }
    };
}

fixed_enum!(Carrier {
    Other => "other", OutputText => "output_text", Refusal => "refusal",
    Reasoning => "reasoning", FunctionArguments => "function_arguments", CustomInput => "custom_input",
});
fixed_enum!(Site {
    Unknown => "unknown", DeltaSplit => "delta_split", DeltaRestore => "delta_restore",
    DoneRestore => "done_restore", SnapshotRestore => "snapshot_restore",
    FinalTextRestore => "final_text_restore", FinalToolRestore => "final_tool_restore",
});
fixed_enum!(LengthBucket {
    Zero => "zero", B01_07 => "b01_07", B08_15 => "b08_15", B16_30 => "b16_30",
    B31_63 => "b31_63", B64_255 => "b64_255", B256_1023 => "b256_1023", B1024Plus => "b1024_plus",
});
fixed_enum!(Shape { None => "none", Digest => "digest", Suffix => "suffix", Incomplete => "incomplete" });
fixed_enum!(InvalidChar {
    None => "none", Markdown => "markdown", Whitespace => "whitespace",
    Ascii => "ascii", NonAscii => "non_ascii", End => "end",
});
fixed_enum!(Event {
    Other => "other", OutputTextDelta => "output_text_delta", OutputTextDone => "output_text_done",
    RefusalDelta => "refusal_delta", RefusalDone => "refusal_done",
    ReasoningDelta => "reasoning_delta", ReasoningDone => "reasoning_done",
    FunctionDelta => "function_delta", FunctionDone => "function_done",
    CustomDelta => "custom_delta", CustomDone => "custom_done",
    ItemAdded => "item_added", ItemDone => "item_done", PartAdded => "part_added", PartDone => "part_done",
    SummaryPartAdded => "summary_part_added", SummaryPartDone => "summary_part_done",
    Completed => "completed", Failed => "failed", Incomplete => "incomplete", Error => "error",
});

impl LengthBucket {
    fn from_len(length: usize) -> Self {
        match length {
            0 => Self::Zero,
            1..=7 => Self::B01_07,
            8..=15 => Self::B08_15,
            16..=30 => Self::B16_30,
            31..=63 => Self::B31_63,
            64..=255 => Self::B64_255,
            256..=1023 => Self::B256_1023,
            _ => Self::B1024Plus,
        }
    }
}

impl Event {
    fn from_name(name: &str) -> Self {
        match name {
            "response.output_text.delta" => Self::OutputTextDelta,
            "response.output_text.done" => Self::OutputTextDone,
            "response.refusal.delta" => Self::RefusalDelta,
            "response.refusal.done" => Self::RefusalDone,
            "response.reasoning_summary_text.delta" => Self::ReasoningDelta,
            "response.reasoning_summary_text.done" => Self::ReasoningDone,
            "response.function_call_arguments.delta" => Self::FunctionDelta,
            "response.function_call_arguments.done" => Self::FunctionDone,
            "response.custom_tool_call_input.delta" => Self::CustomDelta,
            "response.custom_tool_call_input.done" => Self::CustomDone,
            "response.output_item.added" => Self::ItemAdded,
            "response.output_item.done" => Self::ItemDone,
            "response.content_part.added" => Self::PartAdded,
            "response.content_part.done" => Self::PartDone,
            "response.reasoning_summary_part.added" => Self::SummaryPartAdded,
            "response.reasoning_summary_part.done" => Self::SummaryPartDone,
            "response.completed" => Self::Completed,
            "response.failed" => Self::Failed,
            "response.incomplete" => Self::Incomplete,
            "error" => Self::Error,
            _ => Self::Other,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct EventSummary {
    seq: u8,
    event: Event,
    // The barrier state at event admission, before a tool event can activate it.
    barrier: bool,
    frame: LengthBucket,
    structure_known: bool,
    before: LengthBucket,
    after: LengthBucket,
    text: LengthBucket,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Summary {
    observed: bool,
    seq: u8,
    carrier: Carrier,
    site: Site,
    partial: bool,
    shape: Shape,
    invalid: InvalidChar,
    boundary_known: bool,
    cross: bool,
    input: LengthBucket,
    pending: LengthBucket,
    recent: [EventSummary; 4],
}

impl fmt::Display for Summary {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter,
            "token_format v=1 observed={} seq={} carrier={} site={} partial={} shape={} invalid={} boundary_known={} cross={} input={} pending={} recent=[",
            self.observed, self.seq, self.carrier.label(), self.site.label(), self.partial,
            self.shape.label(), self.invalid.label(), self.boundary_known, self.cross,
            self.input.label(), self.pending.label(),
        )?;
        for (index, event) in self.recent.iter().enumerate() {
            if index != 0 {
                formatter.write_str(";")?;
            }
            write!(formatter,
                "seq={},event={},barrier_before={},frame={},structure_known={},before={},after={},text={}",
                event.seq, event.event.label(), event.barrier, event.frame.label(), event.structure_known,
                event.before.label(), event.after.label(), event.text.label(),
            )?;
        }
        formatter.write_str("]")
    }
}

#[derive(Default)]
pub(super) struct TokenFormatDiagnosticState {
    summary: Summary,
    emitted: bool,
}

impl TokenFormatDiagnosticState {
    pub(super) fn record_event(&mut self, evt: &str, barrier: bool, frame_len: usize) {
        if self.summary.observed {
            return;
        }
        self.summary.seq = self.summary.seq.saturating_add(1);
        self.summary.recent.rotate_left(1);
        self.summary.recent[3] = EventSummary {
            seq: self.summary.seq,
            event: Event::from_name(evt),
            barrier,
            frame: LengthBucket::from_len(frame_len),
            ..EventSummary::default()
        };
    }

    /// Record committed tail sizes; an unsuccessful delta keeps the prior tail size.
    pub(super) fn record_delta_structure(
        &mut self,
        pending_before: usize,
        pending_after: usize,
        text_len: usize,
    ) {
        if self.summary.observed || self.summary.seq == 0 {
            return;
        }
        let event = &mut self.summary.recent[3];
        event.structure_known = true;
        event.before = LengthBucket::from_len(pending_before);
        event.after = LengthBucket::from_len(pending_after);
        event.text = LengthBucket::from_len(text_len);
    }

    /// Observe only after the caller has identified an actual token-format failure.
    pub(super) fn observe(
        &mut self,
        text: &str,
        pending_len: usize,
        carrier: Carrier,
        site: Site,
        partial_allowed: bool,
    ) {
        if self.summary.observed {
            return;
        }
        let (shape, invalid, cross) = classify(text.as_bytes(), pending_len);
        self.summary.observed = true;
        self.summary.carrier = carrier;
        self.summary.site = site;
        self.summary.partial = partial_allowed;
        self.summary.shape = shape;
        self.summary.invalid = invalid;
        self.summary.boundary_known = matches!(site, Site::DeltaSplit | Site::DeltaRestore);
        self.summary.cross = self.summary.boundary_known && cross;
        self.summary.input = LengthBucket::from_len(text.len());
        self.summary.pending = LengthBucket::from_len(pending_len);
    }

    pub(super) fn emit_once(&mut self) {
        if !self.summary.observed || self.emitted {
            return;
        }
        self.emitted = true;
        log::warn!(target: "token_format_diagnostics", "{}", self.summary);
    }

    #[cfg(test)]
    fn summary(&self) -> Summary {
        self.summary
    }
}

fn invalid_char(byte: u8) -> InvalidChar {
    if byte.is_ascii_whitespace() {
        InvalidChar::Whitespace
    } else if matches!(
        byte,
        b'*' | b'`'
            | b'['
            | b']'
            | b'('
            | b')'
            | b'{'
            | b'}'
            | b'<'
            | b'>'
            | b'~'
            | b'#'
            | b'|'
            | b'!'
            | b'\\'
    ) {
        InvalidChar::Markdown
    } else if byte.is_ascii() {
        InvalidChar::Ascii
    } else {
        InvalidChar::NonAscii
    }
}

// Inspect borrowed bytes only; raw spans, offsets and category names never enter state.
fn classify(bytes: &[u8], pending_len: usize) -> (Shape, InvalidChar, bool) {
    let mut start = 0;
    while start < bytes.len() {
        let candidate = &bytes[start..];
        if let Some(layout) = token_format::header(candidate) {
            let cross =
                start < pending_len && pending_len - start < candidate.len().min(layout.len);
            for index in layout.digest_start..layout.len {
                let Some(&byte) = candidate.get(index) else {
                    return (Shape::Incomplete, InvalidChar::End, cross);
                };
                if !layout.valid_byte(index, byte) {
                    return (
                        if index < layout.digest_start + token_format::DIGEST_HEX_BYTES {
                            Shape::Digest
                        } else {
                            Shape::Suffix
                        },
                        invalid_char(byte),
                        cross,
                    );
                }
            }
            start += layout.len;
        } else if token_format::is_prefix(candidate) {
            return (
                Shape::Incomplete,
                InvalidChar::End,
                start < pending_len && pending_len < bytes.len(),
            );
        } else {
            start += 1;
        }
    }
    (Shape::None, InvalidChar::None, false)
}

#[cfg(test)]
mod context_tests {
    use super::*;

    #[test]
    fn reference_format_migration_diagnostics_new_grammar() {
        for name in [
            "cn_phone",
            "cn_id_card",
            "private_key",
            "api_token",
            "db_conn",
            "password",
            "infra",
            "personal",
            "custom",
            "mixed",
        ] {
            let header = format!("rredact-{name}-");
            let malformed = format!("{header}Z");
            let (shape, invalid, _) = classify(malformed.as_bytes(), 0);
            assert!(
                matches!(shape, Shape::Digest),
                "new grammar damage must be classified"
            );
            assert!(
                matches!(invalid, InvalidChar::Ascii),
                "only fixed character classes are exposed"
            );
        }
    }

    #[test]
    fn token_format_diagnostic_unknown_structure_and_boundary_are_explicit() {
        let mut state = TokenFormatDiagnosticState::default();
        state.record_event("response.output_text.delta", false, 1);
        assert!(!state.summary().recent[3].structure_known);
        state.record_delta_structure(0, 0, 1);
        assert!(state.summary().recent[3].structure_known);
        state.record_event("response.completed", true, 31);
        assert!(!state.summary().recent[3].structure_known);
        state.observe("", 0, Carrier::Other, Site::Unknown, false);
        assert!(!state.summary().boundary_known);
        assert!(state.summary().to_string().contains("boundary_known=false"));
        assert!(state.summary().to_string().contains("barrier_before="));
        assert!(state.summary().to_string().contains("structure_known="));
        for site in [
            Site::DeltaSplit,
            Site::DeltaRestore,
            Site::FinalTextRestore,
            Site::DoneRestore,
        ] {
            let mut state = TokenFormatDiagnosticState::default();
            state.observe("vary_ph_012Z", 3, Carrier::OutputText, site, false);
            assert_eq!(
                state.summary().boundary_known,
                matches!(site, Site::DeltaSplit | Site::DeltaRestore)
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    const DIGEST: &str = "0123456789abcdef0123";
    const MARKER: &str = "SYNTHETIC_PRIVATE_MARKER_7dd62";
    const MAX_SUMMARY_BYTES: usize = 1536;

    fn observe(text: &str, pending: usize, partial: bool) -> Summary {
        let mut state = TokenFormatDiagnosticState::default();
        state.record_event("response.output_text.delta", false, text.len());
        state.observe(
            text,
            pending,
            Carrier::OutputText,
            Site::DeltaSplit,
            partial,
        );
        state.summary()
    }

    #[test]
    fn token_format_diagnostic_classifies_digest_and_suffix_characters() {
        for (bad, expected) in [
            ("*", InvalidChar::Markdown),
            ("`", InvalidChar::Markdown),
            ("[", InvalidChar::Markdown),
            (" ", InvalidChar::Whitespace),
            ("\n", InvalidChar::Whitespace),
            ("\t", InvalidChar::Whitespace),
            ("Z", InvalidChar::Ascii),
            ("_", InvalidChar::Ascii),
            ("é", InvalidChar::NonAscii),
            ("🙂", InvalidChar::NonAscii),
        ] {
            let digest = observe(&format!("vary_ph_{bad}{DIGEST}_ai"), 0, false);
            assert_eq!(digest.shape, Shape::Digest);
            assert_eq!(digest.invalid, expected);
            let suffix = observe(&format!("vary_ph_{DIGEST}_a{bad}"), 0, false);
            assert_eq!(suffix.shape, Shape::Suffix);
            assert_eq!(suffix.invalid, expected);
        }
        for suffix in ["_Ai", "_aZ", "-ai"] {
            let summary = observe(&format!("vary_ph_{DIGEST}{suffix}"), 0, false);
            assert_eq!(summary.shape, Shape::Suffix);
            assert_eq!(summary.invalid, InvalidChar::Ascii);
        }
    }

    #[test]
    fn token_format_diagnostic_supported_headers_and_lowercase_hex_only() {
        for category in ["ph", "id", "pk", "ak", "db", "pw", "ip", "pi", "cu", "mx"] {
            let valid = format!("vary_{category}_{DIGEST}_ai");
            assert_eq!(observe(&valid, 0, false).shape, Shape::None);
            let bad = format!("vary_{category}_0123456789abcdef012Z_ai");
            assert_eq!(observe(&bad, 0, false).shape, Shape::Digest);
        }
        for ignored in [
            "vary_zz_0123456789abcdef012Z_ai",
            "vary_PH_0123456789abcdef012Z_ai",
            "vary_ph-0123456789abcdef012Z_ai",
            "other_ph_0123456789abcdef012Z_ai",
        ] {
            assert_eq!(observe(ignored, 0, false).shape, Shape::None);
        }
        let later = format!("vary_ph_{DIGEST}_ai then vary_id_0123456789abcdef012Z_ai");
        assert_eq!(observe(&later, 0, false).shape, Shape::Digest);
    }

    #[test]
    fn token_format_diagnostic_incomplete_headers_body_and_suffix() {
        let token = format!("vary_ph_{DIGEST}_ai");
        for end in 1..token.len() {
            for partial in [false, true] {
                let summary = observe(&token[..end], 0, partial);
                assert_eq!(summary.shape, Shape::Incomplete);
                assert_eq!(summary.invalid, InvalidChar::End);
                assert_eq!(summary.partial, partial);
            }
        }
        assert_eq!(observe("plain text", 0, false).shape, Shape::None);
        assert_eq!(observe("vary_z", 0, false).shape, Shape::None);
        assert_eq!(observe("vary_ph_0Z", 0, true).shape, Shape::Digest);
    }

    #[test]
    fn token_format_diagnostic_cross_boundary_requires_candidate_span() {
        let bad = "vary_ph_0123456789abcdef012Z_ai";
        assert!(observe(bad, 5, false).cross);
        assert!(!observe(bad, 0, false).cross);
        assert!(!observe(bad, bad.len(), false).cross);
        assert!(!observe(bad, usize::MAX, false).cross);
        let later = format!("old text {bad}");
        assert!(!observe(&later, "old text ".len(), false).cross);
        let earlier = format!("{bad} new text");
        assert!(!observe(&earlier, bad.len(), false).cross);
        let incomplete = observe("prefix vary_ph_0123", "prefix vary_".len(), false);
        assert!(incomplete.cross);
    }

    #[test]
    fn token_format_diagnostic_length_buckets_cover_extreme_lengths() {
        for (length, bucket) in [
            (0, LengthBucket::Zero),
            (1, LengthBucket::B01_07),
            (7, LengthBucket::B01_07),
            (8, LengthBucket::B08_15),
            (15, LengthBucket::B08_15),
            (16, LengthBucket::B16_30),
            (30, LengthBucket::B16_30),
            (31, LengthBucket::B31_63),
            (63, LengthBucket::B31_63),
            (64, LengthBucket::B64_255),
            (255, LengthBucket::B64_255),
            (256, LengthBucket::B256_1023),
            (1023, LengthBucket::B256_1023),
            (1024, LengthBucket::B1024Plus),
            (usize::MAX, LengthBucket::B1024Plus),
        ] {
            assert_eq!(LengthBucket::from_len(length), bucket);
        }
        let summary = observe("vary_ph_012Z", usize::MAX, false);
        assert_eq!(summary.input, LengthBucket::B08_15);
        assert_eq!(summary.pending, LengthBucket::B1024Plus);
    }

    #[test]
    fn token_format_diagnostic_recent_four_saturate_and_reset_context() {
        let mut state = TokenFormatDiagnosticState::default();
        for _ in 0..300 {
            state.record_event("response.output_text.delta", true, usize::MAX);
            state.record_delta_structure(1, 8, 31);
        }
        let summary = state.summary();
        assert_eq!(summary.seq, 255);
        assert_eq!(summary.recent.len(), 4);
        for event in summary.recent {
            assert_eq!(event.seq, 255);
            assert_eq!(event.event, Event::OutputTextDelta);
            assert!(event.barrier);
            assert_eq!(event.frame, LengthBucket::B1024Plus);
            assert_eq!(event.before, LengthBucket::B01_07);
            assert_eq!(event.after, LengthBucket::B08_15);
            assert_eq!(event.text, LengthBucket::B31_63);
        }
        for (index, name) in [
            "response.refusal.done",
            "response.reasoning_summary_text.delta",
            "response.function_call_arguments.done",
            MARKER,
        ]
        .into_iter()
        .enumerate()
        {
            state.record_event(name, false, index);
        }
        let summary = state.summary();
        assert_eq!(
            summary.recent.map(|entry| entry.event),
            [
                Event::RefusalDone,
                Event::ReasoningDelta,
                Event::FunctionDone,
                Event::Other
            ]
        );
        for event in summary.recent {
            assert_eq!(event.before, LengthBucket::Zero);
            assert_eq!(event.after, LengthBucket::Zero);
            assert_eq!(event.text, LengthBucket::Zero);
        }
        assert_eq!(summary.pending, LengthBucket::Zero);
        assert_eq!(summary.input, LengthBucket::Zero);
        assert!(!summary.observed);
    }

    #[test]
    fn token_format_diagnostic_first_failure_freezes_all_safe_context() {
        let mut state = TokenFormatDiagnosticState::default();
        state.record_event("response.custom_tool_call_input.done", true, 4096);
        state.record_delta_structure(8, 3, 100);
        state.observe(
            "vary_pw_012Z",
            5,
            Carrier::CustomInput,
            Site::FinalToolRestore,
            false,
        );
        let frozen = state.summary();
        assert!(frozen.observed);
        assert_eq!(frozen.carrier, Carrier::CustomInput);
        assert_eq!(frozen.site, Site::FinalToolRestore);
        assert_eq!(frozen.shape, Shape::Digest);
        assert_eq!(frozen.recent[3].event, Event::CustomDone);
        state.record_event(MARKER, false, usize::MAX);
        state.record_delta_structure(usize::MAX, usize::MAX, usize::MAX);
        state.observe("vary_ph_0123", 0, Carrier::Other, Site::DeltaSplit, true);
        assert_eq!(state.summary(), frozen);
    }

    #[test]
    fn token_format_diagnostic_every_site_and_carrier_survives_projection() {
        for carrier in [
            Carrier::OutputText,
            Carrier::Refusal,
            Carrier::Reasoning,
            Carrier::FunctionArguments,
            Carrier::CustomInput,
            Carrier::Other,
        ] {
            for site in [
                Site::Unknown,
                Site::DeltaSplit,
                Site::DeltaRestore,
                Site::DoneRestore,
                Site::SnapshotRestore,
                Site::FinalTextRestore,
                Site::FinalToolRestore,
            ] {
                let mut state = TokenFormatDiagnosticState::default();
                state.observe("vary_ph_012Z", 0, carrier, site, false);
                assert_eq!(state.summary().carrier, carrier);
                assert_eq!(state.summary().site, site);
            }
        }
    }

    #[test]
    fn token_format_diagnostic_summary_is_fixed_size_and_content_free() {
        assert!(!std::mem::needs_drop::<TokenFormatDiagnosticState>());
        assert!(!std::mem::needs_drop::<Summary>());
        assert!(std::mem::size_of::<TokenFormatDiagnosticState>() <= 256);
        assert!(std::mem::size_of::<Summary>() <= 256);
        let mut state = TokenFormatDiagnosticState::default();
        let payload = format!("{MARKER} vary_pw_0123456789abcdef012Z_ai private@example.invalid");
        for _ in 0..1000 {
            state.record_event(MARKER, true, 123456789);
            state.record_delta_structure(123456789, 987654321, 123456789);
        }
        state.observe(
            &payload,
            123456789,
            Carrier::FunctionArguments,
            Site::SnapshotRestore,
            false,
        );
        let summary = state.summary();
        let printed = summary.to_string();
        let debugged = format!("{summary:?}");
        for forbidden in [
            MARKER,
            "vary_",
            "0123456789abcdef",
            "private@example.invalid",
            "123456789",
            "987654321",
            "category=",
            "offset=",
            "hash=",
            "config=",
            " id=",
        ] {
            assert!(!printed.contains(forbidden));
            assert!(!debugged.contains(forbidden));
        }
        assert!(printed.len() <= MAX_SUMMARY_BYTES);
        assert!(printed.starts_with("token_format v=1 observed=true seq=255 "));
        assert!(printed.contains("shape=digest invalid=ascii"));
        assert!(printed.contains("recent=["));
    }

    struct CaptureLogger;
    static CAPTURE_LOGGER: CaptureLogger = CaptureLogger;
    static LOGS: Mutex<Vec<(log::Level, String, String)>> = Mutex::new(Vec::new());

    impl log::Log for CaptureLogger {
        fn enabled(&self, _: &log::Metadata<'_>) -> bool {
            true
        }

        fn log(&self, record: &log::Record<'_>) {
            if record.target() == "token_format_diagnostics" {
                LOGS.lock().unwrap().push((
                    record.level(),
                    record.target().to_owned(),
                    record.args().to_string(),
                ));
            }
        }

        fn flush(&self) {}
    }

    #[test]
    fn token_format_diagnostic_emit_once_logs_only_safe_warning() {
        const CHILD: &str = "VARY_TOKEN_FORMAT_DIAGNOSTIC_UNIT_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let name = concat!(
                module_path!(),
                "::token_format_diagnostic_emit_once_logs_only_safe_warning"
            );
            let test_name = name.split_once("::").unwrap().1;
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", test_name, "--nocapture"])
                .env(CHILD, "1")
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "isolated diagnostic logging test failed"
            );
            let stdout = String::from_utf8(output.stdout).unwrap();
            assert!(stdout.contains("running 1 test"));
            assert!(stdout.contains("1 passed; 0 failed"));
            return;
        }
        log::set_logger(&CAPTURE_LOGGER).unwrap();
        log::set_max_level(log::LevelFilter::Trace);
        let mut state = TokenFormatDiagnosticState::default();
        state.emit_once();
        assert!(LOGS.lock().unwrap().is_empty());
        state.record_event(MARKER, true, usize::MAX);
        let payload = format!("{MARKER} vary_ph_0123456789abcdef012Z_ai");
        state.observe(&payload, 0, Carrier::OutputText, Site::DoneRestore, false);
        let expected = state.summary().to_string();
        state.emit_once();
        state.emit_once();
        state.record_event(MARKER, false, usize::MAX);
        state.emit_once();
        let logs = LOGS.lock().unwrap();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].0, log::Level::Warn);
        assert_eq!(logs[0].1, "token_format_diagnostics");
        assert_eq!(logs[0].2, expected);
        assert!(!logs[0].2.contains(MARKER));
        assert!(!logs[0].2.contains("vary_"));
        assert!(logs[0].2.len() <= MAX_SUMMARY_BYTES);
    }
}
