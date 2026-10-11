//! zapret2's `circular` orchestrator (zapret2 spec 5.4): read from a strategy's text, checked,
//! written back by the editor's builder, and made of several strategies by `merge`.
//!
//! In a profile, `--in-range=-s5556 --lua-desync=circular:fails=3` takes over the instances after
//! it: those tagged `strategy=K` make variant K (numbered from 1, no gaps) and only the host's
//! current variant runs; after `fails` failures for a host circular moves it to the next variant,
//! and a variant with `final` is where it stops. Only `--option=value` tokens are read, as in
//! `strategies::check`.

use serde::{Deserialize, Serialize};
use zapret::custom::quoted_tokens;

use crate::strategies::args;

/// Lua stops seeing outgoing packets past the success detector's 32768 plus one packet.
pub const OUT_RANGE: &str = "--out-range=-s34228";
/// circular counts failures on incoming packets, which are off by default: the detector's 4096
/// plus one packet.
pub const IN_RANGE: &str = "--in-range=-s5556";

/// Blobs the builder's presets use and how ALT11 defines them: added to a text that lacks them.
const BLOBS: [(&str, &str); 2] = [
    (
        "tls_google",
        r#"--blob=tls_google:@"%BIN%tls_clienthello_www_google_com.bin""#,
    ),
    ("stun2", r#"--blob=stun2:@"%BIN%stun2.bin""#),
];

/// One `--lua-desync=` call, without `strategy=` and `final`, and the `--payload` it runs under
/// (`None` for zapret2's default, `all`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instance {
    pub payload: Option<String>,
    pub call: String,
}

/// circular's own arguments: `fails` and `retrans` (3 when not given), the rest as written.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct CircularSettings {
    pub fails: Option<u32>,
    pub retrans: Option<u32>,
    /// `maxseq=16384`, `reset`, ...
    pub extra: Vec<String>,
}

impl CircularSettings {
    fn parse(args: &[&str]) -> Self {
        let mut settings = Self::default();
        for arg in args {
            let number = |key: &str| arg.strip_prefix(key)?.parse().ok();
            if let Some(fails) = number("fails=") {
                settings.fails = Some(fails);
            } else if let Some(retrans) = number("retrans=") {
                settings.retrans = Some(retrans);
            } else {
                settings.extra.push((*arg).to_owned());
            }
        }
        settings
    }

    fn call(&self) -> String {
        let mut call = vec!["circular".to_owned()];
        call.extend(self.fails.map(|n| format!("fails={n}")));
        call.extend(self.retrans.map(|n| format!("retrans={n}")));
        call.extend(self.extra.iter().filter(|arg| !arg.is_empty()).cloned());
        call.join(":")
    }
}

/// Variant `number` of a circular profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct CircularVariant {
    pub number: u32,
    /// Its instances, one `--lua-desync=` a line, `--payload=` where it changes.
    pub text: String,
    /// Has `final`: circular stops on it.
    pub is_final: bool,
}

/// A profile as the card and the builder show it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct Profile {
    /// From 0, counting every `--new`: what `CircularTarget` names.
    pub index: u32,
    /// From 1, as the card numbers profiles; 0 for one with nothing but global options.
    pub number: u32,
    /// What the profile matches: every option but the global ones and the desync part.
    pub filter: String,
    pub circular: Option<CircularSettings>,
    pub variants: Vec<CircularVariant>,
    /// The instances without `strategy=`, as a variant's text: a plain profile's whole desync.
    pub plain: String,
}

/// A strategy's profiles and what is wrong with its circular ones.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct Analysis {
    pub profiles: Vec<Profile>,
    /// What winws2 would fail on, or what cannot work: the static check refuses these.
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

/// A profile read token by token.
#[derive(Debug, Default)]
struct Parsed {
    index: u32,
    number: u32,
    filter: Vec<String>,
    circular: Option<CircularSettings>,
    circulars: u32,
    /// An `--in-range` other than `x` came before circular.
    opened: bool,
    /// After circular: `strategy=` as written, the instance, `final`.
    tagged: Vec<(String, Instance, bool)>,
    /// `strategy=` before any circular.
    orphans: u32,
    /// Without `strategy=`: before circular (they run), or all of them in a plain profile.
    plain: Vec<Instance>,
    /// Without `strategy=` after circular: never run.
    skipped: u32,
}

fn is_new(token: &str) -> bool {
    token == "--new" || token.starts_with("--new=")
}

fn is_global(token: &str) -> bool {
    ["--wf-", "--lua-init", "--blob"]
        .iter()
        .any(|prefix| token.starts_with(prefix))
}

/// The options of a profile's desync part, which the builder rewrites.
fn is_desync(token: &str) -> bool {
    ["--payload=", "--in-range=", "--out-range=", "--lua-desync="]
        .iter()
        .any(|prefix| token.replace('"', "").starts_with(prefix))
}

/// `fake:blob=x:strategy=2:final` → (`fake:blob=x`, `Some("2")`, true).
fn untag(call: &str) -> (String, Option<String>, bool) {
    let mut strategy = None;
    let mut is_final = false;
    let mut kept = Vec::new();
    for (i, arg) in call.split(':').enumerate() {
        if i > 0
            && let Some(number) = arg.strip_prefix("strategy=")
        {
            strategy = Some(number.to_owned());
        } else if i > 0 && arg == "final" {
            is_final = true;
        } else {
            kept.push(arg);
        }
    }
    (kept.join(":"), strategy, is_final)
}

fn payload_value(value: &str) -> Option<String> {
    (value != "all").then(|| value.to_owned())
}

fn parse(args: &str) -> Vec<Parsed> {
    let tokens = quoted_tokens(args);
    let mut profiles = Vec::new();
    let mut number = 0;
    for (index, chunk) in tokens.split(|token| is_new(token)).enumerate() {
        let mut p = Parsed {
            index: index as u32,
            ..Parsed::default()
        };
        let mut payload = None;
        let mut in_range: Option<String> = None;
        for token in chunk {
            let bare = token.replace('"', "");
            if is_global(&bare) {
                continue;
            }
            p.number = 1;
            if let Some(value) = bare.strip_prefix("--payload=") {
                payload = payload_value(value);
            } else if let Some(value) = bare.strip_prefix("--in-range=") {
                in_range = Some(value.to_owned());
            } else if bare.starts_with("--out-range=") {
            } else if let Some(call) = bare.strip_prefix("--lua-desync=") {
                let (call, strategy, is_final) = untag(call);
                if call == "circular" || call.starts_with("circular:") {
                    p.circulars += 1;
                    if p.circular.is_none() {
                        let args: Vec<&str> = call.split(':').skip(1).collect();
                        p.circular = Some(CircularSettings::parse(&args));
                        p.opened = in_range.as_deref().is_some_and(|range| range != "x");
                    }
                    continue;
                }
                let instance = Instance {
                    payload: payload.clone(),
                    call,
                };
                match (strategy, p.circular.is_some()) {
                    (Some(k), true) => p.tagged.push((k, instance, is_final)),
                    (Some(_), false) => p.orphans += 1,
                    (None, true) => p.skipped += 1,
                    (None, false) => p.plain.push(instance),
                }
            } else {
                p.filter.push(token.clone());
            }
        }
        if p.number > 0 {
            number += 1;
            p.number = number;
        }
        profiles.push(p);
    }
    profiles
}

impl Parsed {
    /// The tagged instances by their valid `strategy=` number, in order.
    fn variants(&self) -> Vec<(u32, Vec<Instance>, bool)> {
        let mut variants: Vec<(u32, Vec<Instance>, bool)> = Vec::new();
        for (k, instance, is_final) in &self.tagged {
            let Some(k) = k.parse().ok().filter(|k| *k > 0) else {
                continue;
            };
            match variants.iter_mut().find(|v| v.0 == k) {
                Some(variant) => {
                    variant.1.push(instance.clone());
                    variant.2 |= is_final;
                }
                None => variants.push((k, vec![instance.clone()], *is_final)),
            }
        }
        variants.sort_by_key(|v| v.0);
        variants
    }

    fn dto(&self) -> Profile {
        Profile {
            index: self.index,
            number: self.number,
            filter: self.filter.join(" "),
            circular: self.circular.clone(),
            variants: self
                .variants()
                .into_iter()
                .map(|(number, instances, is_final)| CircularVariant {
                    number,
                    text: lines(&instances),
                    is_final,
                })
                .collect(),
            plain: lines(&self.plain),
        }
    }

    fn problems(&self, errors: &mut Vec<String>, warnings: &mut Vec<String>) {
        let n = self.number;
        if self.circulars > 1 {
            errors.push(format!(
                "profile {n} has --lua-desync=circular {} times: the first one takes over every instance after it",
                self.circulars
            ));
        }
        if self.orphans > 0 {
            errors.push(format!(
                "profile {n}: {} instance(s) with strategy= come before any --lua-desync=circular, so nothing rotates them and they all run at once",
                self.orphans
            ));
        }
        if self.circular.is_none() {
            return;
        }
        if self.tagged.is_empty() {
            errors.push(format!(
                "profile {n}: --lua-desync=circular has no instances with strategy=1, 2, ... after it"
            ));
        }
        for (k, _, _) in &self.tagged {
            if !k.parse::<u32>().is_ok_and(|k| k > 0) {
                errors.push(format!("profile {n}: strategy={k} is not a number from 1"));
            }
        }
        let numbers: Vec<u32> = self.variants().iter().map(|v| v.0).collect();
        if !numbers.iter().copied().eq(1..=numbers.len() as u32) {
            let found: Vec<String> = numbers.iter().map(u32::to_string).collect();
            errors.push(format!(
                "profile {n}: strategy numbers must run 1, 2, 3... without gaps, found {}",
                found.join(", ")
            ));
        }
        if !self.opened {
            warnings.push(format!(
                "profile {n}: no --in-range before --lua-desync=circular: circular needs incoming packets to count failures (put {IN_RANGE} right before it)"
            ));
        }
        if self.skipped > 0 {
            warnings.push(format!(
                "profile {n}: {} instance(s) after --lua-desync=circular have no strategy= and never run",
                self.skipped
            ));
        }
    }
}

/// `args` (`strategies::args` of a text, or a command line) profile by profile.
pub fn analyze(args: &str) -> Analysis {
    let parsed = parse(args);
    let mut analysis = Analysis {
        profiles: parsed.iter().map(Parsed::dto).collect(),
        ..Analysis::default()
    };
    for profile in &parsed {
        profile.problems(&mut analysis.errors, &mut analysis.warnings);
    }
    analysis
}

/// `instance` as a line, after a `--payload=` line when it runs under another payload than
/// `payload` (which then follows it).
fn push(out: &mut Vec<String>, payload: &mut Option<String>, instance: &Instance, suffix: &str) {
    if instance.payload != *payload {
        payload.clone_from(&instance.payload);
        out.push(format!("--payload={}", payload.as_deref().unwrap_or("all")));
    }
    out.push(format!("--lua-desync={}{suffix}", instance.call));
}

/// `instances` as lines, starting under the default payload.
fn lines(instances: &[Instance]) -> String {
    let mut out = Vec::new();
    let mut payload = None;
    for instance in instances {
        push(&mut out, &mut payload, instance, "");
    }
    out.join("\n")
}
/// A variant as the builder holds it: `--payload=` and `--lua-desync=` lines (`#` comments
/// allowed); a `strategy=` or `final` in them is dropped, the builder numbers them.
pub fn parse_variant(text: &str) -> anyhow::Result<Vec<Instance>> {
    let mut payload = None;
    let mut instances = Vec::new();
    for token in quoted_tokens(&args(text)) {
        let bare = token.replace('"', "");
        if let Some(value) = bare.strip_prefix("--payload=") {
            payload = payload_value(value);
        } else if let Some(call) = bare.strip_prefix("--lua-desync=") {
            let (call, _, _) = untag(call);
            anyhow::ensure!(
                call != "circular" && !call.starts_with("circular:"),
                "circular goes before the variants, not in one of them"
            );
            anyhow::ensure!(!call.is_empty(), "--lua-desync= names no function");
            instances.push(Instance {
                payload: payload.clone(),
                call,
            });
        } else {
            anyhow::bail!("a variant has only --payload= and --lua-desync= options, not {token}");
        }
    }
    anyhow::ensure!(!instances.is_empty(), "a variant has no --lua-desync=");
    Ok(instances)
}

/// What the builder writes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct CircularEdit {
    pub settings: CircularSettings,
    /// Each variant's text, as `parse_variant` reads it; variant 1 first.
    pub variants: Vec<String>,
    /// The last variant is `final`: circular stops there.
    pub stop: bool,
}

/// Where the builder writes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub enum CircularTarget {
    /// This profile's desync part (`Profile::index`) becomes the circular one.
    Profile(u32),
    /// A new profile with `filter` (`--filter-tcp=443 --hostlist=...`) right after profile `after`.
    New { after: u32, filter: String },
}

/// The desync part of a circular profile: the ranges, circular, then each variant's instances
/// tagged with its number.
pub fn render(edit: &CircularEdit) -> anyhow::Result<Vec<String>> {
    anyhow::ensure!(
        !edit.variants.is_empty(),
        "circular needs at least one variant"
    );
    let mut out = vec![
        OUT_RANGE.to_owned(),
        format!("{IN_RANGE} --lua-desync={}", edit.settings.call()),
        "--in-range=x".to_owned(),
    ];
    let count = edit.variants.len();
    let mut payload = None;
    for (i, text) in edit.variants.iter().enumerate() {
        let k = i + 1;
        let instances = parse_variant(text).map_err(|err| anyhow::anyhow!("variant {k}: {err}"))?;
        for (j, instance) in instances.iter().enumerate() {
            let end = if edit.stop && k == count && j == 0 {
                ":final"
            } else {
                ""
            };
            push(
                &mut out,
                &mut payload,
                instance,
                &format!(":strategy={k}{end}"),
            );
        }
    }
    Ok(out)
}
/// `text` with `edit` written where `target` says, plus the `--blob=` lines its presets need.
/// Lines the change does not touch stay as written; a line it takes options out of is rebuilt
/// from its remaining ones.
pub fn apply(text: &str, target: &CircularTarget, edit: &CircularEdit) -> anyhow::Result<String> {
    let text = text.trim_start_matches('\u{feff}');
    let block = render(edit)?;
    let (at, insert, rewrite) = match target {
        CircularTarget::Profile(index) => (*index, block, true),
        CircularTarget::New { after, filter } => {
            let tokens = quoted_tokens(filter);
            anyhow::ensure!(!tokens.is_empty(), "a new profile needs a filter");
            if let Some(token) = tokens.iter().find(|t| is_new(t) || is_desync(t)) {
                anyhow::bail!("{token} does not belong in a profile's filter");
            }
            let mut insert = vec![
                "--new".to_owned(),
                String::new(),
                "# circular".to_owned(),
                tokens.join(" "),
            ];
            insert.extend(block);
            (*after, insert, false)
        }
    };
    let count = parse(&args(text)).len() as u32;
    anyhow::ensure!(at < count, "the strategy has no profile {}", at + 1);

    let mut out: Vec<String> = Vec::new();
    let mut profile = 0;
    let mut pending = Some(insert);
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            out.push(line.to_owned());
            continue;
        }
        let mut kept: Vec<String> = Vec::new();
        let mut changed = false;
        for token in quoted_tokens(line) {
            if is_new(&token) {
                if profile == at
                    && let Some(insert) = pending.take()
                {
                    if !kept.is_empty() {
                        out.push(std::mem::take(&mut kept).join(" "));
                    }
                    out.extend(insert);
                    changed = true;
                }
                profile += 1;
            } else if rewrite && profile == at && is_desync(&token) {
                changed = true;
                continue;
            }
            kept.push(token);
        }
        if !changed {
            out.push(line.to_owned());
        } else if !kept.is_empty() {
            out.push(kept.join(" "));
        }
    }
    if let Some(insert) = pending {
        out.extend(insert);
    }
    add_blobs(&mut out, edit);
    let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let end = if text.ends_with('\n') { newline } else { "" };
    Ok(out.join(newline) + end)
}

/// The `BLOBS` an edit's instances name and `out` does not define, after the last global line.
fn add_blobs(out: &mut Vec<String>, edit: &CircularEdit) {
    let defined = |name: &str| {
        let prefix = format!("--blob={name}:");
        out.iter()
            .filter(|line| !line.trim_start().starts_with('#'))
            .flat_map(|line| quoted_tokens(line))
            .any(|token| token.starts_with(&prefix))
    };
    let named = |name: &str| {
        edit.variants.iter().any(|text| {
            quoted_tokens(&args(text)).iter().any(|token| {
                token
                    .split(':')
                    .any(|arg| arg.split_once('=').is_some_and(|(_, value)| value == name))
            })
        })
    };
    let missing: Vec<String> = BLOBS
        .iter()
        .filter(|(name, _)| named(name) && !defined(name))
        .map(|(_, line)| (*line).to_owned())
        .collect();
    if missing.is_empty() {
        return;
    }
    let at = out
        .iter()
        .rposition(|line| {
            !line.trim_start().starts_with('#')
                && quoted_tokens(line)
                    .iter()
                    .any(|t| t.starts_with("--blob=") || t.starts_with("--lua-init="))
        })
        .map_or(0, |i| i + 1);
    out.splice(at..at, missing);
}

/// What `merge` did with a profile, for its preview.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub enum NoteKind {
    /// Now rotates between `variants` sets of instances, the first of each from `names`.
    Rotates,
    /// The same in every strategy: kept.
    Same,
    /// Not in `names` (no profile with the same filter): taken from the first.
    Missing,
    /// Not TCP only: circular counts failures by TCP retransmissions. Taken from the first.
    NotTcp,
    /// Already circular in `names`: taken from the first.
    Circular,
    /// No desync in `names` (passes traffic as it is): taken from the first.
    NoDesync,
    /// A profile of `names[0]` that the first strategy does not have: left out.
    LeftOut,
    /// `names[0]` defines the blob `filter` otherwise than the first: the first's is kept.
    BlobConflict,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct MergeNote {
    pub kind: NoteKind,
    /// The profile's number in the first strategy (in `names[0]` for `LeftOut`); 0 for a blob.
    pub number: u32,
    /// The profile's filter, or the blob's name.
    pub filter: String,
    pub names: Vec<String>,
    pub variants: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct Merged {
    pub text: String,
    pub notes: Vec<MergeNote>,
}

/// One strategy of `sources` (name, text) made of all of them: the first one's text, where each
/// TCP profile that every strategy has (the same filter options, in any order) and whose
/// instances differ between them becomes a circular profile with one variant per distinct set,
/// in the order of `sources`. Everything else stays as in the first one; the notes say why. The
/// `--blob=` and `--lua-init=` options of the others that the first lacks are added.
pub fn merge(sources: &[(String, String)]) -> anyhow::Result<Merged> {
    anyhow::ensure!(
        (2..=5).contains(&sources.len()),
        "pick 2 to 5 strategies to merge, not {}",
        sources.len()
    );
    let parsed: Vec<Vec<Parsed>> = sources.iter().map(|(_, text)| parse(&args(text))).collect();
    let key = |p: &Parsed| {
        let mut tokens: Vec<String> = p.filter.iter().map(|t| t.replace('"', "")).collect();
        tokens.sort();
        tokens.join(" ")
    };
    let mut used: Vec<Vec<bool>> = parsed.iter().map(|p| vec![false; p.len()]).collect();
    let mut notes = Vec::new();
    let mut edits = Vec::new();
    let names: Vec<String> = sources.iter().map(|(name, _)| name.clone()).collect();
    for p in parsed[0].iter().filter(|p| p.number > 0) {
        let note = |kind, names: Vec<String>, variants| MergeNote {
            kind,
            number: p.number,
            filter: p.filter.join(" "),
            names,
            variants,
        };
        let mut matched = vec![(0, p)];
        let mut missing = Vec::new();
        for (s, profiles) in parsed.iter().enumerate().skip(1) {
            let found = profiles
                .iter()
                .position(|q| q.number > 0 && !used[s][q.index as usize] && key(q) == key(p));
            match found {
                Some(j) => {
                    used[s][j] = true;
                    matched.push((s, &profiles[j]));
                }
                None => missing.push(names[s].clone()),
            }
        }
        let of = |pick: &dyn Fn(&Parsed) -> bool| -> Vec<String> {
            matched
                .iter()
                .filter(|(_, q)| pick(q))
                .map(|(s, _)| names[*s].clone())
                .collect()
        };
        let circular = of(&|q| q.circular.is_some() || q.orphans > 0);
        let bare = of(&|q| q.plain.is_empty());
        if !missing.is_empty() {
            notes.push(note(NoteKind::Missing, missing, 0));
            continue;
        }
        if !circular.is_empty() {
            notes.push(note(NoteKind::Circular, circular, 0));
            continue;
        }
        let mut variants: Vec<(&Vec<Instance>, String)> = Vec::new();
        for (s, q) in &matched {
            if !variants.iter().any(|(instances, _)| **instances == q.plain) {
                variants.push((&q.plain, names[*s].clone()));
            }
        }
        if variants.len() == 1 {
            notes.push(note(NoteKind::Same, Vec::new(), 1));
            continue;
        }
        if !bare.is_empty() {
            notes.push(note(NoteKind::NoDesync, bare, 0));
            continue;
        }
        let tcp = p.filter.iter().any(|t| t.starts_with("--filter-tcp="))
            && !p.filter.iter().any(|t| t.starts_with("--filter-udp="));
        if !tcp {
            notes.push(note(NoteKind::NotTcp, Vec::new(), 0));
            continue;
        }
        let count = variants.len() as u32;
        let from = variants.iter().map(|(_, name)| name.clone()).collect();
        edits.push((
            p.index,
            CircularEdit {
                settings: CircularSettings::default(),
                variants: variants
                    .iter()
                    .map(|(instances, _)| lines(instances))
                    .collect(),
                stop: false,
            },
        ));
        notes.push(note(NoteKind::Rotates, from, count));
    }
    for (s, profiles) in parsed.iter().enumerate().skip(1) {
        for q in profiles
            .iter()
            .filter(|q| q.number > 0 && !used[s][q.index as usize])
        {
            notes.push(MergeNote {
                kind: NoteKind::LeftOut,
                number: q.number,
                filter: q.filter.join(" "),
                names: vec![names[s].clone()],
                variants: 0,
            });
        }
    }

    let globals = |text: &str| -> Vec<String> {
        quoted_tokens(&args(text))
            .into_iter()
            .filter(|t| {
                let bare = t.replace('"', "");
                bare.starts_with("--blob=") || bare.starts_with("--lua-init=")
            })
            .collect()
    };
    let blob_name = |token: &str| {
        let bare = token.replace('"', "");
        bare.strip_prefix("--blob=")
            .and_then(|rest| rest.split_once(':'))
            .map(|(name, _)| name.to_owned())
    };
    let mut have = globals(&sources[0].1);
    let mut added = Vec::new();
    for (name, text) in sources.iter().skip(1) {
        for token in globals(text) {
            let bare = token.replace('"', "");
            if have.iter().any(|t| t.replace('"', "") == bare) {
                continue;
            }
            if let Some(blob) = blob_name(&token)
                && have.iter().any(|t| blob_name(t).as_deref() == Some(&blob))
            {
                notes.push(MergeNote {
                    kind: NoteKind::BlobConflict,
                    number: 0,
                    filter: blob,
                    names: vec![name.clone()],
                    variants: 0,
                });
                continue;
            }
            have.push(token.clone());
            added.push(token);
        }
    }

    let base = sources[0].1.trim_start_matches('\u{feff}');
    let newline = if base.contains("\r\n") { "\r\n" } else { "\n" };
    let mut text = format!(
        "# circular over {}: the profiles they share rotate between their variants{newline}",
        names.join(", ")
    );
    for token in added {
        text.push_str(&token);
        text.push_str(newline);
    }
    text.push_str(base);
    for (index, edit) in edits {
        text = apply(&text, &CircularTarget::Profile(index), &edit)?;
    }
    Ok(Merged { text, notes })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_variant_starting_under_another_payload_says_its_own() {
        let edit = CircularEdit {
            settings: CircularSettings::default(),
            variants: vec![
                "--payload=tls_client_hello\n--lua-desync=fake:blob=x".into(),
                "--lua-desync=multidisorder".into(),
                "--lua-desync=multisplit".into(),
            ],
            stop: true,
        };
        assert_eq!(
            render(&edit).unwrap(),
            [
                OUT_RANGE,
                "--in-range=-s5556 --lua-desync=circular",
                "--in-range=x",
                "--payload=tls_client_hello",
                "--lua-desync=fake:blob=x:strategy=1",
                "--payload=all",
                "--lua-desync=multidisorder:strategy=2",
                "--lua-desync=multisplit:strategy=3:final",
            ]
        );
    }
}
