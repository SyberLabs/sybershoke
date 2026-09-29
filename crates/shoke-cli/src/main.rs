//! `shoke`: shock a system, then check what happened.

mod report;

use shoke_core::{check, History, Invariant};
use shoke_jev::invariants::{deadline_scope, limits};
use shoke_jev::{
    default_set, minimal, simulate, sweep, Bug, Config, JevFault, Mix, Profile, Scenario, SweepOpts,
};
use std::collections::BTreeSet;
use std::process::ExitCode;

const USAGE: &str = "\
shoke: shock a system, then check what happened

USAGE
  shoke run     [options]     simulate one seeded scenario, check it, exit 1 on a violation
  shoke sweep   [options]     many seeds: how often is each invariant broken
  shoke shrink  [options]     reduce the faults of one failing seed to a minimal set
  shoke check   FILE [--require-floor] [--all]
                              check a shoke-history/v1 file written by anything
  shoke report  [--seeds N]   the full campaign as markdown (see scripts/report.sh)
  shoke bugs                  list the seeded bugs

OPTIONS
  --seed N          scenario seed (run, shrink) or first seed (sweep)      [0]
  --seeds N         number of seeds (sweep, report)                        [200]
  --requests N      requests per scenario                                  [60]
  --faults N        injected faults per scenario                           [2]
  --mix CLASS       fault class: all|http|slow|cold|truncate|menu          [all]
  --profile P       jev | kev-warm | kev-cold                              [jev]
  --floor           the system under test has a preset floor
  --require-floor   also check I7: every request is answered with a plan
  --bug NAME        enable a seeded bug (repeatable; see `shoke bugs`)
  --out FILE        write the history to FILE (run)
  --invariant ID    the invariant to shrink for (shrink)                   [first failing]
  --shrink          in sweep: also shrink the first failing seed of each invariant
  --max-runs N      shrinker budget                                        [500]
  --all             in check: print every violation, not the first five

EXIT CODES
  0 no violation   1 violation found   2 usage or input error
";

struct Opts {
    seed: u64,
    seeds: u64,
    requests: usize,
    faults: usize,
    mix: Mix,
    profile: Profile,
    floor: bool,
    require_floor: bool,
    bugs: BTreeSet<Bug>,
    out: Option<String>,
    invariant: Option<String>,
    shrink: bool,
    max_runs: u32,
    all: bool,
    positional: Vec<String>,
}

impl Default for Opts {
    fn default() -> Self {
        Opts {
            seed: 0,
            seeds: 200,
            requests: 60,
            faults: 2,
            mix: Mix::All,
            profile: Profile::Jev,
            floor: false,
            require_floor: false,
            bugs: BTreeSet::new(),
            out: None,
            invariant: None,
            shrink: false,
            max_runs: 500,
            all: false,
            positional: Vec::new(),
        }
    }
}

fn number<T: std::str::FromStr>(flag: &str, value: Option<&String>) -> Result<T, String> {
    let v = value.ok_or_else(|| format!("{flag} needs a value"))?;
    v.parse()
        .map_err(|_| format!("{flag}: `{v}` is not a valid number"))
}

fn parse_opts(args: &[String]) -> Result<Opts, String> {
    let mut o = Opts::default();
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--seed" => o.seed = number("--seed", it.next())?,
            "--seeds" => o.seeds = number("--seeds", it.next())?,
            "--requests" => o.requests = number("--requests", it.next())?,
            "--faults" => o.faults = number("--faults", it.next())?,
            "--max-runs" => o.max_runs = number("--max-runs", it.next())?,
            "--mix" => {
                let v = it.next().ok_or("--mix needs a value")?;
                o.mix = Mix::parse(v).ok_or_else(|| format!("unknown fault class `{v}`"))?;
            }
            "--profile" => {
                let v = it.next().ok_or("--profile needs a value")?;
                o.profile = Profile::parse(v).ok_or_else(|| format!("unknown profile `{v}`"))?;
            }
            "--bug" => {
                let v = it.next().ok_or("--bug needs a value")?;
                o.bugs.insert(
                    Bug::parse(v).ok_or_else(|| format!("unknown bug `{v}` (try `shoke bugs`)"))?,
                );
            }
            "--out" => o.out = Some(it.next().ok_or("--out needs a value")?.clone()),
            "--invariant" => {
                o.invariant = Some(it.next().ok_or("--invariant needs a value")?.clone())
            }
            "--floor" => o.floor = true,
            "--require-floor" => o.require_floor = true,
            "--shrink" => o.shrink = true,
            "--all" => o.all = true,
            flag if flag.starts_with("--") => return Err(format!("unknown option `{flag}`")),
            _ => o.positional.push(arg.clone()),
        }
    }
    Ok(o)
}

/// Inputs that would otherwise report a PASS for work that was never done.
fn generated(o: &Opts) -> Result<(), String> {
    if let Some(extra) = o.positional.first() {
        return Err(format!("unexpected argument `{extra}`"));
    }
    if o.requests == 0 {
        return Err("--requests must be at least 1".into());
    }
    Ok(())
}

fn seed_range(o: &Opts) -> Result<(), String> {
    if o.seeds == 0 {
        return Err("--seeds must be at least 1".into());
    }
    o.seed
        .checked_add(o.seeds - 1)
        .map(|_| ())
        .ok_or_else(|| format!("seeds {} + {} run past the largest seed", o.seed, o.seeds))
}

fn config_of(o: &Opts) -> Config {
    let mut cfg = Config::new(o.profile);
    cfg.floor = o.floor;
    cfg.bugs = o.bugs.clone();
    cfg
}

fn describe(o: &Opts) -> String {
    let bugs: Vec<&str> = o.bugs.iter().map(|b| b.name()).collect();
    format!(
        "profile={} floor={} bugs={} mix={} faults={} requests={}",
        o.profile.name(),
        if o.floor { "on" } else { "off" },
        if bugs.is_empty() {
            "none".to_string()
        } else {
            bugs.join(",")
        },
        o.mix.name(),
        o.faults,
        o.requests
    )
}

fn fault_lines(faults: &[JevFault]) -> String {
    if faults.is_empty() {
        return "  (no faults)\n".to_string();
    }
    faults.iter().map(|f| format!("  {f}\n")).collect()
}

fn cmd_run(o: &Opts) -> Result<ExitCode, String> {
    generated(o)?;
    let cfg = config_of(o);
    let sc = Scenario::generate(o.seed, o.requests, o.faults, o.mix);
    let history = simulate(&cfg, &sc);
    if let Some(path) = &o.out {
        std::fs::write(path, history.to_text()).map_err(|e| format!("cannot write {path}: {e}"))?;
    }
    let invs = default_set(o.require_floor);
    let report = check(&invs, &history);
    println!("run  seed={} {}", o.seed, describe(o));
    print!("faults:\n{}", fault_lines(&sc.faults));
    print!("{}", report.render());
    if let Some(path) = &o.out {
        println!("history written to {path}");
    }
    Ok(if report.passed() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    })
}

fn print_minimal(
    cfg: &Config,
    sc: &Scenario,
    invs: &[Box<dyn Invariant>],
    id: &str,
    max_runs: u32,
) {
    let out = minimal(cfg, sc, invs, id, max_runs);
    println!(
        "  {id} seed {}: {} fault(s) -> {} after {} runs",
        sc.seed,
        sc.faults.len(),
        out.faults.len(),
        out.runs
    );
    print!("{}", fault_lines(&out.faults));
}

fn cmd_sweep(o: &Opts) -> Result<ExitCode, String> {
    generated(o)?;
    seed_range(o)?;
    let cfg = config_of(o);
    let invs = default_set(o.require_floor);
    let opts = SweepOpts {
        start: o.seed,
        seeds: o.seeds,
        requests: o.requests,
        faults: o.faults,
        mix: o.mix,
    };
    let result = sweep(&cfg, &opts, &invs);
    println!("sweep {} seeds={} (from {})", describe(o), o.seeds, o.seed);
    for inv in &invs {
        let n = result.count(inv.id());
        let first = result
            .first_seed
            .get(inv.id())
            .map(|s| format!("  first failing seed {s}"))
            .unwrap_or_default();
        println!(
            "{:<4}{:<28}{:>5}/{}{}",
            inv.id(),
            inv.name(),
            n,
            o.seeds,
            first
        );
    }
    if o.shrink && !result.first_seed.is_empty() {
        println!("shrunk:");
        for (id, seed) in &result.first_seed {
            let sc = Scenario::generate(*seed, o.requests, o.faults, o.mix);
            print_minimal(&cfg, &sc, &invs, id, o.max_runs);
        }
    }
    Ok(if result.failing_seeds.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    })
}

fn cmd_shrink(o: &Opts) -> Result<ExitCode, String> {
    generated(o)?;
    let cfg = config_of(o);
    let invs = default_set(o.require_floor);
    let sc = Scenario::generate(o.seed, o.requests, o.faults, o.mix);
    let failing = check(&invs, &simulate(&cfg, &sc)).failing();
    let id = match &o.invariant {
        Some(id) => id.clone(),
        None => failing
            .first()
            .map(|s| s.to_string())
            .ok_or("this seed does not fail any invariant; nothing to shrink")?,
    };
    if !failing.contains(&id.as_str()) {
        return Err(format!(
            "seed {} does not fail {id}; it fails {failing:?}",
            o.seed
        ));
    }
    println!("shrink {}", describe(o));
    print_minimal(&cfg, &sc, &invs, &id, o.max_runs);
    Ok(ExitCode::SUCCESS)
}

fn cmd_check(o: &Opts) -> Result<ExitCode, String> {
    let path = match o.positional.as_slice() {
        [path] => path,
        [] => return Err("check needs a FILE".into()),
        [_, extra, ..] => return Err(format!("unexpected argument `{extra}`")),
    };
    let text = std::fs::read_to_string(path).map_err(|e| format!("cannot read {path}: {e}"))?;
    let history = History::from_text(&text).map_err(|e| format!("{path}: {e}"))?;
    let (deadline, cap) = limits(&history).map_err(|e| format!("{path}: {e}"))?;
    let scope = deadline_scope(&history).map_err(|e| format!("{path}: {e}"))?;
    if history.of_kind("req").next().is_none() {
        return Err(format!("{path}: no requests, so nothing to check"));
    }
    let invs = default_set(o.require_floor);
    let report = check(&invs, &history);
    // The file sets its own bar, so say which bar was applied.
    println!(
        "check {path}  ({} events, {} meta)  deadline_ms={deadline} max_calls={cap} deadline_scope={}",
        history.events.len(),
        history.meta.len(),
        scope.as_str()
    );
    if o.all {
        print!("{}", report.render_up_to(usize::MAX));
    } else {
        print!("{}", report.render());
    }
    Ok(if report.passed() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    })
}

fn cmd_report(o: &Opts) -> Result<ExitCode, String> {
    generated(o)?;
    seed_range(o)?;
    print!(
        "{}",
        report::render(o.seeds, o.requests, o.faults, o.max_runs)
    );
    Ok(ExitCode::SUCCESS)
}

fn cmd_bugs() -> Result<ExitCode, String> {
    for b in Bug::ALL {
        println!("{:<24} {}  {}", b.name(), b.target(), b.describe());
    }
    Ok(ExitCode::SUCCESS)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(cmd) = args.first().cloned() else {
        print!("{USAGE}");
        return ExitCode::from(2);
    };
    if matches!(cmd.as_str(), "-h" | "--help" | "help") {
        print!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    let result = parse_opts(&args[1..]).and_then(|o| match cmd.as_str() {
        "run" => cmd_run(&o),
        "sweep" => cmd_sweep(&o),
        "shrink" => cmd_shrink(&o),
        "check" => cmd_check(&o),
        "report" => cmd_report(&o),
        "bugs" => cmd_bugs(),
        other => Err(format!("unknown command `{other}` (try `shoke help`)")),
    });
    match result {
        Ok(code) => code,
        Err(msg) => {
            eprintln!("shoke: {msg}");
            ExitCode::from(2)
        }
    }
}
