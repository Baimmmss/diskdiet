---
name: mannerly
description: Check and fix the behavior of a command-line tool (help, exit codes, error messages, color in pipes, NO_COLOR, closed pipes, startup time, Ctrl-C). Use after creating or changing a CLI in any language, when asked to polish a CLI, or before releasing one.
---

# mannerly: give the CLI good manners

`mannerly` runs a command the way people, scripts and coding agents do (with `--help`, with a bad flag, in a pipe, in a real terminal, with `NO_COLOR=1`, with Ctrl-C) and scores it 0-100. Every finding says why it matters and how to fix it.

## 1. Make sure mannerly is installed

```bash
command -v mannerly || cargo install mannerly
```

If cargo isn't available, use the install script:

```bash
curl -fsSL https://raw.githubusercontent.com/Baimmmss/mannerly/main/install.sh | sh
```

## 2. Build the CLI, then check the built binary

Check the binary or the exact command people will run, not the source file:

```bash
mannerly --json ./target/debug/mytool      # Rust
mannerly --json ./bin/mytool               # Go (after go build -o bin/mytool)
mannerly --json -- python -m mytool        # Python module
mannerly --json -- node ./bin/cli.js       # Node
```

Use `--` before a command that has its own flags. For a subcommand, pass it as a prefix: `mannerly --json mytool build`.

Safety: mannerly also runs the command **with no arguments**. If the tool does something destructive or slow with no arguments, add `--no-bare`.

## 3. Read the JSON

- `score`: 0-100. `categories`: help, errors, output, robustness.
- `checks[]`: `id`, `status` (`pass`, `warn`, `fail`, `skip`), `detail` (what was observed), `why`, `fix`.

Fix every `fail`, then `warn`s that make sense for this tool. `skip` means not applicable; ignore it. For details on one check: `mannerly --explain <id>`.

## 4. Fix, rebuild, re-run

Repeat until there are no `fail`s. Don't game the checks (for example, don't add the word "example" to help without real examples). If a check is genuinely wrong for this tool, skip it with `--skip <id>` and say why.

## Fix recipes

### Rust (clap)
- `help.examples`, `help.docs`: `#[command(after_help = "Examples:\n  mytool build\n\nDocs: https://...")]`.
- `help.version`: `#[command(version)]`.
- `errors.*`: clap already exits 2 with a message on stderr and suggests typos. Keep `Cli::parse()`, don't catch the error and exit 0.
- `output.pipe-color`, `output.no-color`: use `anstream`/`anstyle` or check `std::io::stdout().is_terminal()` and `NO_COLOR` before emitting color.
- `output.closed-pipe`: `println!` panics on a closed pipe. Write with `writeln!(stdout, ...)` and exit quietly on `ErrorKind::BrokenPipe`, or reset SIGPIPE at the start of `main` on Unix: `unsafe { libc::signal(libc::SIGPIPE, libc::SIG_DFL); }`.
- `help.width`: enable clap's `wrap_help` feature, or set `max_term_width = 100`.

### Go (cobra / flag)
- `help.stdout`: the standard `flag` package prints `-h`/`--help` output to stderr. Use `fs := flag.NewFlagSet(name, flag.ContinueOnError)`, silence its output, and when `fs.Parse` returns `flag.ErrHelp`, print usage to stdout and exit 0; for other errors print to stderr and exit 2.
- cobra: set `cmd.Example`; `SilenceUsage: true` keeps errors short (`errors.concise`); return errors from `RunE`; exit with `os.Exit(2)` for usage errors via `cmd.SetFlagErrorFunc`.
- `--version`: set `rootCmd.Version`.
- `errors.suggest`: cobra suggests by default; don't set `DisableSuggestions`.
- Color: `lipgloss`/`termenv` respect NO_COLOR; don't force a color profile.

### Python (argparse / click / typer)
- argparse: `epilog="Examples:\n  mytool build"` with `formatter_class=argparse.RawDescriptionHelpFormatter`; `parser.add_argument("--version", action="version", version="%(prog)s 1.2.3")`. argparse exits 2 on bad flags.
- click/typer: `@click.version_option()`; examples in the docstring or `epilog`.
- `errors.no-traceback`: catch expected exceptions in `main()` and print one line to stderr, `sys.exit(1)`.
- `output.closed-pipe`: wrap output in `try: ... except BrokenPipeError: os.dup2(os.open(os.devnull, os.O_WRONLY), sys.stdout.fileno()); sys.exit(1)`, or on Unix `signal.signal(signal.SIGPIPE, signal.SIG_DFL)` at startup.
- `robust.startup`: move heavy imports into the functions that need them.

### Node (commander / yargs)
- commander: `.version(pkg.version)`, `.addHelpText("after", "\nExamples:\n  $ mytool build")`; commander exits 1 on unknown options. Use `program.exitOverride()` to exit 2 if you want `errors.usage-code`.
- yargs: `.example(...)`, `.strict()` (rejects unknown flags), `.recommendCommands()` (typo suggestions).
- `errors.no-traceback`: catch errors at the top level: `main().catch(e => { console.error(e.message); process.exit(1) })`.
- `output.closed-pipe`: `process.stdout.on("error", e => { if (e.code === "EPIPE") process.exit(0) })`.
- Color: `chalk`/`picocolors` respect NO_COLOR and pipes; don't set `FORCE_COLOR` in code.

### Shell scripts
- Handle `-h|--help` and `--version` in a `case` before anything else; send errors to stderr with `>&2` and `exit 2`.

## Done when

`mannerly <command>` shows no failures, or each remaining failure is explained to the user. For CI, suggest `mannerly --min-score 90 <command>` or the GitHub Action in the mannerly README.
