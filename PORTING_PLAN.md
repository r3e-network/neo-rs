# Neo-RS Porting Plan (CLI + RPC Stack)

This document captures the next conversion increments for the missing C# modules
highlighted by `reports/parity/latest.json`.  It focuses on the user-facing CLI
and RPC layers so we can make progress in a documentation-driven manner before
updating code.

## Sources of Truth

- `reports/parity/latest.json` — generated map of matched/missing files.
- `neo_csharp/src/Neo.CLI` — reference implementation for CLI commands.
- `neo_csharp/src/Neo.ConsoleService` — shared console abstractions used by the CLI.
- `neo_csharp/src/Plugins/RpcServer` — RPC surface consumed by tooling and plugins.

## Phase 1: Neo.CLI Main Service

**Goal:** Port the CLI orchestration layer (`Neo.CLI/CLI/MainService*.cs`) so
the Rust CLI can expose parity-complete commands.

### Scope

- `MainService.CommandLine`, `MainService.Block`, `MainService.Blockchain`,
  `MainService.Contracts`, `MainService.Logger`, `MainService.NEP17`,
  `MainService.Native`, `MainService.Network`, `MainService.Node`,
  `MainService.Plugins`, `MainService.Tools`, `MainService.Vote`,
  `MainService.Wallet`.
- Supporting helpers: `CLI/Helper.cs`, `CLI/ConsolePercent.cs`,
  `CLI/CommandLineOption.cs`, `CLI/ParseFunctionAttribute.cs`.
- Entry points: `Neo.CLI/Program.cs`, `Neo.CLI/Settings.cs`.

### Approach

1. **Document API surface** — extract command names, arguments, and output
   semantics from the C# files above.  Store notes inline in this plan or
   dedicated markdown pages for each command family.
2. **Define Rust modules** — create `crates/cli/src/commands/{...}` mirroring
   the C# file breakdown (e.g. `commands/block.rs`, `commands/contracts.rs`).
3. **Gradual implementation** — start with shared utilities (`Helper`,
   `ConsolePercent`, `CommandLineOption`) so command modules can compile,
   then flesh out each `MainService.*` variant.
4. **Configuration alignment** — ensure `Neo.CLI/Settings.cs` options map onto
   the existing `NodeConfig` loader (`crates/cli/src/config.rs`) so flags/env
   match the C# behavior.

### Status & Notes

- **Documented commands:**  
  - `MainService.cs` (core shell + wallet plumbing)  
  - `MainService.CommandLine.cs` (console command registration)  
  - `MainService.Block.cs` (block inspection commands: `show block`, `show headers`, etc.)  
  - `MainService.Blockchain.cs` (chain management: `show state`, `set loglevel`, `close`, `open`)  
  - `MainService.Contracts.cs` (deploy/invoke, `deploy`, `invoke`, `testinvoke`, NEP-17 helpers)  
  - `MainService.Logger.cs` (toggle loggers / tracing)  
  - `MainService.Native.cs` (native contract operations: `list nativecontracts`, `set policy`)  
  - `MainService.NEP17.cs` (token balance queries, transfer helpers)  
  - `MainService.Network.cs` (peer queries, `show node`, `connect`, `relay`)  
  - `MainService.Node.cs` (node lifecycle: `start`, `stop`, `plugins`, `config`)  
  - `MainService.Plugins.cs` (plugin listing / enabling)  
  - `MainService.Tools.cs` (utility commands: `calc`, `hash`, `sign`, `verify`)  
  - `MainService.Vote.cs` (`vote`, `unvote`, `show candidates`)  
  - `MainService.Wallet.cs` (`open`, `close`, `list address`, `import`, `export`, `send`, `claim`)  
  - Shared helpers (`Helper.cs`, `ConsolePercent.cs`, `CommandLineOption.cs`, `ParseFunctionAttribute.cs`).  
- **Next implementation targets:** focus on shared helpers + wallet/open/close
  flow so `neo-cli` can open wallets and inspect blockchain data before adding
  more specialized commands.
- **Rust module layout:**  
  - Create `crates/cli/src/commands/mod.rs` with submodules matching the
    C# partials (`block.rs`, `blockchain.rs`, `command_line.rs`, `contracts.rs`,
    `logger.rs`, `native.rs`, `nep17.rs`, `network.rs`, `node.rs`, `plugins.rs`,
    `tools.rs`, `vote.rs`, `wallet.rs`).  
  - Shared helpers live under `crates/cli/src/console/{helper.rs, percent.rs,
    command_option.rs, parse_attribute.rs}` so both the CLI entry point and the
    console service abstractions can reuse them.  
  - `MainService` maps to a Rust `MainService` struct in `crates/cli/src/main_service.rs`
    that owns the `NeoSystem`, `LocalNode`, and `WalletProvider` trait implementation.
- **Progress:** scaffold modules under `crates/cli/src/commands/*` with placeholder
  implementations that surface "not implemented" errors referencing this plan.
  These stubs keep the crate compiling while we port each command family.

#### Detailed Notes: Helper + Wallet (Open/Close)

- `Helper.cs` exposes three utility methods used throughout the CLI:
  - `IsYes(string)` performs a case-insensitive check for `"yes"` or `"y"` and
    is used to confirm destructive operations (returns `false` for null/empty
    inputs).
  - `ToBase64String(byte[])` wraps `Convert.ToBase64String`.
  - `IsScriptValid(ReadOnlyMemory<byte>, ContractAbi)` invokes
    `SmartContract.Helper.Check` and surfaces a `FormatException` with context
    when validation fails.
- `ConsolePercent` implements a 0–100% progress indicator that either writes a
  dynamic inline progress bar (when stdin is not redirected) or prints
  full lines for redirected environments.  It tracks cursor position and color,
  exposes `Value`, `MaxValue`, and `Percent` members, and writes a newline on
  `Dispose`.  The Rust port should mimic the buffered redraw behavior so
  multi-threaded producers (e.g., `Parallel.For` in `create address`) can update
  progress safely.
- `CommandLineOptions` stores launch parameters parsed from `MainService.CommandLine`:
  `config`, `wallet`, `password`, `plugins[]`, `dbEngine`, `dbPath`, `verbose`
  (enum), `noVerify`, and `background`.  The `IsValid` property checks whether
  any field is set.
- `ParseFunctionAttribute` is a simple metadata attribute that describes script
  parsing helpers (e.g., `parse foo` commands).
- Wallet lifecycle commands (`MainService.Wallet` partial):
  - **`open wallet <path>`** — ensures the file exists, prompts for a password
    via `ConsoleHelper.ReadUserInput("password", true)`, aborts if the password
    is empty, and calls `OpenWallet(path, password)` inside
    `try/catch (CryptographicException)` to print a friendly error when the key
    derivation fails.
  - **`OpenWallet(path, password)`** — throws `FileNotFoundException` when the
    path is missing, unregisters any previously tracked wallet from
    `SignerManager`, opens the wallet via `Wallet.Open` (throwing
    `NotSupportedException` if the format is unrecognized), and registers the
    new wallet name as a signer.  This method is also used by the CLI startup
    automation that unlocks wallets based on `Settings`.
  - **`close wallet`** — early-returns when `NoWallet()` reports `true`, and when
    a wallet is present it unregisters the signer, sets `CurrentWallet` to
    `null`, and prints `"Wallet is closed"`.

**Implementation progress**

- Added `crates/cli/src/console/*` mirroring the helper types documented above.
  `StringPromptExt`, `ContractScriptValidator`, `ConsolePercent`, and the
  command-line option model now provide parity with their C# counterparts.
- `WalletCommands` manages a tracked NEP-6 wallet session (open/close) with path
  validation, password enforcement, and future-facing hooks for signer
  registration.

## Phase 2: Neo.ConsoleService

**Goal:** Provide the shared console infrastructure used by the CLI to parse
commands and print rich output.

### Scope

- `Neo.ConsoleService/CommandTokenizer.cs`, `CommandToken.cs`,
  `ConsoleCommandAttribute.cs`, `ConsoleCommandMethod.cs`,
  `ConsoleHelper.cs`, `ConsoleColorSet.cs`, `ConsoleServiceBase.cs`,
  `ServiceProxy.cs`.

### Approach

1. **Tokenizer spec** — document how `CommandTokenizer` splits input (quotes,
   escaping, etc).  Write unit tests in Rust before implementing to keep
   behavior aligned.
2. **Attribute-based routing** — map the attribute metadata to Rust macros or
   derive attributes (e.g., using `proc-macro` or simple struct tags) to retain
   declarative command definitions.
3. **Console rendering** — replicate color handling and progress rendering
   (`ConsolePercent`) using `crossterm` or `termcolor`.

### Detailed Notes: ConsoleService primitives

- **`ConsoleHelper` (`Neo.ConsoleService/ConsoleHelper.cs`)**
  - Provides color-coded logging helpers: `Info` writes tag/value pairs with
    cyan headers, `Warning` uses yellow, and `Error` uses red.  The helper tracks
    the current color via `ConsoleColorSet` so colors are restored after each
    write.
  - `ReadUserInput(prompt, password=false)` prompts the user, switches the
    foreground color to yellow, and reads keystrokes manually (echoing `*` when
    `password=true`).  Printable ASCII characters are enumerated explicitly so
    control characters are ignored; backspace deletes the previous character,
    and redirected console input falls back to `Console.ReadLine()`.  The helper
    toggles a `ReadingPassword` flag while sensitive input is collected.
  - `ReadSecureString(prompt)` mirrors `ReadUserInput` but stores the result in
    a `SecureString`, always echoing `*` regardless of console redirection, and
    returns a read-only secure value.
- **`ConsoleColorSet`** is a tiny value object capturing the current console
  colors and applying them later.  The helper instantiates a default color set to
  restore the user's colors after printing prompts or error messages.
- **`ConsoleCommandAttribute`** annotates command methods with verb sequences and
  metadata (category + description).  Verbs are lowered and split by whitespace,
  allowing commands like `"open wallet"` to be matched token-by-token.
- **`ConsoleCommandMethod`** wraps a reflected method/instance pair with the
  parsed verbs.  Its `IsThisCommand` method consumes tokens while skipping
  whitespace tokens to find matching verbs, returning the number of consumed
  tokens on success (or zero when no match).
- **`CommandToken` / `CommandTokenizer`**
  - `Tokenize()` walks the input string while tracking quote characters (`'`,
    `"`, or backtick) and escape sequences (including `\n`, `\t`, `\xFF`,
    `\u1234`).  Backtick disables escape processing entirely, matching the C#
    semantics.  Tokens retain their offsets (calculated as `index - length`),
    even for whitespace spans, so higher layers can reconstruct the raw input.
  - `CommandToken` exposes helpers like `IsWhiteSpace`, `IsIndicator`
    (prefix `"--"`), `RawValue` (re-emits quotes), and `Value`.  `CommandTokenizer`
    adds list helpers (`Trim`, `Consume`, `ConsumeAll`, `JoinRaw`) that mutate
    the working span of tokens without collapsing embedded whitespace.

**Next steps:** translate these primitives into `crates/cli/src/console_service/*`
modules, starting with `ConsoleHelper` so CLI commands can prompt for passwords
and print colored diagnostics before the full command routing layer is ported.

**Progress:** Added `crates/cli/src/console_service/{color_set,console_helper}.rs`
implementations that mirror `ConsoleColorSet` and `ConsoleHelper`, including
colored `Info`/`Warning`/`Error` logging and password-aware user input handling
based on `crossterm` raw-mode key reading.  Ported the tokenizer stack to
`console_service/{command_token,command_tokenizer}.rs` with parity helpers and
unit tests covering quoting, escaping, and token consumption utilities.
`ConsoleCommandAttribute` and `ConsoleCommandMethod` now live under
`console_service/{console_command_attribute,console_command_method}.rs`, so the
CLI can reuse the same verb registration/matching logic exposed by the C#
console service.  `console_service/argument_parser.rs` implements the sequential
and indicator parsing helpers (plus unit tests) used by `ConsoleServiceBase`,
and `command_dispatcher.rs` wires attributes, parsers, and handlers together so
Rust commands can register verbs ahead of the full interactive console.
`commands/command_line.rs` now hosts a `CommandLine` struct that registers the
first wallet verbs (`open wallet`, `close wallet`) via the dispatcher and exposes
`run_shell`, which the CLI entry (`main.rs`) now invokes so users can issue
commands while the node is running.  The shell also provides a `help` command
that lists the registered verbs using the dispatcher’s metadata, including any
descriptions defined on the C# attributes.
`ParseMode::Auto` allows the dispatcher to choose between sequential and
indicator parsing, so commands like `open wallet` accept either positional or
`--path/--password` forms, matching the C# CLI flexibility.
`main.rs` now exposes `--wallet`/`--password` flags that invoke
`WalletCommands::open_wallet` before starting the console shell, mirroring the
automatic unlock flow in C# `Settings.UnlockWallet`.
The `wallet` command group now includes `create wallet` alongside `open/close`,
using the dispatcher’s Auto mode so both positional and indicator styles are
accepted.
`list address` is now wired through the dispatcher and prints basic account
information using the shared console helper (full script-type reporting will be
ported once the necessary wallet metadata is available).  `list asset` is also
registered, currently emitting a placeholder notice while the balance queries
are being ported, `list key` prints the address/script hash/public key for
accounts with private keys, `create address` generates new accounts while
exporting them to `address.txt`, and `delete address` removes accounts from the
wallet (persisting the change immediately).

## Phase 3: RpcServer + Dependent Plugins

**Goal:** Port the RPC server plugin (`Neo.Plugins.RpcServer`) so the CLI and
plugins (ApplicationLogs, TokensTracker, StateService) can expose REST/RPC
endpoints.

### Scope

- `RpcServer.cs` partials (Blockchain, Node, SmartContract, Wallet, Utilities).
- Supporting models (`Neo.Plugins.RpcServer/Model/*`), parameter converters,
  RPC errors/exceptions, session management.
- Dependent plugin hooks (ApplicationLogs, TokensTracker, StateService) that
  register handlers via `RpcServerPlugin`.

### Approach

1. **API catalog** — list every RPC method handled in the C# partials and map
   them to the current `neo-plugins` crates so we can track implementation
   parity.
2. **Shared models** — port the `Model/*` classes (BlockHashOrIndex,
   ContractNameOrHashOrId, SignersAndWitnesses, etc.) with serde-friendly
   equivalents.
3. **Handler porting** — implement RPC methods module-by-module, reusing
   existing Rust ledger/system abstractions.  Each method should include unit
   tests (where practical) exercising serialization to match the C# expected
   output.
4. **Plugin registration** — ensure `RpcServerPlugin` is registered via the
   `neo_extensions::register_plugin!` inventory macro and that configuration
   files generated by the CLI (see `NodeConfig::write_rpc_server_plugin_config`)
   match the C# schema.

## Tracking

- Update this document as each module is documented or implemented.
- Keep `reports/parity/latest.json` checked after every major change to verify
  the missing list shrinks.
- For each completed file, reference the originating C# path to maintain traceability.
- **`ConsoleServiceBase` argument parsing helpers**
  - `ParseSequentialArguments` walks the reflected method parameters and
    consumes tokens in call order.  Each parameter is parsed via
    `TryProcessValue`, which first consults custom handlers (by type), then
    falls back to `Enum.Parse`.  When no value is present, the helper falls back
    to default parameter values, otherwise it throws `Missing value for parameter`.
  - `ParseIndicatorArguments` supports `--name value` style invocations.  Each
    indicator token is matched (case-insensitive) to a parameter, values are
    optional only for booleans, and whitespace tokens between indicator/value
    are ignored.  Missing required indicators accumulate and surface in a single
    error message.
  - The C# implementation treats `CommandToken.IsIndicator` as the marker for
    prefixed flags and assumes callers trimmed the token list before parsing.
- **Command dispatch (`ConsoleServiceBase.OnCommand`)**
  - Tokenizes the input line, checks each registered command via
    `ConsoleCommandMethod.IsThisCommand`, and attempts to parse arguments
    (indicator vs sequential) based on whether any flag tokens are present.
  - Successful matches capture `(command, arguments)` pairs; ambiguous matches
    raise `ArgumentException` while single matches invoke the reflected method
    synchronously (or `Task.Wait()` for async).  Parse errors populate a
    `possibleHelp` hint so the shell can display usage information when users
    typo a verb.
