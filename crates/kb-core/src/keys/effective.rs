//! The keymap the TUI runs: a preset plus the validated `[tui.keys.*]` overrides.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;

use super::code::{CTRL, Code, Key, KeySeq};
use super::command::Command;
use super::context::Context;
use super::parse::parse_seq;
use super::preset::{self, Keymap};

/// One `command = ["key", ...]` line of a `[tui.keys.<context>]` table, as written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyOverride {
    pub context: String,
    pub command: String,
    pub keys: Vec<String>,
    pub file: Option<PathBuf>,
    pub line: Option<usize>,
    /// The value was not a list of strings.
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyWarning {
    pub file: Option<PathBuf>,
    pub line: Option<usize>,
    pub context: Option<String>,
    pub message: String,
}

impl std::fmt::Display for KeyWarning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(p) = &self.file {
            write!(f, "{}", p.display())?;
            if let Some(l) = self.line {
                write!(f, ":{l}")?;
            }
            f.write_str(": ")?;
        }
        if let Some(c) = &self.context {
            write!(f, "[tui.keys.{c}] ")?;
        }
        f.write_str(&self.message)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    Preset,
    Config,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    pub keys: Vec<KeySeq>,
    pub origin: Origin,
    pub file: Option<PathBuf>,
    pub line: Option<usize>,
}

/// What `translate` does with a key pressed in a sub-view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Translated {
    /// Hand this key to the view's own handler.
    Key(Key),
    /// A default key the user moved elsewhere: do nothing.
    Swallow,
}

#[derive(Debug, Clone)]
pub struct EffectiveKeymap {
    pub keymap: Keymap,
    bindings: BTreeMap<(Context, Command), Binding>,
    dispatch: HashMap<(Context, KeySeq), Command>,
    preset: BTreeMap<(Context, Command), Vec<KeySeq>>,
    pub warnings: Vec<KeyWarning>,
    /// Overrides that were accepted.
    pub active_overrides: usize,
}

/// A validated override aimed at one context.
#[derive(Debug, Clone)]
struct Cand {
    idx: usize,
    ctx: Context,
    cmd: Command,
    keys: Vec<KeySeq>,
    global: bool,
}

const RESERVED: Key = Key::ctrl('c');

fn preset_keys(keymap: Keymap) -> BTreeMap<(Context, Command), Vec<KeySeq>> {
    let mut out = BTreeMap::new();
    for ctx in Context::ALL {
        for (cmd, keys) in preset::rows(keymap, ctx) {
            let parsed = keys
                .iter()
                .map(|k| parse_seq(k).unwrap_or_else(|e| panic!("preset key {k:?}: {e}")))
                .collect();
            out.insert((ctx, cmd), parsed);
        }
    }
    out
}

impl EffectiveKeymap {
    pub fn preset(keymap: Keymap) -> EffectiveKeymap {
        EffectiveKeymap::build(keymap, &[])
    }

    pub fn build(keymap: Keymap, overrides: &[KeyOverride]) -> EffectiveKeymap {
        let preset = preset_keys(keymap);
        let mut warnings = Vec::new();
        let mut cands = validate(keymap, overrides, &mut warnings);
        reject_conflicts(overrides, &mut cands, &mut warnings);
        let mut bindings;
        loop {
            bindings = apply(&preset, &cands, overrides, &mut warnings);
            // help must stay reachable in every list mode
            let lists: &[Context] = if keymap == Keymap::Vim {
                &[Context::Search, Context::Normal]
            } else {
                &[Context::Search]
            };
            let Some(&lost) = lists.iter().find(|c| {
                bindings
                    .get(&(**c, Command::Help))
                    .is_none_or(|b| b.keys.is_empty())
            }) else {
                break;
            };
            let help_keys = preset
                .get(&(lost, Command::Help))
                .cloned()
                .unwrap_or_default();
            let culprits: BTreeSet<usize> = cands
                .iter()
                .filter(|c| {
                    c.ctx == lost
                        && (c.cmd == Command::Help || c.keys.iter().any(|k| help_keys.contains(k)))
                })
                .map(|c| c.idx)
                .collect();
            if culprits.is_empty() {
                break;
            }
            for i in &culprits {
                warnings.push(warn_at(
                    overrides.get(*i),
                    format!(
                        "ignored: `{}` would be left without a help key (at least one is required)",
                        lost.name()
                    ),
                ));
            }
            cands.retain(|c| !culprits.contains(&c.idx));
            warnings.retain(|w| !w.message.starts_with("moved:"));
        }
        let active_overrides = cands.iter().map(|c| c.idx).collect::<BTreeSet<_>>().len();
        let mut dispatch = HashMap::new();
        for ((ctx, cmd), b) in &bindings {
            for k in &b.keys {
                dispatch.insert((*ctx, *k), *cmd);
            }
        }
        // a key cannot be both a command and the start of a sequence: the single key wins
        let clashes: Vec<(Context, KeySeq)> = dispatch
            .keys()
            .filter(|(c, s)| {
                s.second.is_some() && dispatch.contains_key(&(*c, KeySeq::one(s.first)))
            })
            .copied()
            .collect();
        for (c, s) in clashes {
            dispatch.remove(&(c, s));
            warnings.push(KeyWarning {
                file: None,
                line: None,
                context: Some(c.name().into()),
                message: format!(
                    "sequence \"{}\" ignored: {} is bound on its own",
                    s.name(),
                    s.first.name()
                ),
            });
        }
        EffectiveKeymap {
            keymap,
            bindings,
            dispatch,
            preset,
            warnings,
            active_overrides,
        }
    }

    pub fn binding(&self, ctx: Context, cmd: Command) -> Option<&Binding> {
        self.bindings.get(&(ctx, cmd))
    }

    /// The keys of `cmd` in `ctx`, in preset order.
    pub fn keys(&self, ctx: Context, cmd: Command) -> &[KeySeq] {
        self.bindings
            .get(&(ctx, cmd))
            .map_or(&[], |b| b.keys.as_slice())
    }

    /// Every command of `ctx` with its binding, in command order.
    pub fn context(&self, ctx: Context) -> impl Iterator<Item = (Command, &Binding)> {
        self.bindings
            .iter()
            .filter(move |((c, _), _)| *c == ctx)
            .map(|((_, cmd), b)| (*cmd, b))
    }

    pub fn lookup(&self, ctx: Context, key: Key) -> Option<Command> {
        self.dispatch.get(&(ctx, KeySeq::one(key))).copied()
    }

    pub fn lookup_seq(&self, ctx: Context, first: Key, second: Key) -> Option<Command> {
        self.dispatch
            .get(&(
                ctx,
                KeySeq {
                    first,
                    second: Some(second),
                },
            ))
            .copied()
    }

    /// `key` is the first half of some two-key sequence in `ctx`.
    pub fn starts_sequence(&self, ctx: Context, key: Key) -> bool {
        self.dispatch
            .keys()
            .any(|(c, s)| *c == ctx && s.second.is_some() && s.first == key)
    }

    /// The first key `cmd` has in `ctx`, formatted for a hint (`^O`, `Alt-A`, `j`).
    pub fn hint(&self, ctx: Context, cmd: Command) -> Option<String> {
        self.keys(ctx, cmd).first().map(|k| k.short())
    }

    /// For views that match their own keys: the default key of the command `key` is bound to
    /// (so the view's code does not change), nothing for a default key that was moved, or `key`.
    pub fn translate(&self, ctx: Context, key: Key) -> Translated {
        if let Some(cmd) = self.lookup(ctx, key) {
            let preset = self.preset.get(&(ctx, cmd));
            // a default key the user left alone goes through as it is (the view may treat
            // `down` and `tab` differently even though both mean "next")
            if preset.is_some_and(|v| v.contains(&KeySeq::one(key))) {
                return Translated::Key(key);
            }
            let canonical = preset
                .and_then(|v| v.iter().find(|s| s.second.is_none()))
                .map_or(key, |s| s.first);
            return Translated::Key(canonical);
        }
        if self
            .preset
            .iter()
            .any(|((c, _), v)| *c == ctx && v.contains(&KeySeq::one(key)))
        {
            return Translated::Swallow;
        }
        Translated::Key(key)
    }

    /// The commands of `ctx` that can be bound, for `kb keys defaults`.
    pub fn preset_keys(&self, ctx: Context, cmd: Command) -> &[KeySeq] {
        self.preset.get(&(ctx, cmd)).map_or(&[], Vec::as_slice)
    }
}

fn warn_at(o: Option<&KeyOverride>, message: String) -> KeyWarning {
    KeyWarning {
        file: o.and_then(|o| o.file.clone()),
        line: o.and_then(|o| o.line),
        context: o.map(|o| o.context.clone()),
        message,
    }
}

fn validate(
    keymap: Keymap,
    overrides: &[KeyOverride],
    warnings: &mut Vec<KeyWarning>,
) -> Vec<Cand> {
    let mut cands: Vec<Cand> = Vec::new();
    let contexts = Context::ALL.map(Context::name).join(", ");
    'next: for (idx, o) in overrides.iter().enumerate() {
        let mut warn = |m: String| warnings.push(warn_at(Some(o), m));
        if let Some(e) = &o.error {
            warn(format!("{}: {e}", o.command));
            continue;
        }
        let Some(ctx) = Context::from_name(&o.context) else {
            warn(format!(
                "unknown context {:?} (accepted: {contexts})",
                o.context
            ));
            continue;
        };
        let Some(cmd) = Command::from_name(&o.command) else {
            warn(format!(
                "unknown command {:?} (`kb keys defaults` lists them)",
                o.command
            ));
            continue;
        };
        if ctx == Context::Conflict {
            warn("the conflict screen's keys are not configurable".into());
            continue;
        }
        if ctx == Context::Normal && keymap == Keymap::Emacs {
            warn("emacs has no normal mode (use tui.keymap = \"vim\"); ignored".into());
            continue;
        }
        let targets: Vec<Context> = match ctx {
            Context::Global if keymap == Keymap::Vim => vec![Context::Search, Context::Normal],
            Context::Global => vec![Context::Search],
            c => vec![c],
        };
        let targets: Vec<Context> = targets
            .into_iter()
            .filter(|t| preset::valid(keymap, *t, cmd))
            .collect();
        if targets.is_empty() {
            warn(format!(
                "command {:?} cannot be bound in {}",
                o.command, o.context
            ));
            continue;
        }
        let mut keys: Vec<KeySeq> = Vec::new();
        for text in &o.keys {
            let seq = match parse_seq(text) {
                Ok(s) => s,
                Err(e) => {
                    warn(format!("{}: {e}", o.command));
                    continue 'next;
                }
            };
            if ctx.is_text() {
                if seq.second.is_some() {
                    warn(format!(
                        "{}: sequences are not allowed in {} (it takes typed text)",
                        o.command, o.context
                    ));
                    continue 'next;
                }
                if seq.first.is_printable()
                    && !(cmd == Command::Help && seq.first == Key::plain('?'))
                {
                    warn(format!(
                        "{}: {text:?} is a printable key and {} takes typed text",
                        o.command, o.context
                    ));
                    continue 'next;
                }
            } else if seq.second.is_some() && !ctx.allows_sequences() {
                warn(format!(
                    "{}: sequences are only supported in normal mode",
                    o.command
                ));
                continue 'next;
            }
            let reserved = seq.first == RESERVED || seq.second == Some(RESERVED);
            let may_hold_it = cmd == Command::Quit
                && seq.second.is_none()
                && (ctx.is_list() || ctx == Context::Global);
            if reserved && !may_hold_it {
                warn(format!(
                    "{}: ctrl-c is reserved and cannot be rebound",
                    o.command
                ));
                continue 'next;
            }
            if !keys.contains(&seq) {
                keys.push(seq);
            }
        }
        if cmd == Command::Quit
            && (ctx.is_list() || ctx == Context::Global)
            && !keys.contains(&KeySeq::one(RESERVED))
        {
            warn("quit: cannot unbind reserved key ctrl-c".into());
            continue;
        }
        for t in targets {
            cands.push(Cand {
                idx,
                ctx: t,
                cmd,
                keys: keys.clone(),
                global: ctx == Context::Global,
            });
        }
    }
    // a mode's own line beats `global` for the same command
    let specific: BTreeSet<(Context, Command)> = cands
        .iter()
        .filter(|c| !c.global)
        .map(|c| (c.ctx, c.cmd))
        .collect();
    cands.retain(|c| !c.global || !specific.contains(&(c.ctx, c.cmd)));
    cands
}

/// Two overrides giving the same key to different commands in one context: drop both.
fn reject_conflicts(
    overrides: &[KeyOverride],
    cands: &mut Vec<Cand>,
    warnings: &mut Vec<KeyWarning>,
) {
    let mut owners: HashMap<(Context, KeySeq), Vec<usize>> = HashMap::new();
    for (n, c) in cands.iter().enumerate() {
        for k in &c.keys {
            owners.entry((c.ctx, *k)).or_default().push(n);
        }
    }
    let mut dead: BTreeSet<usize> = BTreeSet::new(); // override indexes
    let mut conflicts: Vec<_> = owners
        .into_iter()
        .filter(|(_, v)| {
            v.iter()
                .map(|n| cands[*n].cmd)
                .collect::<BTreeSet<_>>()
                .len()
                > 1
        })
        .collect();
    conflicts.sort_by_key(|((c, k), _)| (*c, *k));
    for ((ctx, key), ns) in conflicts {
        let lines: Vec<String> = ns
            .iter()
            .filter_map(|n| overrides.get(cands[*n].idx))
            .map(|o| {
                format!(
                    "{} ({})",
                    o.command,
                    o.line.map_or("?".into(), |l| format!("line {l}"))
                )
            })
            .collect();
        let first = overrides.get(cands[ns[0]].idx);
        warnings.push(warn_at(
            first,
            format!(
                "{} is given to several commands in {}: {}; those overrides are ignored",
                key.name(),
                ctx.name(),
                lines.join(", ")
            ),
        ));
        dead.extend(ns.iter().map(|n| cands[*n].idx));
    }
    cands.retain(|c| !dead.contains(&c.idx));
}

fn apply(
    preset: &BTreeMap<(Context, Command), Vec<KeySeq>>,
    cands: &[Cand],
    overrides: &[KeyOverride],
    warnings: &mut Vec<KeyWarning>,
) -> BTreeMap<(Context, Command), Binding> {
    warnings.retain(|w| !w.message.starts_with("moved:"));
    let mut b: BTreeMap<(Context, Command), Binding> = preset
        .iter()
        .map(|(k, v)| {
            (
                *k,
                Binding {
                    keys: v.clone(),
                    origin: Origin::Preset,
                    file: None,
                    line: None,
                },
            )
        })
        .collect();
    for c in cands {
        let o = overrides.get(c.idx);
        for key in &c.keys {
            let victims: Vec<Command> = b
                .iter()
                .filter(|((x, other), bb)| *x == c.ctx && *other != c.cmd && bb.keys.contains(key))
                .map(|((_, other), _)| *other)
                .collect();
            for v in victims {
                if let Some(bb) = b.get_mut(&(c.ctx, v)) {
                    bb.keys.retain(|k| k != key);
                }
                warnings.push(warn_at(
                    o,
                    format!(
                        "moved: {} now runs `{}` in {}; `{}` lost it",
                        key.name(),
                        c.cmd.name(),
                        c.ctx.name(),
                        v.name()
                    ),
                ));
            }
        }
        b.insert(
            (c.ctx, c.cmd),
            Binding {
                keys: c.keys.clone(),
                origin: Origin::Config,
                file: o.and_then(|o| o.file.clone()),
                line: o.and_then(|o| o.line),
            },
        );
    }
    b
}

/// True when two commands share a key inside one context (used by the preset test).
pub fn duplicates(keymap: Keymap) -> Vec<String> {
    let mut seen: HashMap<(Context, KeySeq), Command> = HashMap::new();
    let mut out = Vec::new();
    for ((ctx, cmd), keys) in preset_keys(keymap) {
        for k in keys {
            if let Some(prev) = seen.insert((ctx, k), cmd) {
                out.push(format!(
                    "{}: {} is on {prev:?} and {cmd:?}",
                    ctx.name(),
                    k.name()
                ));
            }
        }
    }
    out
}

/// Ctrl-C quits in every list mode; used by tests and the TUI's own guard.
pub fn is_reserved(key: Key) -> bool {
    key.code == Code::Char('c') && key.mods == CTRL
}
